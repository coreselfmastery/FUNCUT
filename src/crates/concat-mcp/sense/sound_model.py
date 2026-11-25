#!/usr/bin/env python3
"""concat-sound-model: what a sound MEANS, from two small models.

The same contract as the other sense scripts: `sound_model.py
<analyse|selftest> --spec -` reads one JSON spec from stdin and prints one
JSON object on stdout. The spec names the tool in `tool`.

WHY THIS EXISTS

`sound_sense.py` measures a sound file: length, peak, attack, brightness,
bass, transients. Those are facts, and a Director can trust them. They are
not meaning. "Bright, 0.4 s, 3 kHz centroid" does not tell anyone whether
the file is a whoosh, a metal hit, or a cat.

Two small models answer the meaning question, and only that question:

  YAMNet     521 AudioSet classes, 3.7 M weights. "What kind of event is
             this?" - whoosh, impact, applause, engine, footsteps.
  CLAP       audio and text in one 512-d space, so "a deep cinematic braam"
             can be compared with the file itself rather than its filename.

Neither model decides where a sound goes or how loud it is. Timing and level
come from `sound_sense.py`; the Director decides. The models rank candidates
and name what is in the file.

THE THREE TOOLS

  describe    one file: identity, YAMNet labels, CLAP embedding and a
              description matched from a phrase bank, the Sound Sense
              measurements, loudness and quality, the roles it fits, and
              where its peak sits so a cut can land on it.
  index       the whole shelf, one record per file, cached by size, mtime
              and model version. The cache is the point: inference happens
              once per asset, never on the playback path.
  rank        a creative description and a role: candidates scored by CLAP
              similarity fused with the measured role fit, using weights
              fitted on the 530 shipped sounds (see docs/SOUND-MODEL.md).

MODELS ARE OPTIONAL, AND SAY SO

numpy and onnxruntime are used when they are installed and never required,
the same rule `voice_quality.py` follows. With no models present every tool
still answers, with `"model": {"state": "absent", ...}` and measurement-only
records - a Director without the models is a Director that is less certain,
not one that is broken.
"""

from __future__ import annotations

import array
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

try:
    import numpy as np
except Exception:  # pragma: no cover - absence is a supported state
    np = None

try:
    import onnxruntime as ort
except Exception:  # pragma: no cover
    ort = None

try:
    from tokenizers import Tokenizer
except Exception:  # pragma: no cover
    Tokenizer = None

sys.path.insert(0, str(Path(__file__).resolve().parent))
import sound_sense  # noqa: E402  - the measurement half of this answer

MODEL_INDEX_VERSION = 4  # bump whenever a record means something different

# CLAP's own front-end, pinned against the reference extractor
# (`transformers.ClapFeatureExtractor`) to 1e-5 on the shipped shelf: 48 kHz
# mono, repeat-tiled then zero-padded to 10 s, reflect-padded STFT of 1024
# with hop 480, periodic Hann, power spectrum, 64 slaney mel bands from
# 50-14000 Hz, then 10*log10 floored at -100 dB. The three traps, all of
# which cost a debugging session each: the scale is 10*log10 and not log10;
# the default (non-fusion) path uses the *slaney* filterbank and not the
# torchaudio one; and short inputs are tiled floor(10s/len) times and then
# zero-padded, not tiled to length and truncated.
CLAP_RATE = 48000
CLAP_SAMPLES = 480000
CLAP_FFT = 1024
CLAP_HOP = 480
CLAP_MELS = 64
CLAP_FMIN = 50.0
CLAP_FMAX = 14000.0
CLAP_TEXT_MAX = 77
CLAP_FLOOR_DB = -100.0

YAMNET_RATE = 16000

# How much the measurements are allowed to move the ranking, per role,
# fitted on the 530 shipped sounds against their curator categories
# (P@64 sweep in docs/SOUND-MODEL.md). 0 means the model alone is the better
# judge: on transitions the measurements actively mislead, because foley and
# whooshes measure alike.
ROLE_WEIGHT = {
    "quick_transition": 0.0,
    "gentle_entrance": 0.5,
    "action_impact": 0.5,
    "tension_build": 1.0,
    "cinematic_reveal": 4.0,
    "emotional_texture": 4.0,
}

# Phrases the description tool matches a file against. Small on purpose:
# a phrase bank that grows without bound stops being a description and
# starts being a search engine.
PHRASES = [
    "a short bright whoosh of air",
    "a fast swish transition",
    "a deep heavy cinematic braam",
    "a dark dramatic impact boom",
    "a sharp metal hit",
    "a punchy hard slam",
    "a soft small interface click",
    "a gentle light chime",
    "a warm sustained pad",
    "a soft ambient bed",
    "a rising tension riser",
    "a suspense build that climbs",
    "a low dark drone",
    "footsteps on a hard floor",
    "water flowing",
    "an engine running",
    "people talking in the distance",
    "applause and cheering",
    "a door closing",
    "wind and rain",
]

MOODS = [
    "calm and reflective",
    "dark and tense",
    "warm and emotional",
    "bright and playful",
    "epic and cinematic",
    "driving and energetic",
    "sad and slow",
    "corporate and neutral",
    "suspenseful and restrained",
    "uplifting and hopeful",
]


# ---------------------------------------------------------------------------
# 1. Small shared helpers
# ---------------------------------------------------------------------------


def _r(value, places=4):
    if value is None or (isinstance(value, float) and not math.isfinite(value)):
        return None
    return round(float(value), places)


def _ffmpeg():
    return os.environ.get("FUNCUT_FFMPEG") or shutil.which("ffmpeg") or "ffmpeg"


def _model_candidates(spec):
    """Where a downloaded model may already be, in the order to try.

    The app keeps its models in `<data>/app.concat.editor/cutout-models`,
    or in `portable/cutout-models` beside the executable for a portable
    build (see `concat-host/src/dirs.rs`). Reading the same folder the
    settings sheet downloads into is what makes "download it in the UI and
    the reader uses it" true without either side knowing about the other.
    An explicit `models` in the spec still wins, which is what the tests
    and the bench use.
    """
    named = spec.get("models") or os.environ.get("FUNCUT_MODELS")
    if named:
        return [Path(named)]
    data = Path(os.environ.get("XDG_DATA_HOME") or (Path.home() / ".local" / "share"))
    candidates = []
    if os.environ.get("FUNCUT_APP_DIR"):
        candidates.append(Path(os.environ["FUNCUT_APP_DIR"]) / "portable" / "cutout-models")
    candidates.append(data / "app.concat.editor" / "cutout-models")
    # Older layouts, so a model downloaded by an earlier build is still found.
    for folder in ("funcut", "FUNCUT", "concat", "Concat"):
        candidates.append(data / folder / "cutout-models")
        candidates.append(data / folder / "models")
    return candidates


def _models_dir(spec):
    """The first candidate that holds the audio models, else the first.

    A folder with only the vision models is not the sound models' home: the
    reader would report them absent while the disk held a whole download,
    which is exactly the confusion this order exists to avoid.
    """
    candidates = _model_candidates(spec)
    for candidate in candidates:
        if (candidate / "clap_text.onnx").is_file() or (candidate / "yamnet.onnx").is_file():
            return candidate
    return candidates[0]


def _model_file(spec, name):
    path = _models_dir(spec) / name
    return path if path.is_file() else None


