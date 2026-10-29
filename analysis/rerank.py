"""concat-rerank: the semantic half of `openvid_search`.

BM25 finds the clips whose captions SAY the words. This one looks at the
frames and finds the clips that SHOW the meaning: the query and each
candidate's keyframe go through MobileCLIP2-S0 (ONNX, CPU), and the
candidates come back ordered by what their pictures have to do with what
was asked. "The moment a decision goes wrong" then reaches worried faces
and hesitation, not every caption that carries the word "decision".

It is a rerank step, not a search: the caller (the MCP's `openvid_search`)
still runs BM25 first, hands over the shortlist with the local files it
could get, and falls back to the BM25 order untouched when anything here
is missing - the models, the wheels, the tokenizer. A search never fails
because the semantic half is not installed.

Models: two ONNX exports of MobileCLIP2-S0, `mobileclip2_s0_vision.onnx`
and `mobileclip2_s0_text.onnx`, looked up in `$FUNCUT_MOBILECLIP`, beside
this file, the app data dir's `models/mobileclip2-s0/`, and
`~/.cache/concat/mobileclip2-s0/`, in that order. The tokenizer is the
stock CLIP BPE MobileCLIP was trained with: it is read from the model
directory when `vocab.json`/`merges.txt` sit beside the ONNX files, else
from Hugging Face (`openai/clip-vit-base-patch32`, the same vocabulary
MobileCLIP's own loader uses), else from an installed `open_clip`. The
frames are decoded with the same ffmpeg the sheet uses; no bytes leave
the machine at any step.
"""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

VISION_ONNX = "mobileclip2_s0_vision.onnx"
TEXT_ONNX = "mobileclip2_s0_text.onnx"
INPUT_SIDE = 256
MEAN = 0.5
STD = 0.5
PIP_LINE = "pip install onnxruntime numpy  # plus: pip install transformers (the tokenizer)"


def _model_dirs():
    """Where the two ONNX files may sit, most specific first."""
    dirs = []
    env = os.environ.get("FUNCUT_MOBILECLIP")
    if env:
        dirs.append(Path(env))
    dirs.append(Path(__file__).resolve().parent)
    app = os.environ.get(
        "FUNCUT_VLM_DIR", str(Path.home() / ".local" / "share" / "app.concat.editor")
    )
    dirs.append(Path(app) / "models" / "mobileclip2-s0")
    dirs.append(Path.home() / ".cache" / "concat" / "mobileclip2-s0")
    return dirs


def find_models():
    """The two model files, or (None, why they are not there)."""
    for directory in _model_dirs():
        vision = directory / VISION_ONNX
        text = directory / TEXT_ONNX
        if vision.is_file() and text.is_file():
            return vision, text
    wanted = ", ".join(str(directory) for directory in _model_dirs()[:2])
    return None, (
        f"MobileCLIP2-S0 ONNX models not found ({VISION_ONNX}, {TEXT_ONNX}); "
        f"put them in $FUNCUT_MOBILECLIP or {wanted}"
    )


def _tokenizer():
    """The CLIP BPE tokenizer MobileCLIP was trained with.

    Local vocabulary first, then Hugging Face, then an installed open_clip
    - the first that answers wins.
    """
    from transformers import CLIPTokenizer

    for directory in _model_dirs():
        if (directory / "vocab.json").is_file() and (directory / "merges.txt").is_file():
            return CLIPTokenizer.from_pretrained(str(directory)), "files"
    try:
        return CLIPTokenizer.from_pretrained("openai/clip-vit-base-patch32"), "huggingface"
    except Exception:
        from open_clip.tokenizer import SimpleTokenizer

        return SimpleTokenizer(), "open_clip"


def _ids_for(tokenizer, kind, text, length):
    """The query as one row of int64 ids, padded to the model's own length.

    open_clip's SimpleTokenizer hands back unpadded ids ending in the EOS
    token, which is exactly CLIP's own padding scheme once zeros follow;
    transformers' does the padding itself.
    """
    if kind == "open_clip":
        ids = tokenizer.encode(text)[:length]
        ids = ids + [0] * (length - len(ids))
        return ids
    batch = tokenizer(text, padding="max_length", truncation=True, return_tensors="np")
    return batch["input_ids"][0].tolist()[:length]


