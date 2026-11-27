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

THE FOUR TOOLS

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
  generate    words into a short sound or a music bed, with one of the
              three downloaded generators (TinyAudio-MF, Stable Audio
              Open Small, TinyMusician). The reply always answers: a
              model that is not installed, a licence a monetized cut may
              not spend (TinyMusician ships no licence file), or a
              generation that comes back near-silent all reply without a
              `path`, so the Director's stages shop the shelf instead of
              shipping no bed and no accents (see docs/SOUND-GENERATION.md).

MODELS ARE OPTIONAL, AND SAY SO

numpy and onnxruntime are used when they are installed and never required,
the same rule `voice_quality.py` follows. With no models present every tool
still answers, with `"model": {"state": "absent", ...}` and measurement-only
records - a Director without the models is a Director that is less certain,
not one that is broken.
"""

from __future__ import annotations

import array
import gc
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
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
        # The caller's own words for the mood they asked the bed to
        # carry, scored against the file the CLAP pair actually heard:
        # the cosine between the bed's audio embedding and the words'
        # text one. A bed that measures as another mood's music is the
        # swap the caller has candidates for.
        mood = str(spec.get("mood") or "").strip()
        if mood:
            record["clap"]["mood"] = mood
            record["clap"]["moodSimilarity"] = _r(
                float(embedding @ models.text_embedding(mood)), 3)
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

    # Measuring, ranking and planning are stdlib-only by contract; only the
    # ONNX paths need numpy. Failing the whole selftest on a missing optional
    # dependency reported a working planner as broken, which is the opposite
    # of what a selftest is for.
    if np is None:
        check("model paths", True,
              "skipped: numpy not installed (pip install numpy onnxruntime)")
    else:
        filters = _slaney_filters()
        check("filterbank is 513x64", filters.shape == (513, 64), str(filters.shape))
        check("filterbank peaks near 1.0", abs(float(filters.max()) - 0.018149) < 5e-4,
              f"max {filters.max():.6f}")  # slaney norm, first band, 50-14000 Hz
        check("filterbank band 0 rises then falls",
              int(np.argmax(filters[:, 0])) in (2, 3),
              f"peak bin {int(np.argmax(filters[:, 0]))}")

        features = clap_features(_tone())
        check("clap features are (1, 1, 1001, 64)", features.shape == (1, 1, 1001, 64),
              str(features.shape))
        loudest = int(np.argmax(features[0, 0].mean(axis=0)))
        check("a 1 kHz tone lands in one band", 8 <= loudest <= 40, f"band {loudest}")
        check("features are dB, floored at -100",
              -100.0 <= float(features.min()) and float(features.max()) < 40.0,
              f"{features.min():.1f}..{features.max():.1f}")

        silent = clap_features(np.zeros(CLAP_RATE))
        check("silence is floored", float(silent.max()) == CLAP_FLOOR_DB,
              f"{silent.max():.1f}")

    check("role weights cover the six roles", len(ROLE_WEIGHT) == 6, str(sorted(ROLE_WEIGHT)))
    check("transitions trust the model alone", ROLE_WEIGHT["quick_transition"] == 0.0)

    # The generator's own contract: an absent model is an answer the
    # Director's stages can read from the run log (ok: true, no `path`),
    # never an error that ends the stage warned with no bed and no
    # accents.
    bogus, bogus_why = _generator({"kind": "not-a-kind"})
    check("an unknown kind is an error, not an answer",
          bogus is None and bogus_why.get("ok") is False, str(bogus_why))
    resolved, absent_why = _generator({"kind": "sfx-small", "models": "/nowhere"})
    if absent_why is not None:
        check("an absent generator answers ok with no path",
              absent_why.get("ok") is True and absent_why.get("model") == "absent"
              and "path" not in absent_why and absent_why.get("generated") is False,
              str(absent_why.get("reason") or absent_why))
        reply = generate_sound({"kind": "sfx-small", "prompt": "a door slams",
                                "models": "/nowhere"})
        check("generate without a model still answers",
              reply.get("ok") is True and reply.get("model") == "absent"
              and "path" not in reply, str(reply.get("reason") or reply))
    else:
        check("generator present: absent-path skipped", True,
              "the model files are installed")
    check("generate without a prompt is an error",
          generate_sound({"kind": "sfx-small", "prompt": "  "}).get("ok") is False
          or _generator({"kind": "sfx-small"})[1] is not None,
          "the empty prompt is refused before anything is written")
    check("slug keeps words and drops the rest",
          _slug("Hello, World! 42") == "hello-world-42", _slug("Hello, World! 42"))
    check("seconds clamp to the model's own span",
          (_gen_seconds({"seconds": 99}, {"seconds": 2.0, "maxSeconds": 4.0}) == 4.0
           and _gen_seconds({}, {"seconds": 2.0, "maxSeconds": 4.0}) == 2.0
           and _gen_seconds({"seconds": 0.1}, {"seconds": 2.0, "maxSeconds": 4.0}) == 0.5),
          str(_gen_seconds({"seconds": 99}, {"seconds": 2.0, "maxSeconds": 4.0})))

    # The licence gate, through the door the Director calls: a monetized
    # ask is an ANSWER (`ok: true`, `reason: "licence"`, no `path`) so the
    # music stage's pad/shelf fallback is reachable in a real run, and the
    # `monetized` flag can only tighten the brief, never loosen it.
    with tempfile.TemporaryDirectory() as project:
        Path(project, "creative-brief.json").write_text(
            json.dumps({"monetized": True}), encoding="utf-8")
        resolved, refusal = _generator({"kind": "bgm", "project": project})
        check("a monetized brief refuses the CC-BY-NC bed",
              resolved is None and refusal.get("ok") is True
              and refusal.get("reason") == "licence"
              and refusal.get("generated") is False and "path" not in refusal,
              str(refusal or "no refusal"))
        check("the caller's flag tightens without a brief",
              _generator({"kind": "bgm", "monetized": True})[1]
              .get("reason") == "licence",
              "the flag alone is enough")
        check("a monetized ask of a commercial model passes",
              _generator({"kind": "sfx-small", "monetized": True,
                          "project": project})[1] is None
              or _generator({"kind": "sfx-small", "monetized": True})[1]
              .get("reason") != "licence",
              "MIT is commercial")
        check("the flag cannot loosen a monetized brief",
              _generator({"kind": "bgm", "monetized": False,
                          "project": project})[1]
              .get("reason") == "licence",
              "the brief's own word wins")
        Path(project, "creative-brief.json").write_text(
            json.dumps({"monetized": False}), encoding="utf-8")
        reply = _generator({"kind": "bgm", "project": project})[1]
        check("a brief that says not monetized frees the bed",
              reply is None or reply.get("reason") != "licence",
              "the gate is the brief's, not a blanket (the model may still be "
              "absent, which is a different answer)")

    # The memory branch of the same contract: a take that will not fit is
    # refused as an ANSWER too - ok: true, generated: false, reason
    # "memory" - so a warn-policy stage reads the reply from the run log
    # and falls back instead of recording a spurious tool failure. The
    # machine's free memory, the model's footprint and the model files
    # are patched to a take that cannot fit, so the branch answers on
    # any machine, whatever is installed.
    real_np, real_ort, real_tok = np, ort, Tokenizer
    real_available, real_set, real_file = _available_bytes, _set_bytes, _model_file
    try:
        globals()["np"] = np if np is not None else object()
        globals()["ort"] = ort if ort is not None else object()
        globals()["Tokenizer"] = Tokenizer if Tokenizer is not None else object()
        globals()["_available_bytes"] = lambda: 64 * 1024 * 1024
        globals()["_set_bytes"] = lambda spec, row: 2 * 1024 * 1024 * 1024
        globals()["_model_file"] = lambda spec, name: Path("/dev/null")
        tight, tight_why = _generator({"kind": "sfx-small"})
        check("an over-memory generator answers ok with no path",
              tight is None and tight_why.get("ok") is True
              and tight_why.get("generated") is False
              and tight_why.get("reason") == "memory"
              and "path" not in tight_why and "error" not in tight_why,
              str(tight_why.get("detail") or tight_why))
    finally:
        globals()["np"] = real_np
        globals()["ort"] = real_ort
        globals()["Tokenizer"] = real_tok
        globals()["_available_bytes"] = real_available
        globals()["_set_bytes"] = real_set
        globals()["_model_file"] = real_file

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
    checks.extend(_licence_selftest())

    failed = [c for c in checks if not c["ok"]]
    return {"tool": "selftest", "ok": not failed, "checks": checks,
            "passed": len(checks) - len(failed), "total": len(checks),
            "model": {"state": models.state, "reason": models.reason}}


def _licence_selftest():
    """The licence table and the gate that spends it, without any model."""
    checks = []
    kinds = {kind: row for kind, row in GEN_KINDS.items()}
    checks.append({
        "check": "the SFX generators are licensed for commercial work",
        "ok": kinds["sfx-small"].get("commercial") is True
        and kinds["sfx-large"].get("commercial") is True,
        "detail": ", ".join(f"{k}:{kinds[k].get('commercial')}" for k in ("sfx-small", "sfx-large")),
    })
    checks.append({
        "check": "the music generator is not: MusicGen-small is CC-BY-NC",
        "ok": kinds["bgm"].get("commercial") is False
        and "NC" in kinds["bgm"]["licence"],
        "detail": kinds["bgm"]["licence"],
    })
    with tempfile.TemporaryDirectory(prefix="funcut-licence-") as project:
        brief = Path(project) / "creative-brief.json"
        checks.append({
            "check": "a project with no brief is not monetized",
            "ok": _project_monetized(project) is False,
            "detail": "",
        })
        brief.write_text('{"monetized": true}')
        checks.append({
            "check": "a brief that says monetized is read as monetized",
            "ok": _project_monetized(project) is True,
            "detail": "",
        })
        _row, refusal = _generator({"kind": "bgm", "project": project})
        checks.append({
            "check": "a monetized project is refused the CC-BY-NC bed",
            "ok": refusal is not None and refusal.get("reason") == "licence"
            and "CC-BY-NC" in str(refusal.get("licence")),
            "detail": str((refusal or {}).get("reason")),
        })
        brief.write_text('{"monetized": false}')
        _row, refusal = _generator({"kind": "bgm", "project": project})
        checks.append({
            "check": "a non-monetized project is not refused on licence",
            "ok": refusal is None or refusal.get("reason") != "licence",
            "detail": str((refusal or {}).get("reason")),
        })
        _row, refusal = _generator({"kind": "bgm", "project": project, "monetized": True})
        checks.append({
            "check": "a caller that says monetized is refused even without a brief",
            "ok": refusal is not None and refusal.get("reason") == "licence",
            "detail": str((refusal or {}).get("reason")),
        })
        _row, refusal = _generator({"kind": "sfx-small", "project": project, "monetized": True})
        checks.append({
            "check": "the commercial generators are never refused on licence",
            "ok": refusal is None or refusal.get("reason") != "licence",
            "detail": str((refusal or {}).get("reason")),
        })
    return checks


# ── Generation ──────────────────────────────────────────────────────────
#
# Three text-to-audio models, downloaded by the settings sheet into the
# same `cutout-models` folder the reader uses:
#
#   sfx-small  TinyAudio-MF        87 M, one mean-flow step, 10 s @ 44.1 kHz
#   sfx-large  Stable Audio Open Small  341 M, eight distilled steps, 11.9 s stereo
#   bgm        TinyMusician        MusicGen 15 layers, four codebooks, 32 kHz
#
# Every one is ONNX plus a Hugging Face tokenizer, so the dependency is the
# same numpy + onnxruntime + tokenizers the reader already asks for.
#
# Memory is the constraint on the laptops this runs on. Sessions are built
# lazily inside one call and released when it ends - `_release_sessions`
# drops the graphs before the process exits, so a caller that asks for a
# sound twice in one spec never pays for the load twice, and a caller that
# asks once pays for nothing after the file is written. A model whose
# weights would not fit the machine's free memory is refused before the
# load rather than swapping the editor out.
#
# `commercial` is the licence, not a preference: TinyMusician is a
# MusicGen-small export, and MusicGen-small's weights are CC-BY-NC 4.0, so
# a bed it writes may not go into monetized work. The generator reads the
# project's own `creative-brief.json` and refuses that bed instead of
# writing a file the edit cannot legally ship.

GEN_KINDS = {
    "sfx-small": {
        "model": "tiny-audio-mf",
        "files": [
            "sfx-small-pipeline.json",
            "sfx-small-tokenizer.json",
            "sfx-small-text.onnx",
            "sfx-small-flow.onnx",
            "sfx-small-vae.onnx",
        ],
        "seconds": 3.0,
        "maxSeconds": 30.0,
        "steps": 1,
        "licence": "MIT",
        "commercial": True,
    },
    "sfx-large": {
        "model": "stable-audio-open-small",
        "files": [
            "sfx-large-meta.json",
            "sfx-large-tokenizer.json",
            "sfx-large-seconds.json",
            "sfx-large-t5.onnx",
            "t5_text_encoder.onnx.data",
            "sfx-large-dit.onnx",
            "dit_step.onnx.data",
            "sfx-large-vae.onnx",
            "vae_decoder.onnx.data",
        ],
        "seconds": 4.0,
        "maxSeconds": 11.9,
        "steps": 8,
        "licence": "Stability AI Community License",
        # Free for commercial work under $1M annual revenue, with the
        # licence's own attribution terms; not a public-domain grant.
        "commercial": True,
    },
    "bgm": {
        "model": "tinymusician",
        "files": [
            "bgm-config.json",
            "bgm-tokenizer.json",
            "bgm-text.onnx",
            "bgm-decoder.onnx",
            "bgm-codec.onnx",
        ],
        "seconds": 8.0,
        "maxSeconds": 10.0,
        "steps": None,
        # The repository carries no licence file, but its `base_model` is
        # facebook/musicgen-small, whose weights are CC-BY-NC 4.0: an
        # export of those weights is bound by them, so this is the licence
        # however the repository is labelled.
        "licence": "CC-BY-NC-4.0 (MusicGen-small weights)",
        "commercial": False,
    },
}


def _slug(text, limit=48):
    kept = "".join(ch.lower() if ch.isalnum() else "-" for ch in text)
    kept = "-".join(part for part in kept.split("-") if part)
    return (kept[:limit].strip("-") or "sound")


def _project_monetized(project):
    """Whether the project's own brief claims the work is monetized.

    The same file and the same field the API's licence gate reads
    (`creative-brief.json`, top-level `monetized`), so a generated bed and
    an imported shelf track are held to one rule. No brief, or no flag, is
    non-monetized - a legacy project says nothing; a brief that cannot be
    read is monetized, because a broken file is not permission.
    """
    if not project:
        return False
    brief = Path(project) / "creative-brief.json"
    try:
        text = brief.read_text()
    except FileNotFoundError:
        return False
    except OSError:
        return True
    try:
        data = json.loads(text)
    except ValueError:
        return True
    return data.get("monetized") is True


def _available_bytes():
    """What the machine can hand out without swapping: None when unknown."""
    try:
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemAvailable:"):
                return int(line.split()[1]) * 1024
    except OSError:
        pass
    return None


def _set_bytes(spec, row):
    """The bytes one kind's files take on disk, as installed."""
    total = 0
    for name in row["files"]:
        path = _model_file(spec, name)
        if path is not None:
            total += path.stat().st_size
    return total