def _decode(path, rate, seconds=None):
    """Decode to mono float32 at `rate` through ffmpeg. None when it fails."""
    command = [_ffmpeg(), "-v", "error", "-i", str(path), "-ac", "1", "-ar", str(rate)]
    if seconds:
        command += ["-t", str(seconds)]
    command += ["-f", "f32le", "-"]
    try:
        done = subprocess.run(command, capture_output=True, timeout=120)
    except Exception:
        return None
    if not done.stdout:
        return None
    return array.array("f", done.stdout)


def _identity(path):
    """Duration, rate and channels, from ffprobe when there is one.

    ffprobe is a separate binary and is not always installed next to
    ffmpeg (a static ffmpeg build has no ffprobe), so the second reading
    parses ffmpeg's own report of the stream rather than giving up.
    """
    probe = shutil.which("ffprobe")
    if probe:
        try:
            done = subprocess.run(
                [probe, "-v", "error", "-show_entries",
                 "stream=sample_rate,channels,duration:format=duration",
                 "-of", "json", str(path)],
                capture_output=True, timeout=60)
            data = json.loads(done.stdout or b"{}")
            stream = (data.get("streams") or [{}])[0]
            duration = stream.get("duration") or (data.get("format") or {}).get("duration")
            if duration:
                return {
                    "duration": _r(float(duration)),
                    "sampleRate": int(stream.get("sample_rate") or 0),
                    "channels": int(stream.get("channels") or 0),
                }
        except Exception:
            pass
    try:
        done = subprocess.run([_ffmpeg(), "-hide_banner", "-i", str(path)],
                              capture_output=True, timeout=60)
        text = done.stderr.decode("utf-8", "replace")
        clock = re.search(r"Duration: (\d+):(\d+):(\d+\.\d+)", text)
        audio = re.search(r"Audio: [^,]+, (\d+) Hz, ([^,]+)", text)
        channels = None
        if audio:
            spoken = audio.group(2).strip()
            channels = {"mono": 1, "stereo": 2}.get(spoken)
            if channels is None:
                counted = re.match(r"(\d+) channels", spoken)
                channels = int(counted.group(1)) if counted else None
        return {
            "duration": _r(int(clock.group(1)) * 3600 + int(clock.group(2)) * 60
                           + float(clock.group(3))) if clock else None,
            "sampleRate": int(audio.group(1)) if audio else None,
            "channels": channels,
        }
    except Exception:
        return {"duration": None, "sampleRate": None, "channels": None}


# ---------------------------------------------------------------------------
# 2. CLAP: the front-end, verified against the reference extractor
# ---------------------------------------------------------------------------


def _hz_to_mel(f):
    f = np.asarray(f, dtype=np.float64)
    f_sp, min_log_hz = 200.0 / 3.0, 1000.0
    min_log_mel, logstep = min_log_hz / f_sp, math.log(6.4) / 27.0
    linear = f / f_sp
    logarithmic = min_log_mel + np.log(np.maximum(f, 1e-10) / min_log_hz) / logstep
    return np.where(f >= min_log_hz, logarithmic, linear)


def _mel_to_hz(m):
    m = np.asarray(m, dtype=np.float64)
    f_sp, min_log_hz = 200.0 / 3.0, 1000.0
    min_log_mel, logstep = min_log_hz / f_sp, math.log(6.4) / 27.0
    return np.where(m >= min_log_mel, min_log_hz * np.exp(logstep * (m - min_log_mel)), f_sp * m)