def _sessions(vision_path, text_path):
    import onnxruntime as ort

    options = ort.SessionOptions()
    options.intra_op_num_threads = max(1, (os.cpu_count() or 2) // 2)
    options.inter_op_num_threads = 1
    vision = ort.InferenceSession(str(vision_path), options, providers=["CPUExecutionProvider"])
    text = ort.InferenceSession(str(text_path), options, providers=["CPUExecutionProvider"])
    return vision, text


def _input_name(session, hints):
    """The input whose name carries one of the hints, else the first.

    ONNX exports name their inputs after the exporter, not after a spec:
    `tokens`, `input_ids`, `pixel_values`, `image`, whatever the tool of
    the day chose. The name decides where it can; order keeps the rest.
    """
    names = [info.name for info in session.get_inputs()]
    for hint in hints:
        for name in names:
            if hint in name.lower():
                return name
    return names[0]


def _output_of(session, wanted_dim=None):
    """The output carrying the embedding: the one the width fits."""
    outputs = session.get_outputs()
    if len(outputs) == 1:
        return outputs[0].name
    for info in outputs:
        shape = info.shape
        if shape and isinstance(shape[-1], int) and (wanted_dim is None or shape[-1] == wanted_dim):
            return info.name
    return outputs[0].name


def _preprocess(frame):
    """RGB bytes into the tensor the vision tower wants."""
    import numpy as np

    array = np.frombuffer(frame, dtype=np.uint8).reshape(INPUT_SIDE, INPUT_SIDE, 3)
    array = array.astype(np.float32) / 255.0
    array = (array - MEAN) / STD
    return array.transpose(2, 0, 1)[None, :, :, :].copy()


def _normalise(vector):
    import numpy as np

    vector = np.asarray(vector, dtype=np.float32).reshape(-1)
    size = float(np.linalg.norm(vector))
    return vector / size if size > 0.0 else vector


def _cid(candidate):
    """A candidate's id as an int, or None: one malformed entry must cost
    that candidate its ranking, not the whole reply (a bare `int()` raised
    before any score could be returned)."""
    try:
        return int(candidate["id"])
    except (KeyError, TypeError, ValueError):
        return None


def rerank(spec):
    """Score the candidates' frames against the query, and order by score.

    Spec: `query`, `frames` [{id, path, at}], `candidates` (the BM25 page,
    its own order, each carrying its `id`), and the ffmpeg facts the sheet
    carries (`ffmpeg`, `autoSetup`). Reply: `order` - every candidate id,
    the scored first by score and the unscored in the BM25 order they came
    in - plus `scores` and the timing. Nothing here raises at the caller:
    every failure is a structured reply the Rust side reads and steps
    over, leaving the BM25 page as it was.
    """
    started = time.perf_counter()
    try:
        import numpy as np  # noqa: F401 - both wheels, checked together
        import onnxruntime  # noqa: F401
    except Exception as error:
        return {"ok": False, "error": f"python extras missing: {error}", "pip": PIP_LINE}

    vision_path, text_path = find_models()
    if vision_path is None:
        return {"ok": False, "error": text_path, "pip": PIP_LINE}

    query = str(spec.get("query") or "").strip()
    if not query:
        return {"ok": False, "error": "no query to rank by"}
    frames = [frame for frame in spec.get("frames") or [] if frame.get("path")]
    candidates = spec.get("candidates") or []
    if not candidates:
        return {"ok": False, "error": "no candidates to rank"}

    try:
        vision_session, text_session = _sessions(vision_path, text_path)
        tokenizer, kind = _tokenizer()
        text_shape = text_session.get_inputs()[0].shape
        length = text_shape[-1] if isinstance(text_shape[-1], int) else 77
        text_in = _input_name(text_session, ["token", "text", "word", "input"])
        query_vector = _normalise(
            text_session.run(
                [_output_of(text_session)],
                {text_in: np.asarray([_ids_for(tokenizer, kind, query, length)], dtype=np.int64)},
            )[0]
        )
        dimension = int(query_vector.shape[0])

        import sense  # the runtime dir this file is unpacked into

        ff, _ = sense._tools(spec)
        vision_in = _input_name(vision_session, ["image", "pixel", "visual", "input"])
        wanted = _output_of(vision_session, dimension)
        best: dict[int, float] = {}
        for frame in frames:
            identifier = frame.get("id")
            if identifier is None:
                continue
            pixels, error = sense.extract_frame(
                ff, frame["path"], float(frame.get("at") or 0.0), INPUT_SIDE, INPUT_SIDE
            )
            if pixels is None:
                continue
            vector = _normalise(
                vision_session.run([wanted], {vision_in: _preprocess(pixels)})[0]
            )
            if int(vector.shape[0]) != dimension:
                return {
                    "ok": False,
                    "error": (
                        f"the two towers disagree ({int(vector.shape[0])} vs {dimension}); "
                        "the ONNX pair is not one MobileCLIP2-S0 export"
                    ),
                }
            score = float(np.dot(query_vector, vector))
            # The caller sends two looks per clip (early and middle): a clip
            # whose subject only appears mid-clip must be ranked on what it
            # shows there, not discarded because the early frame was scored
            # first. Keep each clip's best frame.
            previous = best.get(identifier)
            if previous is None or score > previous:
                best[identifier] = score
    except Exception as error:  # a broken half is a fallback, not a failure
        return {"ok": False, "error": f"{type(error).__name__}: {error}", "pip": PIP_LINE}

    scored = [candidate for candidate in candidates if _cid(candidate) in best]
    kept = [candidate for candidate in candidates if _cid(candidate) not in best]
    scored.sort(key=lambda candidate: -best[_cid(candidate)])
    order = [_cid(candidate) for candidate in scored]
    order += [_cid(candidate) for candidate in kept]
    return {
        "ok": True,
        "order": order,
        "scores": {str(key): round(value, 6) for key, value in best.items()},
        "scored": len(scored),
        "ms": round((time.perf_counter() - started) * 1000.0),
        "engine": "mobileclip2-s0",
    }


def main():
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("cmd", choices=["rerank", "selftest"])
    parser.add_argument("--spec", default="{}")
    args = parser.parse_args()
    raw = sys.stdin.read() if args.spec == "-" else args.spec
    try:
        spec = json.loads(raw)
    except ValueError as error:
        print(json.dumps({"ok": False, "error": f"the spec is not JSON: {error}"}))
        return
    if not isinstance(spec, dict):
        print(json.dumps({"ok": False, "error": "spec must be a JSON object"}))
        return
    if args.cmd == "selftest":
        found, reason = find_models()
        print(json.dumps({"ok": found is not None, "error": reason}))
        return
    print(json.dumps(rerank(spec)))


if __name__ == "__main__":
    main()
