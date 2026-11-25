#!/usr/bin/env python3
"""The acceptance test for the sound model layer: P@64, measured.

Run it after changing a model, a query, or a role weight. It prints the
table in `docs/SOUND-MODEL.md` §3 - CLAP alone, the measurements alone, and
the base rate - over the shipped shelf, using the curator's categories as
weak labels.

This is a development tool, not a sidecar. It is the one place in the repo
that may use numpy, onnxruntime, tokenizers and the reference
`transformers.ClapFeatureExtractor`, because its job is to check the
production front-end against the reference implementation rather than to
run inside FUNCUT. It is not run by CI: it needs the models downloaded and
about four minutes of CPU.

    pip install numpy onnxruntime tokenizers transformers
    python3 scripts/sound-model/bench.py --models ~/.local/share/funcut/models

Exit code is 0 when the table reproduces the documented numbers to within
0.03, and 1 when it does not - drift that large means the layer changed.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import numpy as np

DOCUMENTED = {
    # role: (clap, measure)  - docs/SOUND-MODEL.md §3
    "quick_transition": (0.62, 0.38),
    "gentle_entrance": (0.73, 0.64),
    "action_impact": (0.59, 0.64),
    "tension_build": (0.31, 0.44),
    "emotional_texture": (0.31, 0.42),
    "cinematic_reveal": (0.23, 0.23),
}

QUERIES = {
    "quick_transition": ["a short bright whoosh of air passing by", "a fast swish transition",
                         "air moving quickly, a swoosh"],
    "cinematic_reveal": ["a deep heavy cinematic braam", "a dark dramatic reveal boom",
                         "a low sustained cinematic swell"],
    "action_impact": ["a sharp heavy metal impact", "a punchy hard hit", "a slam, an impact hit"],
    "gentle_entrance": ["a soft small interface click", "a gentle light chime",
                        "a soft pop, a subtle UI tick"],
    "emotional_texture": ["a warm sustained pad, an emotional drone", "a soft ambient bed",
                          "a gentle emotional tone"],
    "tension_build": ["a rising tension riser building up", "an orchestral swell that climbs",
                      "a suspense build with increasing energy"],
}

POSITIVE = {
    "action_impact": {"Impact"},
    "tension_build": {"Riser"},
    "cinematic_reveal": {"Ambience"},
    "emotional_texture": {"Ambience", "Music"},
    "gentle_entrance": {"UI", "Notification", "Keyboard", "Writing"},
    "quick_transition": {"Whoosh", "Motion"},
}


def decode(path, ffmpeg, seconds=10.0):
    done = subprocess.run([ffmpeg, "-v", "error", "-i", str(path), "-ac", "1", "-ar", "48000",
                           "-t", str(seconds), "-f", "f32le", "-"], capture_output=True)
    return np.frombuffer(done.stdout, dtype=np.float32)


def unit(vector):
    return vector / max(np.linalg.norm(vector), 1e-9)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--models", default=str(Path.home() / ".local/share/funcut/models"))
    parser.add_argument("--shelf", default=str(Path(__file__).resolve().parents[2] / "audio"))
    parser.add_argument("--cache", default="/tmp/funcut-clap-embeddings.json")
    args = parser.parse_args()

    import onnxruntime as ort
    from tokenizers import Tokenizer
    from transformers import ClapFeatureExtractor

    models, shelf, cache = Path(args.models), Path(args.shelf), Path(args.cache)
    ffmpeg = os.environ.get("FUNCUT_FFMPEG") or shutil.which("ffmpeg")
    if not ffmpeg:
        sys.exit("no ffmpeg: put one on PATH or set FUNCUT_FFMPEG")

    # the preprocessor config sits beside the weights when it has been
    # downloaded, so the bench can run offline
    source = str(models) if (models / "preprocessor_config.json").is_file() \
        else "laion/clap-htsat-unfused"
    extractor = ClapFeatureExtractor.from_pretrained(source)
    audio = ort.InferenceSession(str(models / "clap_audio.onnx"), providers=["CPUExecutionProvider"])
    text = ort.InferenceSession(str(models / "clap_text.onnx"), providers=["CPUExecutionProvider"])
    tokenizer = Tokenizer.from_file(str(models / "clap_tokenizer.json"))
    tokenizer.no_padding()
    tokenizer.no_truncation()

    manifest = json.loads((shelf / "manifest.json").read_text())
    files = [entry["file"] for entry in manifest["sounds"]]
    category = {entry["file"]: entry.get("category", "") for entry in manifest["sounds"]}

    embeddings = json.loads(cache.read_text()) if cache.is_file() else {}
    for index, name in enumerate(files, 1):
        if name in embeddings:
            continue
        wave = decode(shelf / name, ffmpeg)
        if not wave.size:
            continue
        features = extractor(wave, sampling_rate=48000, return_tensors="np")["input_features"]
        vector = audio.run(None, {"input_features": features.astype(np.float32)})[0][0]
        embeddings[name] = unit(vector).tolist()
        if index % 100 == 0:
            cache.write_text(json.dumps(embeddings))
            print(f"  embedded {index}/{len(files)}", flush=True)
    cache.write_text(json.dumps(embeddings))

    def text_vector(phrase):
        ids = tokenizer.encode(phrase).ids[:77]
        ids += [1] * (77 - len(ids))
        return unit(text.run(None, {"input_ids": np.array([ids], np.int64)})[0][0])

    names = [name for name in files if name in embeddings]
    matrix = np.array([embeddings[name] for name in names])

    index_path = shelf / "sound-index.json"
    if not index_path.is_file():
        sys.exit(f"no measurement index at {index_path}: run sound_sense.py index_library first")
    measured = {entry["file"]: entry["roles"]
                for entry in json.loads(index_path.read_text())["sounds"]}

    def p_at_64(score, positive, k=64):
        order = np.argsort(-score)
        return sum(1 for i in order[:k] if category[names[i]] in positive) / k

    print(f"\n{'role':<18}{'clap':>8}{'measure':>9}{'base':>7}{'doc clap':>10}{'doc meas':>10}")
    worst = 0.0
    for role, positive in POSITIVE.items():
        clap = (matrix @ np.array([text_vector(q) for q in QUERIES[role]]).T).max(axis=1)
        fit = np.array([measured[name][role] for name in names])
        clap_p, fit_p = p_at_64(clap, positive), p_at_64(fit, positive)
        base = sum(1 for name in names if category[name] in positive) / len(names)
        doc_clap, doc_fit = DOCUMENTED[role]
        worst = max(worst, abs(clap_p - doc_clap), abs(fit_p - doc_fit))
        print(f"{role:<18}{clap_p:>8.2f}{fit_p:>9.2f}{base:>7.2f}"
              f"{doc_clap:>10.2f}{doc_fit:>10.2f}")

    verdict = "within tolerance" if worst <= 0.03 else "DRIFTED - re-measure and update the doc"
    print(f"\nlargest drift from docs/SOUND-MODEL.md: {worst:.3f} ({verdict})")
    return 0 if worst <= 0.03 else 1


if __name__ == "__main__":
    sys.exit(main())