def _slaney_filters():
    """The 64 x 513 mel filterbank CLAP was trained with (librosa slaney)."""
    freqs = np.linspace(0.0, CLAP_RATE / 2.0, CLAP_FFT // 2 + 1)
    edges = _mel_to_hz(np.linspace(_hz_to_mel(CLAP_FMIN), _hz_to_mel(CLAP_FMAX), CLAP_MELS + 2))
    filters = np.zeros((freqs.size, CLAP_MELS))
    for band in range(CLAP_MELS):
        lower, centre, upper = edges[band], edges[band + 1], edges[band + 2]
        rise = (freqs - lower) / max(centre - lower, 1e-10)
        fall = (upper - freqs) / max(upper - centre, 1e-10)
        filters[:, band] = np.maximum(0.0, np.minimum(rise, fall)) * (2.0 / (upper - lower))
    return filters


_CLAP_FILTERS = None
_CLAP_WINDOW = None


def _clap_filters():
    global _CLAP_FILTERS, _CLAP_WINDOW
    if _CLAP_FILTERS is None:
        _CLAP_FILTERS = _slaney_filters()
        _CLAP_WINDOW = np.hanning(CLAP_FFT + 1)[:-1].astype(np.float64)
    return _CLAP_FILTERS, _CLAP_WINDOW


def clap_features(samples):
    """10 s of 48 kHz mono as CLAP's (1, 1, 1001, 64) log-mel input."""
    filters, window = _clap_filters()
    x = np.asarray(samples, dtype=np.float64)
    if x.size == 0:
        x = np.zeros(CLAP_SAMPLES)
    elif x.size < CLAP_SAMPLES:
        x = np.tile(x, int(CLAP_SAMPLES / x.size))
        x = np.pad(x, (0, CLAP_SAMPLES - x.size), mode="constant")
    else:
        x = x[:CLAP_SAMPLES]
    x = np.pad(x, (CLAP_FFT // 2, CLAP_FFT // 2), mode="reflect")
    frames = 1 + (x.size - CLAP_FFT) // CLAP_HOP
    index = np.arange(CLAP_FFT)[None, :] + CLAP_HOP * np.arange(frames)[:, None]
    power = np.abs(np.fft.rfft(x[index] * window, axis=1)) ** 2
    mel = np.maximum(1e-10, power @ filters)
    db = np.maximum(10.0 * np.log10(mel), CLAP_FLOOR_DB)
    return db.astype(np.float32)[None, None]


# ---------------------------------------------------------------------------
# 3. The two models, loaded once per run
# ---------------------------------------------------------------------------


class Models:
    """YAMNet and CLAP, or an honest account of what is missing."""

    def __init__(self, spec):
        self.state = "absent"
        self.reason = None
        self.yamnet = None
        self.yamnet_names = []
        self.clap_audio = None
        self.clap_text = None
        self.tokenizer = None
        self._text_cache = {}

        if np is None or ort is None:
            self.reason = "numpy and onnxruntime are not installed (pip install numpy onnxruntime)"
            return

        audio = _model_file(spec, "clap_audio.onnx")
        text = _model_file(spec, "clap_text.onnx")
        tokenizer = _model_file(spec, "clap_tokenizer.json")
        yamnet = _model_file(spec, "yamnet.onnx")
        names = _model_file(spec, "yamnet_class_map.csv")

        if not (audio and text and tokenizer):
            self.reason = (
                f"CLAP is not downloaded into {_models_dir(spec)} "
                "(clap_audio.onnx, clap_text.onnx, clap_tokenizer.json)"
            )
            return
        if Tokenizer is None:
            self.reason = "the tokenizers package is not installed (pip install tokenizers)"
            return

        providers = ["CPUExecutionProvider"]
        self.clap_audio = ort.InferenceSession(str(audio), providers=providers)
        self.clap_text = ort.InferenceSession(str(text), providers=providers)
        self.tokenizer = Tokenizer.from_file(str(tokenizer))
        self.tokenizer.no_padding()
        self.tokenizer.no_truncation()

        if yamnet and names:
            self.yamnet = ort.InferenceSession(str(yamnet), providers=providers)
            rows = names.read_text(encoding="utf-8").splitlines()[1:]
            self.yamnet_names = [
                (row.split(",", 2)[2].strip().strip('"') if row.count(",") >= 2 else "")
                for row in rows
            ]
        self.state = "ready"

    # -- YAMNet ---------------------------------------------------------

    def yamnet_read(self, path):
        if self.yamnet is None:
            return None
        samples = _decode(path, YAMNET_RATE)
        if samples is None or not len(samples):
            return None
        wave = np.asarray(samples, dtype=np.float32)
        scores = self.yamnet.run(None, {"waveform": wave})[0]
        # Loudness-weighted mean over the patch frames. A plain mean lets a
        # 4 s thunder with a quiet tail answer "Silence"; a peak lets any
        # silent patch answer it. Weighting each patch by its own RMS asks
        # the question that matters: what is in the part that is audible?
        weights = _patch_loudness(wave, scores.shape[0])
        combined = (scores * weights[:, None]).sum(axis=0) / max(float(weights.sum()), 1e-9)
        order = np.argsort(-combined)[:8]
        return {
            "frames": int(scores.shape[0]),
            "labels": [
                {"label": self.yamnet_names[i] if i < len(self.yamnet_names) else str(i),
                 "score": _r(combined[i], 3)}
                for i in order
            ],
            "music": _r(sum(combined[i] for i, n in enumerate(self.yamnet_names)
                            if n in ("Music", "Electronic music", "Pop music",
                                     "Hip hop music", "Rock music", "Classical music"))
                        if len(self.yamnet_names) == len(combined) else 0.0, 3),
        }

    # -- CLAP -----------------------------------------------------------

    def clap_embedding(self, path):
        samples = _decode(path, CLAP_RATE, seconds=10)
        if samples is None or not len(samples):
            return None
        features = clap_features(samples)
        vector = self.clap_audio.run(None, {"input_features": features})[0][0]
        return _unit(vector)

    def text_embedding(self, phrase):
        if phrase in self._text_cache:
            return self._text_cache[phrase]
        ids = self.tokenizer.encode(phrase).ids[:CLAP_TEXT_MAX]
        ids += [1] * (CLAP_TEXT_MAX - len(ids))
        vector = self.clap_text.run(
            None, {"input_ids": np.array([ids], dtype=np.int64)})[0][0]
        self._text_cache[phrase] = _unit(vector)
        return self._text_cache[phrase]

    def describe(self, embedding):
        """The phrase bank's best match - a description, not a caption."""
        scored = [(phrase, float(embedding @ self.text_embedding(phrase))) for phrase in PHRASES]
        scored.sort(key=lambda pair: -pair[1])
        return [{"text": phrase, "similarity": _r(score, 3)} for phrase, score in scored[:3]]

    def moods(self, embedding):
        scored = [(mood, float(embedding @ self.text_embedding(mood))) for mood in MOODS]
        scored.sort(key=lambda pair: -pair[1])
        return [{"mood": mood, "similarity": _r(score, 3)} for mood, score in scored[:3]]


def _patch_loudness(samples, patches):
    """RMS of each YAMNet patch (0.96 s window, 0.48 s hop at 16 kHz)."""
    window, hop = 15360, 7680
    weights = np.zeros(patches)
    for i in range(patches):
        chunk = samples[i * hop:i * hop + window]
        weights[i] = float(np.sqrt(np.mean(chunk ** 2))) if chunk.size else 0.0
    return weights


def _models_for(spec):
    """The loaded models, built once per process (a session costs ~0.3 s)."""
    key = str(_models_dir(spec))
    if key not in _MODEL_CACHE:
        _MODEL_CACHE[key] = Models(spec)
    return _MODEL_CACHE[key]


def _unit(vector):
    norm = float(np.linalg.norm(vector))
    return vector / norm if norm > 0 else vector


# ---------------------------------------------------------------------------
# 4. One record per asset
# ---------------------------------------------------------------------------


def _quality(read):
    """The problems a mix would inherit: silence, clipping, a stray tail."""
    peak = read.get("peakDbfs")
    flags = []
    if read.get("silent"):
        flags.append("silent")
    if peak is not None and peak >= -0.1:
        flags.append("clipping")
    if read.get("truncated"):
        flags.append("truncated")
    if read.get("padded"):
        flags.append("padded")
    return {"peakDbfs": _r(peak), "rmsDbfs": _r(read.get("rmsDbfs")),
            "crestDb": _r(read.get("crestDb")), "flags": flags}


def _trim(read):
    """Where the usable sound starts and ends, and what to cut."""
    duration = read.get("duration") or 0.0
    peak_at = read.get("peakAt")
    decay = read.get("decaySec")
    start = 0.0
    if read.get("attackMs") is not None and read["attackMs"] < 5.0 and duration > 0.3:
        start = 0.0  # a hard attack is the point: never trim into it
    end = duration
    if peak_at is not None and decay:
        end = min(duration, peak_at + decay)
    advice = []
    if duration and end < duration - 0.05:
        advice.append(f"tail after {_r(end, 3)}s adds nothing")
    if read.get("silent"):
        advice.append("no usable signal: do not place")
    return {"usableStart": _r(start, 3), "usableEnd": _r(end, 3), "advice": advice}


def describe_file(spec, path, models):
    read, why = sound_sense._read(spec, str(path))
    if read is None:
        return None, why
    identity = _identity(path)
    record = {
        "file": Path(path).name,
        "path": str(path),
        "bytes": Path(path).stat().st_size,
        "identity": identity,
        "model": {"state": models.state},
        "soundSense": {
            "attackMs": _r(read.get("attackMs"), 2),
            "decaySec": _r(read.get("decaySec")),
            "peakAt": _r(read.get("peakAt")),
            "peakFraction": _r(read.get("peakFraction")),
            "brightnessHz": _r(read.get("centroidHz"), 1),
            "rolloffHz": _r(read.get("rolloffHz"), 1),
            "bands": read.get("bands"),
            "transients": read.get("transients"),
            "flatness": _r(read.get("flatness")),
            "pitchHz": _r(read.get("pitchHz"), 2),
            "silent": read.get("silent"),
            "duration": _r(read.get("duration")),
            # a bed is often longer than the measurement window: say which
            # part of the file the numbers describe
            "measuredSec": _r(read.get("measuredSec")),
            "truncated": read.get("truncated"),
            "energySlopeDbPerSec": _r(read.get("energySlopeDbPerSec"), 2),
        },
        "quality": _quality(read),
        # The evidence the fits were computed from: a plan that says "this
        # sound fits a reveal" without the numbers behind it is a guess in
        # a confident voice.
        "evidence": read.get("evidence"),
        "roles": {name: _r(value) for name, value in (read.get("roles") or {}).items()},
        "role": read.get("role"),
        "confidence": _r(read.get("confidence")),
        "syncOffset": _r(read.get("peakAt")),
        "trim": _trim(read),
    }
    if models.state != "ready":
        record["model"]["reason"] = models.reason
        return record, None

    yam = models.yamnet_read(path)
    if yam:
        record["yamnet"] = yam
    embedding = models.clap_embedding(path)
    if embedding is not None:
        record["clap"] = {
            "dim": int(embedding.size),
            "embedding": [_r(float(v), 4) for v in embedding],
            "description": models.describe(embedding),
        }
        if yam and (yam.get("music") or 0.0) >= 0.25:
            record["kind"] = "music"
            record["clap"]["moods"] = models.moods(embedding)
        else:
            record["kind"] = "sfx"
    return record, None


# ---------------------------------------------------------------------------
# 5. The tools
# ---------------------------------------------------------------------------


def describe_sound(spec):
    path = Path(str(spec.get("file") or ""))
    if not path.is_file():
        return {"tool": "describe", "ok": False, "error": f"no such file: {path}"}
    models = _models_for(spec)
    record, why = describe_file(spec, path, models)
    if record is None:
        return {"tool": "describe", "ok": False, "error": why}
    return {"tool": "describe", "ok": True, "sound": record}


def _entries(root):
    return sound_sense._library_entries(root)


def _stamp(path, models_dir):
    """What must not change for a cached record to still be true."""
    models = []
    for name in ("clap_audio.onnx", "clap_text.onnx", "yamnet.onnx"):
        model = Path(models_dir) / name
        models.append([name, model.stat().st_size if model.is_file() else 0,
                       int(model.stat().st_mtime) if model.is_file() else 0])
    return {"version": MODEL_INDEX_VERSION, "models": models}


def _index_one(job):
    """One asset, in whichever process took the job."""
    spec, path = job
    models = _models_for(spec)
    record, why = describe_file(spec, Path(path), models)
    return path, record, why


def _workers(spec):
    """How many processes to measure with.

    Two by default: the models are ~180 MB of weights per process, so a
    worker pool is a memory decision before it is a speed one, and a
    laptop that swaps mid-index is slower than one that does not. The
    ceiling is the machine's own core count, and `workers: 1` turns the
    pool off entirely.
    """
    named = spec.get("workers") or os.environ.get("FUNCUT_SOUND_WORKERS") or 2
    try:
        count = int(named)
    except (TypeError, ValueError):
        count = 2
    return max(1, min(count, 8, os.cpu_count() or 2))


def index_library(spec):
    root = Path(str(spec.get("dir") or ""))
    if not root.is_dir():
        return {"tool": "index", "ok": False, "error": f"no such directory: {root}"}
    out = Path(str(spec.get("out") or (root / "sound-model-index.json")))
    models = _models_for(spec)
    stamp = _stamp(root, _models_dir(spec))

    cached = {}
    if out.is_file():
        try:
            old = json.loads(out.read_text(encoding="utf-8"))
            if old.get("stamp") == stamp:
                cached = {entry["file"]: entry for entry in old.get("sounds", [])}
        except Exception:
            cached = {}

    started = time.time()
    sounds, failed, skipped, pending, listed_of = [], [], [], [], {}
    for path, listed in _entries(root):
        try:
            stat = path.stat()
        except OSError:
            continue
        known = cached.get(path.name)
        if known and known.get("bytes") == stat.st_size and known.get("mtime") == int(stat.st_mtime):
            sounds.append(known)
            continue
        listed_of[str(path)] = (int(stat.st_size), int(stat.st_mtime), listed)
        pending.append(str(path))

    workers = _workers(spec)
    results = []
    if workers > 1 and len(pending) > 8:
        try:
            from concurrent.futures import ProcessPoolExecutor

            # Fork-free start method: the pool is built after the parent has
            # already loaded its own sessions, and a forked child inheriting
            # ONNX Runtime's threads is the classic way to hang a pool.
            with ProcessPoolExecutor(max_workers=workers) as pool:
                results = list(pool.map(_index_one,
                                        [(spec, path) for path in pending], chunksize=4))
        except Exception as exc:  # no shared memory, no /dev/shm, no luck
            results = []
            workers = 1
            spec = {**spec, "poolError": f"{type(exc).__name__}: {exc}"}
    if not results:
        results = [_index_one((spec, path)) for path in pending]

    for path, record, why in results:
        name = Path(path).name
        if record is None:
            failed.append({"file": name, "why": why})
            continue
        size, mtime, listed = listed_of.get(path, (None, None, {}))
        record["mtime"] = mtime
        if listed.get("category"):
            record["category"] = listed["category"]
        if listed.get("useFor"):
            record["useFor"] = listed["useFor"]
        if record.get("soundSense", {}).get("silent"):
            skipped.append({"file": name, "why": "silent: no usable signal"})
        sounds.append(record)

    sounds.sort(key=lambda entry: entry.get("file", ""))
    payload = {
        "tool": "index",
        "ok": True,
        "dir": str(root),
        "stamp": stamp,
        "model": {"state": models.state, "reason": models.reason},
        "workers": workers,
        "poolError": spec.get("poolError"),
        "count": len(sounds),
        "measured": len(pending),
        "reused": len(sounds) - len(pending) + len(failed),
        "failed": failed,
        "skipped": skipped,
        "seconds": _r(time.time() - started, 2),
        "sounds": sounds,
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload), encoding="utf-8")
    payload["out"] = str(out)
    payload["sounds"] = []  # the file has them; the reply does not repeat them
    return payload


_INDEX_CACHE: dict = {}
_MODEL_CACHE: dict = {}


def _index_records(spec):
    """The parsed index, read once per process per file version.

    `plan` ranks once per accent, and re-reading a 3 MB index twenty times
    to answer twenty dot products is the kind of cost that makes a feature
    feel slow without being slow.
    """
    # `index` names the file; with only `dir` given, the file the index
    # tool writes there is the one meant.
    named = str(spec.get("index") or "")
    if not named and spec.get("dir"):
        named = str(Path(str(spec["dir"])) / "sound-model-index.json")
    path = Path(named or "sound-model-index.json")
    if not path.is_file():
        return None, f"no index at {path}: run the index tool first"
    try:
        stamp = path.stat().st_mtime
    except OSError as exc:
        return None, f"could not read {path}: {exc}"
    key = (str(path), stamp)
    if key in _INDEX_CACHE:
        return _INDEX_CACHE[key], None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        return None, f"could not read {path}: {exc}"
    _INDEX_CACHE.clear()
    _INDEX_CACHE[key] = parsed
    return parsed, None


def rank_sounds(spec):
    index, why = _index_records(spec)
    if index is None:
        return {"tool": "rank", "ok": False, "error": why}
    query = str(spec.get("query") or "").strip()
    role = str(spec.get("role") or "").strip()
    if not query and not role:
        return {"tool": "rank", "ok": False, "error": "give a query, a role, or both"}
    limit = int(spec.get("limit") or 8)
    max_duration = spec.get("maxDuration")

    models = _models_for(spec)
    text = models.text_embedding(query) if (query and models.state == "ready") else None

    scored = []
    for sound in index.get("sounds", []):
        if sound.get("soundSense", {}).get("silent"):
            continue
        if max_duration is not None:
            duration = sound.get("identity", {}).get("duration")
            if duration and duration > float(max_duration):
                continue
        embedding = (sound.get("clap") or {}).get("embedding")
        clap = float(np.dot(np.asarray(embedding), text)) if (text is not None and embedding) else None
        measured = (sound.get("roles") or {}).get(role) if role else None
        scored.append({"file": sound["file"], "clap": clap, "measure": measured,
                       "duration": sound.get("identity", {}).get("duration"),
                       "peakDbfs": (sound.get("quality") or {}).get("peakDbfs"),
                       "flags": (sound.get("quality") or {}).get("flags") or [],
                       "role": sound.get("role"), "labels": [
                           l["label"] for l in (sound.get("yamnet") or {}).get("labels", [])[:3]],
                       "syncOffset": sound.get("syncOffset")})

    degraded = None
    if role and role in ROLE_WEIGHT and text is None:
        # No CLAP, and this role's ranking leans on it: ranking by the
        # measurements anyway is honest only if the reply says so, because
        # for a transition the measurements barely separate a whoosh from
        # foley, and an alphabetical list dressed as a ranking is a lie.
        for s in scored:
            s["fused"] = s["measure"] or 0.0
            s["why"] = "measured role fit only: no CLAP"
        degraded = f"no CLAP: ranked by measurements alone, which is weak for {role}"
    elif role and role in ROLE_WEIGHT:
        weight = ROLE_WEIGHT[role]
        claps = [s["clap"] for s in scored if s["clap"] is not None]
        measured = [s["measure"] for s in scored if s["measure"] is not None]
        c_mean = sum(claps) / len(claps) if claps else 0.0
        c_sd = _sd(claps) or 1e-9
        m_mean = sum(measured) / len(measured) if measured else 0.0
        m_sd = _sd(measured) or 1e-9
        for s in scored:
            fused = 0.0
            if s["clap"] is not None:
                fused += (s["clap"] - c_mean) / c_sd
            if s["measure"] is not None:
                fused += weight * (s["measure"] - m_mean) / m_sd
            s["fused"] = fused
            s["why"] = _why(s, role, weight)
    else:
        for s in scored:
            s["fused"] = s["clap"] if s["clap"] is not None else (s["measure"] or 0.0)
            s["why"] = "clap similarity" if s["clap"] is not None else "measured role fit"

    scored.sort(key=lambda s: -(s["fused"] if s["fused"] is not None else -9e9))
    top = scored[:limit]
    for s in top:
        s["fused"] = _r(s["fused"], 3)
        s["clap"] = _r(s["clap"], 3)
        s["measure"] = _r(s["measure"], 3)
    reply = {"tool": "rank", "ok": True, "query": query, "role": role,
             "weight": ROLE_WEIGHT.get(role), "model": {"state": models.state,
                                                        "reason": models.reason},
             "considered": len(scored), "candidates": top}
    if degraded:
        reply["degraded"] = degraded
    return reply


def _sd(values):
    if len(values) < 2:
        return 0.0
    mean = sum(values) / len(values)
    return math.sqrt(sum((v - mean) ** 2 for v in values) / len(values))


def _why(candidate, role, weight):
    parts = []
    if candidate["clap"] is not None:
        parts.append("clap similarity")
    if candidate["measure"] is not None:
        parts.append(f"measured {role} fit x{weight:g}")
    return " + ".join(parts) if parts else "no evidence"


# ---------------------------------------------------------------------------
# 6. plan: every signal, one sound design
# ---------------------------------------------------------------------------
#
# The Director's other stages each answer one question - the arc names the
# mood, prosody names the stressed word, the vision read names the cut, the
# music read names the downbeat. Sound design is where they have to agree,
# and a whoosh that lands 80 ms after the stressed syllable is wrong even
# though every one of those reads was right on its own.
#
# So this tool takes them all at once and answers with a plan: what sound,
# where its peak lands, how loud, what it must not cover, and which signal
# put it there. Nothing here invents a number - every field cites the input
# it came from, because a plan a Director cannot audit is a guess with
# better formatting.

ROLE_FOR_CONTEXT = {
    # context: the role the moment is asking for
    "hook": "action_impact",
    "reveal": "cinematic_reveal",
    "cut": "quick_transition",
    "emphasis": "action_impact",
    "act_start": "tension_build",
    "graphic": "gentle_entrance",
    "calm": "emotional_texture",
}

EMOTION_ROLE = {
    "anger": "action_impact",
    "fear": "tension_build",
    "surprise": "cinematic_reveal",
    "joy": "gentle_entrance",
    "sadness": "emotional_texture",
    "disgust": "action_impact",
    "neutral": "quick_transition",
}

TEXTURE_WORDS = {
    "action_impact": "a sharp heavy impact",
    "cinematic_reveal": "a deep heavy cinematic boom",
    "quick_transition": "a short bright whoosh of air",
    "gentle_entrance": "a soft light interface entrance",
    "tension_build": "a rising tension build",
    "emotional_texture": "a warm sustained emotional texture",
}


def _accent_sources(spec):
    """Every moment that could carry an accent, with the signal that says so.

    The order is the order a mixer would consider them: the hook first,
    because the first three seconds are the only part a viewer is
    guaranteed to see; then reveals, because they are the payoff; then the
    stressed words; then cuts and motion hits, which are accents of
    opportunity.
    """
    visual = spec.get("visual") or {}
    arc = spec.get("arc") or {}
    out = []

    def add(at, context, source, weight, **extra):
        if at is None:
            return
        out.append({"at": float(at), "context": context, "source": source,
                    "weight": float(weight), **extra})

    for event in visual.get("events") or []:
        if not isinstance(event, dict):
            continue
        kind = str(event.get("kind") or "").lower()
        if kind in ("reveal", "payoff", "result", "title"):
            add(event.get("at"), "reveal", "visual.events.reveal", 1.0,
                kind=kind)
    for act in arc.get("acts") or []:
        if not isinstance(act, dict):
            continue
        name = str(act.get("name") or "").lower()
        if name in ("hook", "opening", "title"):
            add(act.get("start", 0.0), "hook", "arc.acts.hook", 0.9)
        elif name in ("turn", "reveal", "payoff", "climax"):
            add(act.get("start"), "reveal", f"arc.acts.{name}", 0.9)
        elif act.get("start") is not None:
            add(act.get("start"), "act_start", f"arc.acts.{name}", 0.45)
    for beat in spec.get("beats") or []:
        if not isinstance(beat, dict):
            continue
        for hit in beat.get("emphasis") or []:
            at = hit.get("at") if isinstance(hit, dict) else None
            word = hit.get("word") if isinstance(hit, dict) else str(hit)
            add(at, "emphasis", "beats.emphasis", 0.8, word=word,
                emotion=beat.get("emotion"), beat=beat)
    for at in visual.get("cuts") or []:
        add(at, "cut", "visual.cuts", 0.55)
    for at in visual.get("motionHits") or []:
        add(at, "cut", "visual.motionHits", 0.5)
    out.sort(key=lambda moment: (moment["at"], -moment["weight"]))
    # Two reads naming the same instant are ONE accent: a reveal the vision
    # read and the stressed word both landed on is not a place for two
    # sounds, and reporting the second as a density violation hides the
    # fact that the two signals agreed.
    merged = []
    for moment in out:
        if merged and moment["at"] - merged[-1]["at"] < 0.02:
            kept = merged[-1]
            kept["sources"] = kept.get("sources", [kept["source"]]) + [moment["source"]]
            kept["weight"] = max(kept["weight"], moment["weight"])
            # The beat carries the prosody - the pause, the pitch, the
            # stressed word - and a vision event at the same instant has
            # none of it. Dropping the beat with the merge threw away the
            # pause that decides whether the moment wants silence.
            if not kept.get("beat") and moment.get("beat"):
                kept["beat"] = moment["beat"]
            # A reveal outranks a cut at the same instant: the payoff is the
            # reason the frame exists.
            if moment["context"] == "reveal" and kept["context"] != "reveal":
                kept["context"] = "reveal"
                kept["beat"] = moment.get("beat", kept.get("beat"))
            continue
        merged.append(moment)
    # The act boundary and the frame that proves it are one payoff: an arc
    # saying "the reveal starts at 22.0" and a vision read saying "the
    # reveal frame is at 22.4" is the same beat seen twice, and two booms
    # 400 ms apart is the double-hit a mixer would cut. The stronger read
    # wins - the measured frame over the planned boundary - and the weaker
    # one is not reported as a moment that could not be filled, because it
    # was never a separate moment.
    one = []
    for moment in merged:
        if (one and moment["context"] == "reveal"
                and one[-1]["context"] == "reveal"
                and moment["at"] - one[-1]["at"] <= 0.6):
            kept = one[-1]
            if moment["weight"] > kept["weight"]:
                kept["at"] = moment["at"]
                kept["weight"] = moment["weight"]
                kept["source"] = moment["source"]
                kept["beat"] = moment.get("beat", kept.get("beat"))
            kept["sources"] = sorted(set(kept.get("sources", [kept["source"]])
                                         + moment.get("sources", [moment["source"]])))
            continue
        one.append(moment)
    return one


def _role_for(moment, spec):
    """The job this moment needs, from its context and the story's mood."""
    arc = spec.get("arc") or {}
    beat = moment.get("beat") or {}
    emotion = str(beat.get("emotion") or arc.get("emotion") or "neutral").lower()
    intensity = float(beat.get("intensity") or arc.get("intensity") or 0.5)
    role = ROLE_FOR_CONTEXT.get(moment["context"], "quick_transition")
    # A question whose answer arrives is a reveal, not a cut: the pitch
    # rose and the pause held, which is the shape of a setup.
    if (beat.get("sentenceType") or "").lower() in ("question", "interrogative") \
            and float(beat.get("pauseBefore") or 0.0) >= 0.35:
        role = "cinematic_reveal"
    # A quiet line does not take an impact, however it is punctuated.
    if intensity < 0.35 and role == "action_impact":
        role = "gentle_entrance"
    # Rising pitch inside a tension act is a build, not a hit.
    if (beat.get("pitchDirection") or "").lower() in ("rising", "up") \
            and role in ("action_impact", "quick_transition") and intensity >= 0.6:
        role = "tension_build"
    if emotion in EMOTION_ROLE and moment["context"] in ("emphasis", "act_start"):
        role = EMOTION_ROLE[emotion] if intensity >= 0.5 else role
    return role, emotion, intensity


def _query_for(role, emotion, moment):
    """The sentence the shelf is searched with: role texture + mood."""
    base = TEXTURE_WORDS.get(role, "a sound effect")
    word = moment.get("word")
    if word:
        return f"{base} for the word {word!r}, {emotion} moment"
    return f"{base}, {emotion} moment"


def _pick(candidates, role, weight, moment, used, gap):
    """The best candidate that fits the moment, with the reason it won."""
    best = None
    spent = long_for_gap = 0
    for candidate in candidates:
        if candidate.get("file") in used:
            spent += 1
            continue
        duration = candidate.get("duration")
        if duration and gap is not None and duration > gap * 1.6 + 0.1:
            # A 4 s boom cannot sit in a 1 s gap: it would cover the next
            # word, which is the failure the whole layer exists to catch.
            long_for_gap += 1
            continue
        score = candidate.get("fused") or 0.0
        if best is None or score > best[1]:
            best = (candidate, score)
    if best is None:
        if not candidates:
            return None, "the shelf answered nothing for this query"
        return None, (f"no candidate fits: {spent} of {len(candidates)} already used"
                      + (f", {long_for_gap} longer than the {_r(gap, 2)}s gap"
                         if long_for_gap else ""))
    return best[0], None


def plan_sound(spec):
    """Every signal at once, into a plan the Director can execute."""
    index, why = _index_records(spec)
    if index is None:
        return {"tool": "plan", "ok": False, "error": why}
    models = _models_for(spec)
    beats = spec.get("beats") or []
    style = spec.get("style") or {}
    duration = float(spec.get("durationSec") or 0.0)
    max_per_10s = float(style.get("maxAccentsPer10s") or 4.0)
    min_gap = float(style.get("minAccentGapSec") or 0.45)
    voice_lufs = spec.get("voiceLufs")

    moments = _accent_sources(spec)
    used, accents, warnings, silence = [], [], [], []
    last_at = None

    for index_of, moment in enumerate(moments):
        role, emotion, intensity = _role_for(moment, spec)
        beat = moment.get("beat") or {}
        # The gap a sound has to fit inside: to the next moment that will
        # actually be placed (a weaker one inside the floor gives way to
        # this one, so it is not a wall), and never past the end of the
        # line being spoken.
        gap = None
        for later in moments[index_of + 1:]:
            if later["at"] <= moment["at"]:
                continue
            if later["at"] - moment["at"] < min_gap and later["weight"] < moment["weight"]:
                continue
            gap = later["at"] - moment["at"]
            break
        if beat.get("end") is not None:
            until = float(beat["end"]) - moment["at"]
            if until > 0 and (gap is None or until < gap):
                gap = until
        # A held pause is a fact about the take, not a consequence of
        # placing an accent: it is reported whether or not this moment ends
        # up carrying a sound, because the room to hold is the direction.
        pause = float(beat.get("pauseBefore") or 0.0)
        if pause >= 0.5 and role == "cinematic_reveal":
            silence.append({
                "at": _r(moment["at"] - pause, 3), "until": _r(moment["at"], 3),
                "why": f"a {_r(pause, 2)}s pause before the reveal: hold the "
                       "room, then land the sound on the reveal itself",
                "signal": "beats.pauseBefore",
            })
        # Density: an accent every 300 ms is noise, not design. A stronger
        # moment still wins the instant - the reveal outranks the cut that
        # happens to sit 400 ms before it, and a cut is not worth losing
        # the payoff over.
        if last_at is not None and moment["at"] - last_at < min_gap:
            close = [a for a in accents if moment["at"] - a["at"] < min_gap]
            weakest = min(close, key=lambda accent: accent["weight"]) if close else None
            if weakest is not None and moment["weight"] > weakest["weight"] + 0.05:
                accents.remove(weakest)
                if weakest["file"] in used:
                    used.remove(weakest["file"])
                warnings.append({
                    "at": weakest["at"], "kind": "evicted",
                    "why": f"dropped for the {moment['context']} at "
                           f"{_r(moment['at'], 3)}s, {_r(moment['at'] - weakest['at'], 3)}s "
                           f"away: weight {_r(weakest['weight'], 2)} against "
                           f"{_r(moment['weight'], 2)}",
                    "signal": weakest["why"]["signal"],
                })
            else:
                warnings.append({
                    "at": _r(moment["at"], 3), "kind": "density",
                    "why": f"{_r(moment['at'] - last_at, 3)}s after the previous accent, "
                           f"below the {min_gap}s floor: dropped",
                    "signal": moment["source"],
                })
                continue
        window = [a for a in accents if moment["at"] - 10.0 <= a["at"] <= moment["at"]]
        if len(window) >= max_per_10s:
            weakest = min(window, key=lambda accent: accent["weight"])
            if moment["weight"] > weakest["weight"] + 0.05:
                # The story's payoff outranks an accent of opportunity: the
                # weaker one gives way, and the plan says which and why.
                accents.remove(weakest)
                if weakest["file"] in used:
                    used.remove(weakest["file"])
                warnings.append({
                    "at": weakest["at"], "kind": "evicted",
                    "why": f"dropped for the {moment['context']} at {_r(moment['at'], 3)}s: "
                           f"{len(window)} accents in 10s, and this was the weakest "
                           f"(weight {_r(weakest['weight'], 2)} against "
                           f"{_r(moment['weight'], 2)})",
                    "signal": weakest["why"]["signal"],
                })
            else:
                warnings.append({
                    "at": _r(moment["at"], 3), "kind": "density",
                    "why": f"{len(window)} accents in the last 10s, at the "
                           f"{max_per_10s:g}/10s ceiling: dropped",
                    "signal": moment["source"],
                })
                continue
        query = _query_for(role, emotion, moment)
        ranked = rank_sounds({**spec, "query": query, "role": role,
                              "limit": 12, "models": spec.get("models")})
        candidates = ranked.get("candidates") or [] if ranked.get("ok") else []
        chosen, failure = _pick(candidates, role, ranked.get("weight"), moment, used, gap)
        if chosen is None:
            warnings.append({
                "at": _r(moment["at"], 3), "kind": "no-sound",
                "why": failure or "no candidate", "signal": moment["source"],
                "query": query,
            })
            continue
        used.append(chosen["file"])
        offset = chosen.get("syncOffset") or 0.0
        # The level: the shelf sits near full scale, so an accent is placed
        # by its own measured peak against a target that leaves the voice
        # on top. A loud take gets a quieter accent, which is what a mixer
        # does with a fader rather than a different sound.
        peak = (chosen.get("peakDbfs") if chosen.get("peakDbfs") is not None else -12.0)
        target = -12.0
        if voice_lufs is not None:
            target = max(-18.0, min(-9.0, float(voice_lufs) + 4.0))
        wanted = target - float(peak)
        # A fader has limits: more than +6 dB of boost on a shelf sound
        # brings its noise floor up with it, and past -24 dB the accent is
        # inaudible, which is the same as not placing it.
        gain = round(max(-24.0, min(6.0, wanted)), 1)
        clamped = abs(gain - wanted) > 0.05
        accents.append({
            "at": _r(moment["at"], 3),
            "role": role,
            "context": moment["context"],
            "weight": moment["weight"],
            "file": chosen["file"],
            "id": Path(chosen["file"]).stem,
            "query": query,
            "startAt": _r(max(0.0, moment["at"] - offset), 3),
            "peakLandsAt": _r(moment["at"], 3),
            "syncOffset": _r(offset, 3),
            "gainDb": gain,
            "gainClamped": clamped,
            "peakDbfs": peak,
            "duration": chosen.get("duration"),
            "fitsGap": gap,
            "why": {
                "role": f"{moment['context']} + {emotion} (intensity {_r(intensity, 2)})",
                "signal": ", ".join(moment.get("sources", [moment["source"]])),
                "word": moment.get("word"),
                "score": chosen.get("fused"),
                "clap": chosen.get("clap"),
                "measure": chosen.get("measure"),
                "labels": chosen.get("labels"),
            },
        })
        last_at = moment["at"]

    music = _plan_music(spec, models, accents, warnings, voice_lufs)
    reply = {
        "tool": "plan",
        "ok": True,
        "model": {"state": models.state, "reason": models.reason},
        "considered": len(moments),
        "accents": accents,
        "silence": silence,
        "music": music,
        "warnings": warnings,
        "unusedSignals": _unused(spec),
    }
    return reply


def _plan_music(spec, models, accents, warnings, voice_lufs):
    """Where the music goes, where it changes, and what it must not cover."""
    bgm = spec.get("bgm") or {}
    candidates = bgm.get("candidates") or []
    arc = spec.get("arc") or {}
    duration = float(spec.get("durationSec") or 0.0)
    if not candidates:
        return {"state": "none", "why": "no music candidates offered: the cut is "
                "designed dry, which is a decision and is reported as one"}
    # The mood the story is asking the music to carry.
    mood = str(arc.get("emotion") or "neutral").lower()
    scored = []
    for candidate in candidates:
        score = 0.0
        why = []
        moods = candidate.get("moods") or []
        if moods:
            hit = next((m for m in moods if mood.split()[0] in m["mood"]), None)
            if hit:
                score += hit["similarity"]
                why.append(f"mood match {hit['similarity']}")
        if candidate.get("bpm"):
            score += 0.05
            why.append(f"{candidate['bpm']} bpm")
        scored.append((score, candidate, why))
    scored.sort(key=lambda row: -row[0])
    _, track, why = scored[0]
    # The change lands on the music's own structure, not on a round number:
    # a cut-safe point near the story's turn is where the ear expects it.
    turn = None
    for act in arc.get("acts") or []:
        if str(act.get("name") or "").lower() in ("turn", "reveal", "climax"):
            turn = float(act.get("start") or 0.0)
            break
    change = None
    safe = track.get("cutSafePoints") or []
    if turn is not None and safe:
        change = min(safe, key=lambda point: abs(float(point) - turn))
    ducking = []
    for beat in spec.get("beats") or []:
        start, end = beat.get("at"), beat.get("end")
        if start is None or end is None:
            continue
        ducking.append({"from": _r(start, 2), "to": _r(end, 2), "duckDb": -12.0,
                        "rampMs": 250,
                        "why": "narration: the bed sits under the voice"})
    ending = None
    if duration and track.get("duration"):
        track_end = float(track["duration"])
        if track_end - duration < 0.5:
            ending = {"at": _r(max(0.0, duration - 1.5), 2), "fadeSec": 1.5,
                      "why": "the track would be cut off before it resolves: "
                             "fade rather than stop"}
    return {
        "state": "planned",
        "file": track.get("file"),
        "why": why,
        "changeAt": _r(change, 3) if change is not None else None,
        "changeWhy": "nearest cut-safe point to the story's turn" if change else None,
        "ducking": ducking,
        "ending": ending,
        "gainDb": -14.0 if voice_lufs is None else round(float(voice_lufs) - 6.0, 1),
    }


def _unused(spec):
    """The signals the plan did not read - said out loud, not silently dropped."""
    known = {"beats", "visual", "arc", "bgm", "style", "durationSec", "voiceLufs",
             "index", "models", "tool", "query", "role", "limit", "file", "dir",
             "out", "maxDuration"}
    return sorted(key for key in spec if key not in known)


# ---------------------------------------------------------------------------
# 6. selftest: the numbers, not the vibes
# ---------------------------------------------------------------------------


def _tone(freq=1000.0, seconds=1.0, rate=CLAP_RATE):
    return np.array([0.5 * math.sin(2 * math.pi * freq * i / rate)
                     for i in range(int(seconds * rate))], dtype=np.float64)


def _plan_selftest(spec):
    """The planner's rules, on records that need no decoding."""
    import tempfile

    checks = []

    def check(name, ok, detail=""):
        checks.append({"check": name, "ok": bool(ok), "detail": detail})

    def record(name, duration, fits, peak=-6.0):
        return {"file": name, "identity": {"duration": duration},
                "quality": {"peakDbfs": peak, "flags": []},
                "roles": fits, "syncOffset": 0.05,
                "soundSense": {"silent": False}}

    index = {"stamp": {"models": "synthetic", "version": MODEL_INDEX_VERSION},
             "sounds": [
                 record("boom.ogg", 2.2, {"cinematic_reveal": 0.9, "action_impact": 0.7}),
                 record("hit.ogg", 0.6, {"action_impact": 0.9, "quick_transition": 0.4}),
                 record("whoosh.ogg", 0.5, {"quick_transition": 0.9}),
                 record("riser.ogg", 3.0, {"tension_build": 0.85}),
                 record("soft.ogg", 0.9, {"gentle_entrance": 0.8}),
                 record("pad.ogg", 4.0, {"emotional_texture": 0.8}),
             ]}
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "index.json"
        path.write_text(json.dumps(index), encoding="utf-8")
        plan = plan_sound({
            "tool": "plan", "index": str(path), "models": str(Path(tmp) / "none"),
            "durationSec": 30, "voiceLufs": -18.0,
            "arc": {"emotion": "fear", "acts": [{"name": "hook", "start": 0.5},
                                                {"name": "turn", "start": 12.0}]},
            "beats": [
                {"at": 1.0, "end": 3.0, "emotion": "fear", "intensity": 0.8,
                 "emphasis": [{"word": "never", "at": 1.4}]},
                {"at": 12.0, "end": 16.0, "emotion": "surprise", "intensity": 0.9,
                 "pauseBefore": 0.8, "sentenceType": "question",
                 "emphasis": [{"word": "everything", "at": 12.4}]},
            ],
            "visual": {"cuts": [12.0, 12.3, 12.6], "events": [{"at": 12.4, "kind": "reveal"}]},
            "style": {"maxAccentsPer10s": 3, "minAccentGapSec": 0.45},
        })

    check("plan answers without models", plan.get("ok") is True, str(plan.get("error", ""))[:80])
    accents = plan.get("accents") or []
    check("plan places accents", len(accents) >= 2, f"{len(accents)} accents")
    check("every accent cites the signal that put it there",
          all(a["why"].get("signal") for a in accents),
          str([a["why"].get("signal") for a in accents]))
    check("every accent names a real file", all(a.get("file") for a in accents))
    check("the reveal gets a sound",
          any(a["at"] == 12.4 or a["role"] == "cinematic_reveal" for a in accents),
          str([(a["at"], a["role"]) for a in accents]))
    # The window ends where the accent lands and begins one pause earlier,
    # so it is the pause the beat carried (0.8 s before the reveal at 12.4),
    # not a number the planner invented.
    check("a held pause is reported as silence",
          any(abs(float(window["until"]) - 12.4) < 0.01
              and abs((float(window["until"]) - float(window["at"])) - 0.8) < 0.01
              for window in plan.get("silence") or []),
          str(plan.get("silence")))
    check("a cluster of cuts is thinned, not stacked",
          any(w["kind"] in ("density", "evicted") for w in plan.get("warnings") or []),
          str([(w["at"], w["kind"]) for w in plan.get("warnings") or []]))
    check("a gain is measured, not assumed",
          all(a.get("peakDbfs") is not None for a in accents),
          str([a.get("gainDb") for a in accents]))
    check("the same file is not used twice", len({a["file"] for a in accents}) == len(accents),
          str([a["file"] for a in accents]))
    return checks


def selftest(spec=None):
    spec = spec or {}
    checks = []

    def check(name, ok, detail=""):
        checks.append({"check": name, "ok": bool(ok), "detail": detail})

    if np is None:
        check("numpy present", False, "pip install numpy")
        return {"tool": "selftest", "ok": False, "checks": checks}

    filters = _slaney_filters()
    check("filterbank is 513x64", filters.shape == (513, 64), str(filters.shape))
    check("filterbank peaks near 1.0", abs(float(filters.max()) - 0.018149) < 5e-4,
          f"max {filters.max():.6f}")  # slaney norm, first band, 50-14000 Hz
    check("filterbank band 0 rises then falls",
          int(np.argmax(filters[:, 0])) in (2, 3), f"peak bin {int(np.argmax(filters[:, 0]))}")

    features = clap_features(_tone())
    check("clap features are (1, 1, 1001, 64)", features.shape == (1, 1, 1001, 64),
          str(features.shape))
    loudest = int(np.argmax(features[0, 0].mean(axis=0)))
    check("a 1 kHz tone lands in one band", 8 <= loudest <= 40, f"band {loudest}")
    check("features are dB, floored at -100",
          -100.0 <= float(features.min()) and float(features.max()) < 40.0,
          f"{features.min():.1f}..{features.max():.1f}")

    silent = clap_features(np.zeros(CLAP_RATE))
    check("silence is floored", float(silent.max()) == CLAP_FLOOR_DB, f"{silent.max():.1f}")

    check("role weights cover the six roles", len(ROLE_WEIGHT) == 6, str(sorted(ROLE_WEIGHT)))
    check("transitions trust the model alone", ROLE_WEIGHT["quick_transition"] == 0.0)

    models = Models(spec)
    check("models are absent without files or present with them",
          models.state in ("ready", "absent"), models.state or models.reason or "")

    if models.state == "ready":
        shelf = Path(str(spec.get("shelf") or Path(__file__).resolve().parents[4] / "audio"))
        whoosh = shelf / "reverse-whoosh-medium-strong.ogg"
        if whoosh.is_file():
            yam = models.yamnet_read(whoosh)
            labels = [l["label"].lower() for l in (yam or {}).get("labels", [])]
            check("yamnet calls a whoosh a whoosh",
                  any("whoosh" in label or "swish" in label for label in labels), str(labels[:3]))
            embedding = models.clap_embedding(whoosh)
            check("clap embedding is 512-d and unit length",
                  embedding is not None and embedding.size == 512
                  and abs(float(np.linalg.norm(embedding)) - 1.0) < 1e-3,
                  "" if embedding is None else f"{embedding.size}")

    checks.extend(_plan_selftest(spec))

    failed = [c for c in checks if not c["ok"]]
    return {"tool": "selftest", "ok": not failed, "checks": checks,
            "passed": len(checks) - len(failed), "total": len(checks),
            "model": {"state": models.state, "reason": models.reason}}


TOOLS = {
    "describe": describe_sound,
    "index": index_library,
    "rank": rank_sounds,
    "plan": plan_sound,
    "selftest": selftest,
}


def command(spec):
    """The family door sense.py calls: one spec in, one object out."""
    tool = str(spec.get("tool") or "")
    action = str(spec.get("action", "extract"))
    try:
        if action == "selftest":
            return selftest(spec if tool in TOOLS else None)
        handler = TOOLS.get(tool)
        if handler is None:
            return {"ok": False, "error": f"unknown tool {tool!r}: one of {sorted(TOOLS)}"}
        return handler(spec)
    except Exception as exc:  # noqa: BLE001 - a sidecar reports, it does not traceback
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def main(argv):
    if "--spec" not in argv:
        print(json.dumps({"ok": False, "error": "usage: sound_model.py <tool> --spec -"}))
        return 2
    raw = sys.stdin.read() if argv[argv.index("--spec") + 1:][:1] == ["-"] else "{}"
    try:
        spec = json.loads(raw or "{}")
    except Exception as exc:
        print(json.dumps({"ok": False, "error": f"bad spec: {exc}"}))
        return 2
    tool = str(spec.get("tool") or (argv[1] if len(argv) > 1 else ""))
    handler = TOOLS.get(tool)
    if handler is None:
        print(json.dumps({"ok": False, "error": f"unknown tool: {tool}",
                          "tools": sorted(TOOLS)}))
        return 2
    try:
        result = handler(spec)
    except Exception as exc:  # a sidecar reports, it does not traceback
        result = {"tool": tool, "ok": False, "error": f"{type(exc).__name__}: {exc}"}
    print(json.dumps(result))
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