def _generator(spec):
    """The kind's table row, or the reason it cannot run.

    The order is the order of the answers: a model this project may not
    use is refused before anything is read, a model that is not downloaded
    is refused before anything is loaded, and a model that will not fit is
    refused before it swaps the machine.

    And every one of those refusals is an ANSWER, not an error: the reply
    says `ok: true` with no `path` (the reader's own degrade pattern),
    because the Director's music and SFX stages read this reply from the
    run log and shop the shelf when it carries no `path`. A refusal
    phrased as an error never reaches that log - the runner only records
    replies, not errors - and a stage that never sees the refusal ends
    warned, with no bed and no accents, which is a regression the shelf
    fallback was written to prevent.
    """
    kind = str(spec.get("kind") or "sfx-small")
    row = GEN_KINDS.get(kind)
    if row is None:
        # A spelling mistake is an error, not an answer: nothing downstream
        # can usefully react to it.
        return None, {"tool": "generate", "ok": False,
                      "error": f"unknown kind {kind!r}: one of {sorted(GEN_KINDS)}"}
    # The brief is the authority; a caller that says the work is monetized
    # can only tighten it, never loosen it - an unattended run knows its
    # own plan, and a project whose brief was never written does not.
    monetized = spec.get("monetized") is True or _project_monetized(spec.get("project"))
    if not row.get("commercial", True) and monetized:
        # An ANSWER, not an error: the Director's music stage reads this
        # reply from the run log, sees `reason: "licence"` with no `path`,
        # and asks sfx-large for a pad or shops the FreePD shelf instead.
        return None, {
            "tool": "generate",
            "ok": True,
            "generated": False,
            "model": row["model"],
            "kind": kind,
            "licence": row["licence"],
            "reason": "licence",
            "detail": f"{row['model']} writes under {row['licence']}, which does not "
            "allow monetized work: this project's brief says it is monetized. "
            "Use the CC0 shelf, `sfx-large` (Stability AI Community Licence) for a "
            "written bed, or mark the project non-monetized",
        }
    if np is None or ort is None or Tokenizer is None:
        missing = []
        if np is None:
            missing.append("numpy")
        if ort is None:
            missing.append("onnxruntime")
        if Tokenizer is None:
            missing.append("tokenizers")
        return None, {
            "tool": "generate",
            "ok": True,
            "model": "absent",
            "generated": False,
            "kind": kind,
            "reason": "the generators need " + ", ".join(missing)
            + " (pip install numpy onnxruntime tokenizers)",
            "missing": missing,
        }
    absent = [name for name in row["files"] if _model_file(spec, name) is None]
    if absent:
        return None, {
            "tool": "generate",
            "ok": True,
            "model": "absent",
            "generated": False,
            "kind": kind,
            "reason": f"the {kind} model is not downloaded: missing {', '.join(absent)} "
            "(Settings > Models, or models_download)",
            "missing": absent,
        }
    needed = _set_bytes(spec, row)
    available = _available_bytes()
    if available is not None and needed and needed * 1.35 > available:
        # An ANSWER, not an error: every refusal in this generator's own
        # contract is a reply the Director's stages can read from the run
        # log - an ok:false tool failure never reaches that log, and a
        # stage that never sees the refusal ends warned with no bed and
        # no accents.
        return None, {
            "tool": "generate",
            "ok": True,
            "model": row["model"],
            "kind": kind,
            "generated": False,
            "reason": "memory",
            "detail": f"the {kind} model needs about {needed * 1.35 / 1e9:.2f} GB and "
            f"{available / 1e9:.2f} GB is free: close what you can, or use a smaller "
            "kind (`sfx-small` is 0.35 GB)",
        }
    return (kind, row), None


# The graphs one call has open, by file and thread count: a call that
# writes several sounds loads each graph once, and `_release_sessions`
# lets them go before the process ends.
_SESSIONS = {}


def _session(name, spec, threads):
    path = _model_file(spec, name)
    key = (str(path), threads)
    held = _SESSIONS.get(key)
    if held is not None:
        return held
    options = ort.SessionOptions()
    options.intra_op_num_threads = threads
    options.inter_op_num_threads = 1
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    options.log_severity_level = 3
    session = ort.InferenceSession(str(path), options, providers=["CPUExecutionProvider"])
    _SESSIONS[key] = session
    return session


def _release_sessions():
    """Drop every graph this call loaded, and the arena behind them."""
    if not _SESSIONS:
        return
    _SESSIONS.clear()
    gc.collect()


def _threads(spec):
    named = spec.get("threads")
    if isinstance(named, (int, float)) and named >= 1:
        return min(int(named), 16)
    # Half the cores, never more than four: this runs beside a window that
    # is drawing frames, and on a four-core laptop all four threads make
    # the editor stutter for a sound it is not yet playing.
    return max(1, min(4, (os.cpu_count() or 2) // 2))


def _gen_seconds(spec, row):
    named = spec.get("seconds")
    seconds = float(named) if isinstance(named, (int, float)) else float(row["seconds"])
    return max(0.5, min(seconds, float(row.get("maxSeconds") or 30.0)))


def _normalise(audio, rate=44100):
    """Peak-normalise to 0.89 and fade the ends, so a placed clip neither
    clips nor clicks at its boundaries."""
    peak = float(np.abs(audio).max()) if audio.size else 0.0
    if peak > 1e-6:
        audio = audio * (0.89 / peak)
    fade = min(int(0.01 * rate), audio.shape[-1] // 4 if audio.ndim else 0)
    if fade > 1:
        ramp = np.linspace(0.0, 1.0, fade, dtype=np.float32)
        audio[..., :fade] *= ramp
        audio[..., -fade:] *= ramp[::-1]
    return audio


def _write_float_wav(path, audio, rate):
    """`audio` is (samples,) mono or (2, samples) stereo, float in [-1, 1]."""
    import wave

    audio = np.asarray(audio, dtype=np.float32)
    if audio.ndim == 2:
        inter = np.empty(audio.shape[1] * audio.shape[0], dtype=np.float32)
        for channel in range(audio.shape[0]):
            inter[channel:: audio.shape[0]] = audio[channel]
        channels = audio.shape[0]
    else:
        inter = audio.reshape(-1)
        channels = 1
    pcm = (np.clip(inter, -1.0, 1.0) * 32767.0).astype(np.int16)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(channels)
        handle.setsampwidth(2)
        handle.setframerate(int(rate))
        handle.writeframes(pcm.tobytes())
    return channels, len(inter) // channels


def _gen_tiny(spec, prompt, seconds, seed, threads, steps):
    pipeline = json.loads(_model_file(spec, "sfx-small-pipeline.json").read_text())
    rate = int(pipeline.get("audio_sample_rate") or 44100)
    max_tokens = int(pipeline.get("prompt_max_length") or 77)
    latent_len = int(pipeline.get("latent_seq_len") or 431)
    latent_dim = int(pipeline.get("latent_dim") or 64)

    tokenizer = Tokenizer.from_file(str(_model_file(spec, "sfx-small-tokenizer.json")))
    pad_id = tokenizer.token_to_id("<pad>")
    tokenizer.enable_truncation(max_length=max_tokens)
    tokenizer.enable_padding(length=max_tokens, pad_id=0 if pad_id is None else pad_id)
    encoded = tokenizer.encode(prompt)
    ids = np.array([encoded.ids], dtype=np.int64)
    mask = np.array([encoded.attention_mask], dtype=np.int64)

    text = _session("sfx-small-text.onnx", spec, threads)
    flow = _session("sfx-small-flow.onnx", spec, threads)
    vae = _session("sfx-small-vae.onnx", spec, threads)

    hidden, _global, padding = text.run(
        ["text_hidden", "text_global", "text_padding_mask"],
        {"input_ids": ids, "attention_mask": mask},
    )
    rng = np.random.default_rng(seed)
    x = rng.standard_normal((1, latent_len, latent_dim), dtype=np.float32)
    steps = max(1, min(int(steps or pipeline.get("default_num_steps") or 1), 8))
    schedule = np.linspace(1.0, 0.0, steps + 1, dtype=np.float32)
    for i in range(steps):
        x = flow.run(
            ["x_next"],
            {
                "text_hidden": hidden.astype(np.float32),
                "text_padding_mask": padding.astype(bool),
                "x": x,
                "t": schedule[i : i + 1],
                "r": schedule[i + 1 : i + 2],
                "cfg_strength": np.array([7.0], dtype=np.float32),
                "cfg_interval_min": np.array([0.0], dtype=np.float32),
                "cfg_interval_max": np.array([1.0], dtype=np.float32),
            },
        )[0]
    audio = np.asarray(vae.run(["audio"], {"x": x.astype(np.float32)})[0], np.float32).reshape(-1)
    audio = audio[: int(seconds * rate)]
    return audio, rate


def _gen_stable(spec, prompt, seconds, seed, threads, steps):
    text_tokens, cond_dim = 64, 768
    rate = 44100
    tokenizer = Tokenizer.from_file(str(_model_file(spec, "sfx-large-tokenizer.json")))
    tokenizer.enable_truncation(max_length=text_tokens)
    tokenizer.enable_padding(length=text_tokens, pad_id=0)
    encoded = tokenizer.encode(prompt)
    ids = np.array([encoded.ids], dtype=np.int64)
    mask = np.array([encoded.attention_mask], dtype=np.int64)

    t5 = _session("sfx-large-t5.onnx", spec, threads)
    dit = _session("sfx-large-dit.onnx", spec, threads)
    vae = _session("sfx-large-vae.onnx", spec, threads)

    embeddings = t5.run(["embeddings"], {"input_ids": ids, "attention_mask": mask})[0]
    embedder = json.loads(_model_file(spec, "sfx-large-seconds.json").read_text())
    fourier = np.array(embedder["fourier_weights"], dtype=np.float32)
    weight = np.array(embedder["linear_weight"], dtype=np.float32)
    bias = np.array(embedder["linear_bias"], dtype=np.float32)
    scaled = np.float32(seconds / float(embedder.get("max_val") or 256.0))
    feats = np.empty(2 * len(fourier) + 1, dtype=np.float32)
    feats[0] = scaled
    angles = scaled * fourier * 2 * math.pi
    feats[1 : 1 + len(fourier)] = np.sin(angles)
    feats[1 + len(fourier) :] = np.cos(angles)
    seconds_embedding = (weight @ feats + bias)[None, :]
    cross = np.zeros((1, text_tokens + 1, cond_dim), dtype=np.float32)
    cross[0, :text_tokens] = np.where(mask[0][:, None] > 0, embeddings[0], 0)
    cross[0, text_tokens] = seconds_embedding[0]

    steps = max(1, min(int(steps or 8), 16))
    schedule = np.zeros(steps + 1, dtype=np.float32)
    for i in range(steps + 1):
        u = 1.0 - i / steps
        schedule[i] = 1.0 if i == 0 else (0.0 if i == steps else 1.0 / (1.0 + math.exp(-(8.2 * u - 2.0))))
    rng = np.random.default_rng(seed)
    latents = rng.standard_normal((1, 64, 256), dtype=np.float32)
    for i in range(steps):
        current, following = schedule[i], schedule[i + 1]
        velocity = dit.run(
            ["v"],
            {
                "x": latents,
                "t": np.array([current], np.float32),
                "cross_attn_cond": cross,
                "global_cond": seconds_embedding,
            },
        )[0]
        denoised = latents - current * velocity
        if following == 0:
            latents = denoised
        else:
            latents = (1 - following) * denoised + following * rng.standard_normal(
                (1, 64, 256), dtype=np.float32
            )
    audio = np.asarray(vae.run(["audio"], {"latent": latents.astype(np.float32)})[0], np.float32)[0]
    frames = min(int(seconds * rate), audio.shape[1])
    return audio[:, :frames], rate


def _gen_musician(spec, prompt, seconds, seed, threads):
    config = json.loads(_model_file(spec, "bgm-config.json").read_text())
    rate = int((config.get("audio_encoder") or {}).get("sampling_rate") or 32000)
    codebooks = int((config.get("decoder") or {}).get("num_codebooks") or 4)
    bos = 2048
    # One codebook frame is 640 samples at 32 kHz in this export: the
    # measured 160-token run decoded to 3.12 s.
    frames = max(codebooks + 8, int(seconds * rate / 640.0) + 1)

    tokenizer = Tokenizer.from_file(str(_model_file(spec, "bgm-tokenizer.json")))
    encoded = tokenizer.encode(prompt)
    ids = np.array([encoded.ids], dtype=np.int64)
    mask = np.array([encoded.attention_mask], dtype=np.int64)

    text = _session("bgm-text.onnx", spec, threads)
    decoder = _session("bgm-decoder.onnx", spec, threads)
    codec = _session("bgm-codec.onnx", spec, threads)

    hidden = text.run(["last_hidden_state"], {"input_ids": ids, "attention_mask": mask})[0]

    def build(seq_len, max_length):
        shifted = np.full((1, codebooks, max_length), -1, dtype=np.int64)
        tokens = np.full((1, codebooks, seq_len), bos, dtype=np.int64)
        for channel in range(codebooks):
            shifted[:, channel, channel : channel + seq_len] = tokens[:, channel]
        delay = np.zeros((codebooks, max_length), dtype=bool)
        for channel in range(codebooks):
            delay[channel, max_length - codebooks + 1 + channel :] = True
            delay[channel, : channel + 1] = True
        out = np.where(~delay, shifted[0], bos)
        starts = np.nonzero(out[0] == -1)[0]
        first = int(starts.min()) if len(starts) else seq_len
        return out[:, :first], out

    def apply(sequence, pattern):
        width = sequence.shape[1]
        return np.where(pattern[:, :width] == -1, sequence, pattern[:, :width])

    # The exported merged decoder's cache branch is broken (it reshapes a
    # zero-batch past into a non-empty present), so the whole prefix is fed
    # every step with `use_cache_branch` false. That is quadratic in the
    # length, which is why the music model is the slow one.
    past = {}
    for layer in range(int(config.get("decoder", {}).get("num_hidden_layers") or 15)):
        for name in ("decoder.key", "decoder.value", "encoder.key", "encoder.value"):
            past[f"past_key_values.{layer}.{name}"] = np.zeros((1, 16, 0, 64), np.float32)
    sequence, pattern = build(1, frames)
    rng = np.random.default_rng(seed)
    for _ in range(frames - 1):
        feed = dict(past)
        feed.update(
            {
                "input_ids": apply(sequence, pattern),
                "encoder_attention_mask": mask,
                "encoder_hidden_states": hidden,
                "use_cache_branch": np.array([False], dtype=bool),
            }
        )
        logits = decoder.run(None, feed)[0][:, -1, :]
        next_tokens = np.empty(codebooks, dtype=np.int64)
        for channel in range(codebooks):
            row = logits[channel].astype(np.float64)
            top = np.argpartition(-row, 50)[:50]
            scores = row[top] - row[top].max()
            weights = np.exp(scores)
            weights /= weights.sum()
            next_tokens[channel] = top[rng.choice(len(top), p=weights)]
        sequence = np.concatenate([sequence, next_tokens[:, None]], axis=1)
    codes = apply(sequence, pattern)
    codes = codes[codes != bos].reshape(1, 1, codebooks, -1)
    audio = np.asarray(codec.run(["audio_values"], {"audio_codes": codes})[0], np.float32).reshape(-1)
    return audio[: int(seconds * rate)], rate


def generate_sound(spec):
    """Write a short sound or a music bed from words.

    The kind names the model, the prompt names the sound, and the file
    lands where the caller can import it: inside `project` when one is
    named, so an auto edit never writes outside the folder it edits.
    A model that is absent, a licence a monetized cut may not spend, or
    a generation that comes back silent all answer without a `path`, so
    the Director shops the shelf rather than shipping nothing.
    """
    resolved, refusal = _generator(spec)
    if refusal is not None:
        return refusal
    kind, row = resolved
    prompt = str(spec.get("prompt") or "").strip()
    if not prompt:
        return {
            "tool": "generate",
            "ok": False,
            "error": "name `prompt`: the words the model writes from",
        }
    seconds = _gen_seconds(spec, row)
    seed = int(spec.get("seed") or 0)
    threads = _threads(spec)
    steps = spec.get("steps")
    if steps is None:
        steps = row.get("steps")
    started = time.time()
    if kind == "sfx-small":
        audio, rate = _gen_tiny(spec, prompt, seconds, seed, threads, steps)
        channels = 1
    elif kind == "sfx-large":
        audio, rate = _gen_stable(spec, prompt, seconds, seed, threads, steps)
        channels = 2
    else:
        audio, rate = _gen_musician(spec, prompt, seconds, seed, threads)
        channels = 1
    # The generated audio is checked before it is normalised, because a
    # normaliser lifts any level to 0.89: a model that came back
    # near-silent (its loudest sample 40 dB under full scale) wrote
    # nothing a cut can spend, and the answer carries no `path`, so the
    # Director shops the shelf rather than placing a silent file and
    # calling the moment accented.
    raw_peak = float(np.abs(audio).max()) if getattr(audio, "size", 0) else 0.0
    if raw_peak < 0.02:
        return {
            "tool": "generate",
            "ok": True,
            "model": row["model"],
            "generated": False,
            "kind": kind,
            "licence": row["licence"],
            "prompt": prompt,
            "reason": f"the generator wrote near-silence (peak {raw_peak:.4f}): "
            "not placed - shopping the shelf instead",
            "seed": seed,
            "steps": steps,
            "threads": threads,
            "elapsedSec": _r(time.time() - started, 2),
        }
    audio = _normalise(np.asarray(audio, np.float32), rate)

    project = spec.get("project")
    out = spec.get("out")
    if out:
        path = Path(out)
        if project and not path.is_absolute():
            path = Path(project) / path
    elif project:
        path = Path(project) / "generated-audio" / f"{kind}-{_slug(prompt)}-{seed}.wav"
    else:
        path = Path(tempfile.mkdtemp(prefix="funcut-sound-")) / f"{kind}-{_slug(prompt)}-{seed}.wav"
    if project and path.is_absolute():
        try:
            path.resolve().relative_to(Path(project).resolve())
        except ValueError:
            return {"ok": False, "error": "`out` must stay inside `project`"}
    channels, frames = _write_float_wav(path, audio, rate)
    reply = {
        "ok": True,
        "tool": "generate",
        "kind": kind,
        "model": row["model"],
        "licence": row["licence"],
        "commercial": row.get("commercial", True),
        "prompt": prompt,
        "path": str(path),
        "seconds": _r(frames / rate, 3),
        "rate": rate,
        "channels": channels,
        "peak": _r(float(np.abs(audio).max()) if audio.size else 0.0, 3),
        "rms": _r(float(np.sqrt((audio.astype(np.float64) ** 2).mean())) if audio.size else 0.0, 4),
        "seed": seed,
        "steps": steps,
        "threads": threads,
        "elapsedSec": _r(time.time() - started, 2),
    }
    _write_licence_record(path, reply)
    return reply


def _write_licence_record(path, reply):
    """Leave the licence beside the file it governs.

    The project folder outlives this process and travels with the edit, so
    the record - not the tool's reply - is what a later import can check.
    A record that cannot be written is not fatal: the reply still carries
    the licence, and a reader without one treats the file as it treats any
    other unlabelled media.
    """
    record = {
        "model": reply["model"],
        "kind": reply["kind"],
        "licence": reply["licence"],
        "commercial": reply["commercial"],
        "prompt": reply["prompt"],
        "seed": reply["seed"],
    }
    try:
        beside = Path(str(reply["path"]) + ".licence.json")
        beside.write_text(json.dumps(record, indent=2) + "\n")
    except OSError:
        pass


TOOLS = {
    "describe": describe_sound,
    "index": index_library,
    "rank": rank_sounds,
    "plan": plan_sound,
    "generate": generate_sound,
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
    finally:
        _release_sessions()


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
