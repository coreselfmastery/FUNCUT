#!/usr/bin/env python3
"""concat-sound-sense: what a sound file actually IS, measured.

Pure Python standard library, the same contract as the other sense
scripts: `sound_sense.py <analyse|selftest> --spec -` reads one JSON spec
from stdin and prints one JSON object on stdout. The spec names the tool
in `tool`.

WHY THIS EXISTS

The shelf ranks sounds by what their text SAYS - name, tags, id, category,
use-for, description (`concat-host/src/sounds.rs`, `Listed::score`) - and
the music library ranks tracks by title, artist and category
(`concat-host/src/bgm.rs`). Nothing in the editor has ever opened the file.
So a search for "deep cinematic reveal" answers with whatever a curator
wrote in a JSON field, the Director picks the third hit because the first
two were spent, and `syncAt` - a claim in the manifest - is trusted for
where the sound's peak lands.

This module is the missing ear. It measures the decoded waveform and says
what the sound IS: how bright, how heavy, how sharp the attack, how long
the body, whether it is noise-like air or a tone, whether its energy rises,
and where its peak actually falls. Then it scores those measurements
against the six sound roles a cut actually needs, with the formula and the
evidence returned so a Director can read why.

WHAT IT IS NOT

No model, no download, no numpy requirement. A CLAP embedding would rank
by a learned audio-text space and would be a better answer to "does this
sound like the sentence" - it is named in docs/SOUND-SENSE.md as the
upgrade behind this same interface. What ships here is deterministic,
explainable and instant, which is what a correction loop can be built on:
an issue it reports always comes with the number that proves it.

THE FOUR TOOLS

  describe_sound   one file: duration, peak and RMS level, crest factor,
                   attack, body, decay, transient count, spectral centroid,
                   band balance, flatness (noise vs tone), pitch movement,
                   energy trend, and the peak's measured position - then
                   the six role fits with the evidence behind each
  rank_sounds      a role (or words naming one) and a list of files:
                   candidates ordered by measured fit, with `why`
  check_placement  one placed accent judged against its intent: is the
                   peak on the moment, is it the right sound, is it too
                   loud for the voice under it, is this the fourth whoosh
                   in ten seconds - each issue with its fix
  index_library    measure a whole library once, cached by size+mtime,
                   and report where the manifest's `syncAt` disagrees with
                   the file's real peak

Everything degrades loudly: a file that will not decode is an error with
the reason, never a zero-filled reading.
"""

from __future__ import annotations

import argparse
import array
import io
import json
import math
import os
import sys
import tempfile
import time
import wave
from pathlib import Path

try:
    import voice_signals as _vs
except ImportError:  # pragma: no cover
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import voice_signals as _vs

try:
    import voice_quality as _vq
except ImportError:  # pragma: no cover
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import voice_quality as _vq

SCHEMA = "funcut.sound-sense/1"
# Bumped whenever the read or the role formulas change: an index written by
# an older engine is not reused, because a cached role is a claim the old
# numbers made and the new ones never checked.
INDEX_VERSION = 2

# 32 kHz, not voice_quality's 16 kHz: the top octave is where a whoosh
# lives. 16 kHz would put the air band's ceiling at 8 kHz and call every
# bright sweep "mid-forward".
RATE = 32000
HOP = 512  # 16 ms envelope frames
WIN = 2048  # 64 ms FFT window at 32 kHz: 15.6 Hz bins
MAX_WINDOWS = 24  # spectral windows per file: enough for a stable read, bounded for a library sweep
TRUNCATE_SEC = 45.0  # a bed longer than this is measured at its first 45 s and says so

# The six roles an accent is shopped for. These are the words the Director's
# own sound stage reaches for (`sfx_queries_for_emotion` and
# `sfx_queries_for_line` in director_program.rs), named as what they ARE
# rather than as the shelf's categories.
ROLES = (
    "quick_transition",
    "cinematic_reveal",
    "action_impact",
    "gentle_entrance",
    "emotional_texture",
    "tension_build",
)

# Words a caller may use instead of a role name: the library's categories,
# the Director's registers, and the six descriptions the plan is written in.
ROLE_WORDS = {
    "quick_transition": ("transition", "whoosh", "swish", "whip", "swipe", "swoosh",
                         "fast", "swish", "cut", "air"),
    "cinematic_reveal": ("reveal", "cinematic", "boom", "braam", "deep", "sub",
                         "trailer", "dramatic", "sweep", "wide"),
    "action_impact": ("impact", "hit", "slam", "punch", "action", "heavy",
                      "aggressive", "thud", "stinger"),
    "gentle_entrance": ("gentle", "soft", "pop", "chime", "click", "ui",
                        "notification", "light", "delicate", "airy"),
    "emotional_texture": ("texture", "pad", "ambience", "ambient", "drone",
                          "emotional", "subtle", "tone", "warm", "bed"),
    "tension_build": ("riser", "build", "tension", "rise", "uplift", "sweep",
                      "suspend", "dread"),
}

_PCM = {}


def _db(x, floor=1e-9):
    return 20.0 * math.log10(max(x, floor))


def _clamp(x, lo=0.0, hi=1.0):
    return lo if x < lo else hi if x > hi else x


def _n(x, lo, hi):
    """`x` placed on 0..1 between `lo` and `hi`, clamped at both ends."""
    if hi == lo:
        return 0.0
    return _clamp((x - lo) / (hi - lo))


def _r(x, places=4):
    """A JSON-safe rounded number: serde_json refuses NaN and Infinity, and
    a division by an empty buffer is exactly how a metric becomes one."""
    if x is None or not math.isfinite(float(x)):
        return None
    return round(float(x), places)


# ---------------------------------------------------------------------------
# The decode: one file, 32 kHz mono, cached for the call
# ---------------------------------------------------------------------------


def _to_mono(frames, channels):
    if channels <= 1:
        return frames
    mono = array.array("h", bytes(len(frames) // channels * 2))
    for i in range(0, len(frames) - channels + 1, channels):
        mono[i // channels] = sum(frames[i:i + channels]) // channels
    return mono


def _resample(frames, rate, target=RATE):
    """Linear resample. Honest for envelopes, transients and spectra at
    these ratios; the library's own files are 44.1/48 kHz, both within a
    1.5x stretch of 32 kHz."""
    if rate == target or not len(frames):
        return frames
    step = rate / target
    last = len(frames) - 1
    out = array.array("h", bytes(int(len(frames) / step) * 2))
    for i in range(len(out)):
        pos = i * step
        at = int(pos)
        frac = pos - at
        a = frames[at] if at <= last else frames[last]
        b = frames[at + 1] if at + 1 <= last else a
        out[i] = int(a + (b - a) * frac)
    return out


def _decode(spec, path):
    """Decode `path` to (samples, rate) mono at 32 kHz. A WAV decodes with
    the standard library alone, which is the selftest's contract; every
    other container is ffmpeg's business, through the shared bootstrap."""
    key = str(path)
    if key in _PCM:
        return _PCM[key]
    if not Path(str(path)).is_file():
        return None, None
    samples = None
    rate = RATE
    try:
        with wave.open(str(path), "rb") as handle:
            channels = handle.getnchannels()
            width = handle.getsampwidth()
            rate = handle.getframerate()
            raw = handle.readframes(handle.getnframes())
        if width == 2:
            frames = array.array("h", raw[: len(raw) // 2 * 2])
        elif width == 1:
            frames = array.array("h", ((byte - 128) * 256 for byte in raw))
        elif width == 4:
            wide = array.array("i", raw[: len(raw) // 4 * 4])
            frames = array.array("h", (max(-32768, min(32767, s >> 16)) for s in wide))
        else:
            raise ValueError(f"{width}-byte PCM is not a shape this reader takes")
        samples = _resample(_to_mono(frames, channels), rate)
        rate = RATE
    except (wave.Error, ValueError, EOFError):
        samples = None
    if samples is None:
        ff, _ = _vs._ffmpeg(spec)
        cmd = [ff, "-hide_banner", "-nostdin", "-i", str(path), "-ac", "1",
               "-ar", str(RATE), "-c:a", "pcm_s16le", "-f", "wav", "-"]
        rc, out, _err = _vs._run(cmd, 240)
        if rc != 0 or not out:
            return None, None
        try:
            with wave.open(io.BytesIO(out)) as handle:
                rate = handle.getframerate()
                raw = handle.readframes(handle.getnframes())
            samples = array.array("h", raw[: len(raw) // 2 * 2])
        except Exception:  # noqa: BLE001
            return None, None
    result = (samples, rate)
    _PCM[key] = result
    return result


# ---------------------------------------------------------------------------
# 1. The time-domain read: level, attack, body, transients, the peak
# ---------------------------------------------------------------------------


def _envelope(samples, hop=HOP):
    """RMS per hop, linear. The base layer every time measurement reads."""
    out = []
    for i in range(0, len(samples) - hop + 1, hop):
        acc = 0
        chunk = samples[i:i + hop]
        for s in chunk:
            acc += s * s
        out.append(math.sqrt(acc / hop) / 32768.0)
    return out


def _peak_index(samples):
    best, at = -1.0, 0
    for i, s in enumerate(samples):
        a = s if s >= 0 else -s
        if a > best:
            best, at = a, i
    return best / 32768.0, at


def _time_read(samples, rate):
    """Levels, the envelope and the transient shape. Everything here is a
    measurement of THIS file; nothing is a threshold borrowed from taste."""
    hop = HOP
    env = _envelope(samples, hop)
    if not env:
        return None
    fps = rate / hop
    peak, peak_at_sample = _peak_index(samples)
    peak_frame = min(len(env) - 1, peak_at_sample // hop)
    peak_frame_db = _db(env[peak_frame])
    env_db = [_db(v) for v in env]
    rms = math.sqrt(sum(v * v for v in env) / len(env))

    active = [i for i, db in enumerate(env_db) if db >= peak_frame_db - 40.0]
    attacked = peak_frame
    while attacked > 0 and env_db[attacked - 1] >= peak_frame_db - 30.0:
        attacked -= 1
    decayed = peak_frame
    while decayed + 1 < len(env_db) and env_db[decayed + 1] >= peak_frame_db - 20.0:
        decayed += 1
    body = sum(1 for db in env_db if db >= peak_frame_db - 20.0) / fps

    # Transients: local maxima standing clear of the envelope's own neighbour,
    # at least 40 ms apart - one hit is not a cluster.
    min_gap = int(0.04 * fps)
    # Smoothed first: on noise, the raw envelope's own jitter is full of
    # little maxima, and a soft continuous whoosh used to report eight
    # "transients". Three frames (48 ms) is shorter than any real hit and
    # longer than the jitter.
    smooth = [
        sum(env_db[max(0, i - 1):i + 2]) / len(env_db[max(0, i - 1):i + 2])
        for i in range(len(env_db))
    ]
    transient_times = []
    last = -min_gap
    for i in range(1, len(smooth) - 1):
        if smooth[i] < peak_frame_db - 12.0:
            continue
        if smooth[i] < smooth[i - 1] or smooth[i] < smooth[i + 1]:
            continue
        if i - last < min_gap:
            continue
        transient_times.append(round(i / fps, 3))
        last = i

    # Energy trend over the active span, in dB per second: a riser climbs,
    # a braam holds, a one-shot falls.
    xs = [i / fps for i in active]
    ys = [env_db[i] for i in active]
    slope = _slope(xs, ys) if len(xs) >= 3 else 0.0
    # The same trend over the WHOLE file. A sparse ambience - thirty seconds
    # of birds with one loud crow at the end - has a steep trend across its
    # active frames and a flat one across its length, and only the second is
    # a description of the file. The rise a build is judged on is the
    # smaller of the two.
    whole = _slope([i / fps for i in range(len(env_db))], env_db) if len(env_db) >= 3 else 0.0
    # A build does not merely end loud, it CLIMBS: the level at the end is
    # well above the level at the start and the walk there is mostly upward.
    # Without the second half of that, thirty seconds of birds with one crow
    # at the end was the strongest build in the library - the peak was late,
    # the trend across the whole file was flat, and only steadiness says so.
    fifth = max(1, len(env_db) // 5)
    gain_db = _median(env_db[-fifth:]) - _median(env_db[:fifth]) if len(env_db) >= 2 else 0.0
    # A tail that has faded to digital silence reads as -180 dB and would
    # print a gain of -164, which is true and useless; -96 dB is the floor
    # at which a file is silent and says so.
    gain_db = _clamp(gain_db, -96.0, 96.0)
    steps = [post - pre for pre, post in zip(env_db, env_db[1:])]
    steady = sum(1 for step in steps if step >= -0.5) / len(steps) if steps else 0.0

    return {
        "duration": len(samples) / rate,
        "peak_dbfs": _db(peak),
        "rms_dbfs": _db(rms),
        "crest_db": _db(peak) - _db(rms),
        "peak_at": peak_at_sample / rate,
        "peak_frac": peak_at_sample / max(1, len(samples)),
        "attack_ms": (peak_frame - attacked) / fps * 1000.0,
        "body_s": body,
        "decay_s": (decayed - peak_frame) / fps,
        "transient_times": transient_times[:8],
        "transients": len(transient_times),
        "energy_slope_db_s": slope,
        "slope_full_db_s": whole,
        "gain_db": gain_db,
        "steady_rise": steady,
        "env_db": env_db,
        "fps": fps,
    }


def _slope(xs, ys):
    n = len(xs)
    mx = sum(xs) / n
    my = sum(ys) / n
    den = sum((x - mx) ** 2 for x in xs)
    if den <= 0:
        return 0.0
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den


# ---------------------------------------------------------------------------
# 2. The spectral read: brightness, weight, noise-vs-tone, movement
# ---------------------------------------------------------------------------

BANDS = (
    ("sub", 20.0, 80.0),
    ("low", 80.0, 250.0),
    ("mid", 250.0, 2000.0),
    ("presence", 2000.0, 6000.0),
    ("air", 6000.0, 12000.0),
)


def _median(values):
    if not values:
        return None
    vals = sorted(values)
    mid = len(vals) // 2
    return vals[mid] if len(vals) % 2 else 0.5 * (vals[mid - 1] + vals[mid])


def _window_offsets(time_read, length, fps):
    """Where to look: up to MAX_WINDOWS spans spread across the part of the
    file that is actually playing, plus the peak. A bed's tail and a long
    silence would otherwise dominate the average spectrum."""
    last = max(0, length - WIN)
    peak_off = int(min(last, time_read["peak_at"] * fps * HOP))
    floor = max(time_read["env_db"]) - 40.0
    active = [i for i, db in enumerate(time_read["env_db"]) if db >= floor]
    if not active:
        active = [0]
    span = len(active)
    count = max(1, min(MAX_WINDOWS, span))
    offsets = []
    for k in range(count):
        frame = active[min(span - 1, int(k * span / count))]
        offsets.append(int(min(last, frame * HOP)))
    offsets.append(peak_off)
    return sorted(set(offsets))


def _power(mags):
    return [m * m for m in mags]


def _bands_of(power, rate, size):
    per_bin = rate / size
    total = 0.0
    out = {}
    for name, lo, hi in BANDS:
        a = max(0, int(lo / per_bin))
        b = min(len(power) - 1, int(hi / per_bin))
        energy = sum(power[a:b + 1])
        out[name] = energy
        total += energy
    if total <= 0:
        return {name: 0.0 for name, _, _ in BANDS}, 0.0
    return {name: round(value / total, 4) for name, value in out.items()}, total


def _centroid(power, rate, size, lo=40.0, hi=12000.0):
    per_bin = rate / size
    a = max(0, int(lo / per_bin))
    b = min(len(power) - 1, int(hi / per_bin))
    num = den = 0.0
    for k in range(a, b + 1):
        num += (k * per_bin) * power[k]
        den += power[k]
    return num / den if den > 0 else 0.0


def _rolloff(power, rate, size, fraction=0.85, hi=12000.0):
    per_bin = rate / size
    b = min(len(power) - 1, int(hi / per_bin))
    total = sum(power[:b + 1])
    if total <= 0:
        return 0.0
    seen = 0.0
    for k in range(b + 1):
        seen += power[k]
        if seen >= fraction * total:
            return k * per_bin
    return b * per_bin


def _flatness(power, rate, size, lo=100.0, hi=12000.0):
    """Geometric over arithmetic mean: 1.0 is white noise, near 0 is a tone.
    The single most useful number for "is this air or is this a note"."""
    per_bin = rate / size
    a = max(1, int(lo / per_bin))
    b = min(len(power) - 1, int(hi / per_bin))
    if b <= a:
        return 0.0
    floor = 1e-12
    logs = 0.0
    arithmetic = 0.0
    count = b - a + 1
    for k in range(a, b + 1):
        p = max(power[k], floor)
        logs += math.log(p)
        arithmetic += p
    if arithmetic <= 0:
        return 0.0
    geometric = math.exp(logs / count)
    return geometric / (arithmetic / count)


def _tilt(power, rate, size, lo=100.0, hi=10000.0):
    """Energy fall-off in dB per octave. A sub-heavy braam is steeply
    negative; bright air is close to flat."""
    per_bin = rate / size
    a = max(2, int(lo / per_bin))
    b = min(len(power) - 1, int(hi / per_bin))
    xs, ys = [], []
    for k in range(a, b + 1):
        if power[k] <= 0:
            continue
        xs.append(math.log2(k * per_bin))
        ys.append(10.0 * math.log10(power[k]))
    if len(xs) < 4:
        return 0.0
    return _slope(xs, ys)


def _f0(power, rate, size, lo=40.0, hi=1200.0):
    """The strongest partial in the bass/mid region: an honest 'note' for a
    tonal sound, and meaningless for noise - which the caller gates on
    flatness before believing it."""
    per_bin = rate / size
    a = max(1, int(lo / per_bin))
    b = min(len(power) - 1, int(hi / per_bin))
    if b <= a:
        return None
    best, at = -1.0, None
    for k in range(a, b + 1):
        if power[k] > best:
            best, at = power[k], k
    return None if at is None else at * per_bin


def _spectral_read(samples, rate, time_read):
    length = len(samples)
    offsets = _window_offsets(time_read, length, time_read["fps"])
    powers, centroids, f0s = [], [], []
    for off in offsets:
        mags = _vq._spectrum(samples, off, WIN)
        if not mags:
            continue
        power = _power(mags)
        powers.append(power)
        centroids.append(_centroid(power, rate, WIN))
        flat = _flatness(power, rate, WIN)
        if flat < 0.6:
            note = _f0(power, rate, WIN)
            if note:
                f0s.append(note)
    if not powers:
        return None
    mean_power = [value / len(powers) for value in map(sum, zip(*powers))]
    bands, _total = _bands_of(mean_power, rate, WIN)

    ratio = None
    direction = "no pitch"
    if len(f0s) >= 3:
        early = _median(f0s[: max(1, len(f0s) // 3)])
        late = _median(f0s[-max(1, len(f0s) // 3):])
        if early and late:
            ratio = late / early
            direction = ("rising" if ratio > 1.20 else
                         "falling" if ratio < 0.83 else "steady")
    # Spectral flux: how much the spectrum MOVES between consecutive windows,
    # half-wave rectified and normalised. It is the physical difference
    # between air in motion (a whoosh sweeps) and a hit that simply starts
    # and stops (a foley slap measures like a whoosh on every static
    # reading, because it is bright, short and mid-heavy too).
    flux = 0.0
    if len(powers) >= 3:
        moves = []
        for before, after in zip(powers, powers[1:]):
            total = sum(after) + 1e-12
            moves.append(sum(max(0.0, b - a) for a, b in zip(before, after)) / total)
        flux = sum(moves) / len(moves)
    drift = 0.0
    if len(centroids) >= 3:
        early = _median(centroids[: max(1, len(centroids) // 3)]) or 0.0
        late = _median(centroids[-max(1, len(centroids) // 3):]) or 0.0
        drift = late - early
    return {
        "centroid_hz": _centroid(mean_power, rate, WIN),
        "rolloff_hz": _rolloff(mean_power, rate, WIN),
        "bands": bands,
        "flatness": _flatness(mean_power, rate, WIN),
        "tilt_db_oct": _tilt(mean_power, rate, WIN),
        "pitch_hz": _median(f0s) or 0.0,
        "pitch_ratio": ratio or 0.0,
        "pitch_direction": direction,
        "centroid_drift_hz": drift,
        "flux": flux,
        "windows": len(powers),
    }


# ---------------------------------------------------------------------------
# 3. One file, one read
# ---------------------------------------------------------------------------


def _read(spec, path):
    """The whole measured picture of one sound, or (None, reason)."""
    samples, rate = _decode(spec, path)
    if samples is None or not len(samples):
        return None, f"could not decode {path} (is it a file ffmpeg reads?)"
    truncated = False
    if len(samples) / rate > TRUNCATE_SEC:
        samples = samples[:int(TRUNCATE_SEC * rate)]
        truncated = True
    if len(samples) < WIN:
        # A 30 ms UI tick is shorter than one FFT window: pad with silence so
        # the spectrum is of the whole sound rather than of nothing, and say
        # so, because a padded read's flatness is not comparable to a full one.
        samples = samples + array.array("h", bytes((WIN - len(samples)) * 2))
        padded = True
    else:
        padded = False
    timing = _time_read(samples, rate)
    if timing is None:
        return None, f"{path} decoded to nothing measurable"
    spectral = _spectral_read(samples, rate, timing)
    if spectral is None:
        return None, f"{path} was too short to measure a spectrum"
    read = {
        "duration": timing["duration"],
        "peakDbfs": timing["peak_dbfs"],
        "rmsDbfs": timing["rms_dbfs"],
        "crestDb": timing["crest_db"],
        "peakAt": timing["peak_at"],
        "peakFraction": timing["peak_frac"],
        "attackMs": timing["attack_ms"],
        "bodySec": timing["body_s"],
        "decaySec": timing["decay_s"],
        "transients": timing["transients"],
        "transientTimes": timing["transient_times"],
        "energySlopeDbPerSec": timing["energy_slope_db_s"],
        "slopeFullDbPerSec": timing["slope_full_db_s"],
        "gainDb": timing["gain_db"],
        "steadyRise": timing["steady_rise"],
        "centroidHz": spectral["centroid_hz"],
        "rolloffHz": spectral["rolloff_hz"],
        "bands": spectral["bands"],
        "flatness": spectral["flatness"],
        "tiltDbPerOctave": spectral["tilt_db_oct"],
        "pitchHz": spectral["pitch_hz"],
        "pitchRatio": spectral["pitch_ratio"],
        "pitchDirection": spectral["pitch_direction"],
        "centroidDriftHz": spectral["centroid_drift_hz"],
        "flux": spectral["flux"],
        "windows": spectral["windows"],
        "truncated": truncated,
        "padded": padded,
    }
    read["roles"], read["role"], read["confidence"], read["evidence"] = _roles(read)
    return read, None


# ---------------------------------------------------------------------------
# 4. The role read: what this sound is FOR, scored from the measurements
# ---------------------------------------------------------------------------
#
# Six roles, each a weighted sum of normalised evidence - never a lookup on
# a filename or a tag. The weights say which measurements matter to that
# job: a transition is bright, airy and MOVING (spectral flux), held for
# about a third of a second; a reveal is heavy, sustained and tonal or
# sweeping; an impact is heavy and sharp-edged and does not hold; a gentle
# entrance is soft, tonal, still and small; a texture is tonal, sustained
# and gentle; a build climbs in level, steadily, and holds. The evidence dict
# returned beside the scores is the part a Director reads: a score with no
# number behind it is taste, and taste is what this module refuses to fake.


def _roles(read):
    """The six jobs, as fits from measured evidence.

    The normalisation ranges are calibrated against the 530 shipped sounds
    (percentiles of the shelf's own categories, in `docs/SOUND-SENSE.md`),
    not against guesses: the first pass used ranges picked from synthetic
    fixtures, under which every whoosh in the library read as a gentle
    entrance and a foley slap outranked both. They are still physical
    ranges, so a user's own import lands somewhere sensible rather than
    off the end of the scale.
    """
    bands = read["bands"]
    heavy = _n(bands["sub"] + bands["low"], 0.25, 0.75)
    brightness = _n(read["centroidHz"], 400.0, 3000.0)
    airy = _n(bands["air"] + bands["presence"], 0.02, 0.35)
    noisy = _n(read["flatness"], 0.0, 0.30)
    tonal = 1.0 - noisy
    short = 1.0 - _n(read["bodySec"], 0.15, 1.00)
    # A transition moves air for a moment: 16 ms of click is not a whoosh and
    # 3 s of wash is not one either, so this peaks near a third of a second
    # and falls away on both sides - the one place a plain "short" cannot
    # tell a whip from a UI tick.
    transition_length = 1.0 - min(1.0, abs(math.log(max(read["bodySec"], 0.005) / 0.35))
                                  / math.log(6.0))
    sustain = _n(read["bodySec"], 0.60, 2.50)
    fast = 1.0 - _n(read["attackMs"], 5.0, 90.0)
    sharp = _n(read["crestDb"], 8.0, 20.0)
    soft = 1.0 - _n(read["crestDb"], 6.0, 16.0)
    rise = (_n(read["gainDb"], 2.0, 10.0) * _n(read["steadyRise"], 0.45, 0.80))
    sweep = _n(abs(read["centroidDriftHz"]), 300.0, 2500.0)
    flux = _n(read["flux"], 0.03, 0.45)

    # Five of the jobs need a property the weights alone would let the others
    # borrow: a transition MOVES AIR, an impact and a reveal are HEAVY, a
    # gentle entrance is NOT heavy and does not move. Without these gates a
    # 0.55 s sub thud scored 0.44 as a transition on its attack and length
    # alone - every measurement right, the answer wrong - which is the kind
    # of plausible number this module exists to refuse.
    air_gate = 0.35 + 0.65 * max(brightness, airy)
    soft_gate = 1.0 - 0.70 * heavy
    held_gate = 0.40 + 0.60 * sustain
    impact_gate = 0.25 + 0.75 * heavy
    reveal_gate = 0.45 + 0.55 * heavy
    still_gate = 1.0 - flux
    build_gate = 0.25 + 0.75 * rise
    evidence = {
        "heavy": _r(heavy),
        "bright": _r(brightness),
        "airy": _r(airy),
        "noisy": _r(noisy),
        "short": _r(short),
        "transitionLength": _r(transition_length),
        "sustained": _r(sustain),
        "fastAttack": _r(fast),
        "sharp": _r(sharp),
        "soft": _r(soft),
        "climbs": _r(rise),
        "gainsDb": _r(read["gainDb"]),
        "steadyRise": _r(read["steadyRise"]),
        # Reported, never scored: the dominant partial jumps an octave
        # whenever a stronger harmonic takes over (a power-up reads 16x), so
        # it says "something moved up", not "the pitch rose".
        "dominantPartialRatio": _r(read["pitchRatio"]),
        "sweeps": _r(sweep),
        "flux": _r(flux),
        "airGate": _r(air_gate),
        "impactGate": _r(impact_gate),
        "revealGate": _r(reveal_gate),
        "heldGate": _r(held_gate),
        "buildGate": _r(build_gate),
    }
    scores = {
        "quick_transition": air_gate * (0.28 * brightness + 0.24 * transition_length
                                        + 0.28 * flux + 0.20 * airy),
        "cinematic_reveal": reveal_gate * (0.34 * heavy + 0.28 * sustain
                                           + 0.22 * max(sweep, tonal)
                                           + 0.16 * (1.0 - noisy)),
        "action_impact": impact_gate * (0.40 * heavy + 0.32 * sharp + 0.28 * short),
        "gentle_entrance": soft_gate * still_gate * (0.30 * tonal + 0.25 * short
                                                     + 0.25 * soft + 0.20 * airy),
        "emotional_texture": held_gate * (0.34 * tonal + 0.28 * sustain
                                          + 0.22 * soft + 0.16 * (1.0 - heavy)),
        "tension_build": build_gate * (0.45 * rise + 0.30 * sustain
                                       + 0.25 * (1.0 - short)),
    }
    scores = {name: _r(_clamp(value)) for name, value in scores.items()}
    ordered = sorted(scores.items(), key=lambda kv: (-kv[1], kv[0]))
    top, best = ordered[0]
    margin = best - ordered[1][1]
    confidence = _r(_clamp(0.45 + 2.0 * margin, 0.45, 0.95))
    return scores, top, confidence, evidence


# ---------------------------------------------------------------------------
# The tools
# ---------------------------------------------------------------------------


def _num(source, key, default=0.0):
    value = source.get(key)
    try:
        number = float(value)
    except (TypeError, ValueError):
        return default
    return number if math.isfinite(number) else default


def _role_name(word):
    """A role from its own name or from the words a caller would use for it.
    Returns (role, error)."""
    text = str(word or "").strip().lower().replace("-", "_").replace(" ", "_")
    if text in ROLES:
        return text, None
    hits = []
    for role, words in ROLE_WORDS.items():
        if any(w in text for w in words):
            hits.append(role)
    if len(set(hits)) == 1:
        return hits[0], None
    if hits:
        return None, (f"{word!r} names more than one role ({sorted(set(hits))}): "
                      f"say which, or use one of {list(ROLES)}")
    return None, f"{word!r} is not a sound role: use one of {list(ROLES)}"


def describe_sound(spec):
    path = spec.get("path") or spec.get("audio")
    if not path:
        return {"ok": False, "error": "`path` is the sound file to measure"}
    read, error = _read(spec, path)
    if error:
        return {"ok": False, "error": error}
    out = {
        "ok": True,
        "schema": SCHEMA,
        "tool": "describe_sound",
        "path": str(path),
        "rate": RATE,
        "measured": read,
        "note": ("roles are measured fits, not labels: the evidence beside them is what "
                 "the score was made of, and a low confidence means the file sits "
                 "between two jobs rather than in one"),
    }
    if read["truncated"]:
        out["note"] += f" (only the first {int(TRUNCATE_SEC)} s were measured)"
    return out


def _library_entries(root):
    """Every audio file in a folder with the manifest entry that names it,
    when there is a manifest beside it."""
    root = Path(str(root))
    listed = {}
    manifest = root / "manifest.json"
    if manifest.is_file():
        try:
            data = json.loads(manifest.read_text())
            for entry in data.get("sounds", []):
                if entry.get("file"):
                    listed[str(entry["file"])] = entry
        except (ValueError, OSError):
            listed = {}
    entries = []
    for path in sorted(root.iterdir()) if root.is_dir() else []:
        if path.suffix.lower() not in (".ogg", ".mp3", ".m4a", ".flac", ".wav", ".opus"):
            continue
        entries.append((path, listed.get(path.name, {})))
    return entries


def rank_sounds(spec):
    role, error = _role_name(spec.get("role"))
    if error:
        return {"ok": False, "error": error}
    limit = max(1, int(_num(spec, "limit", 5)))
    shortest = _num(spec, "maxDuration", 0.0)
    longest = _num(spec, "minDuration", 0.0)

    paths = []
    for item in spec.get("paths") or []:
        if isinstance(item, str):
            paths.append((item, {}))
        elif isinstance(item, dict) and item.get("path"):
            paths.append((item["path"], item))
    if spec.get("dir"):
        paths.extend(_library_entries(spec["dir"]))
    if not paths:
        return {"ok": False, "error": "name the candidates: `paths` (files or {path,...}) or `dir`"}

    scored, skipped = [], []
    for path, listed in paths:
        try:
            read, why = _read(spec, path)
        except Exception as exc:  # noqa: BLE001
            read, why = None, f"{type(exc).__name__}: {exc}"
        if why:
            skipped.append({"path": str(path), "reason": why})
            continue
        if shortest and read["duration"] > shortest:
            skipped.append({"path": str(path), "reason": f"{read['duration']:.2f} s is longer than maxDuration {shortest}"})
            continue
        if longest and read["duration"] < longest:
            skipped.append({"path": str(path), "reason": f"{read['duration']:.2f} s is shorter than minDuration {longest}"})
            continue
        fit = read["roles"][role]
        # The evidence that earned the score, strongest first: what the
        # Director reads instead of trusting a number.
        drivers = sorted(read["evidence"].items(), key=lambda kv: -kv[1])[:4]
        scored.append({
            "path": str(path),
            "id": listed.get("id"),
            "name": listed.get("name") or Path(str(path)).stem,
            "fit": fit,
            "role": read["role"],
            "confidence": read["confidence"],
            "duration": _r(read["duration"], 3),
            "measuredPeakAt": _r(read["peakAt"], 4),
            "manifestSyncAt": listed.get("syncAt"),
            "why": [f"{name} {value:.2f}" for name, value in drivers],
            "measured": {k: read[k] for k in (
                "centroidHz", "flatness", "crestDb", "attackMs", "bodySec",
                "transients", "bands", "pitchDirection", "energySlopeDbPerSec")},
        })
    scored.sort(key=lambda hit: (-hit["fit"], hit["name"]))
    return {
        "ok": True,
        "schema": SCHEMA,
        "tool": "rank_sounds",
        "role": role,
        "candidates": scored[:limit],
        "considered": len(scored),
        "skipped": skipped[:12],
        "note": ("ranked by measured fit for the asking role, not by what the manifest "
                 "says it is for; `why` names the measurements that carried the score"),
    }


def _fix(spec, tool, arguments, needs=(), alias=None):
    """One edit, as arguments the caller can run.

    A fix may only contain fields the measurement determines. Which clip it
    is, and in which project, are the caller's facts, so they are copied in
    when the spec carries them and named in `needs` when it does not - a fix
    that invented a clip id would be exactly the kind of confident lie this
    module exists to refuse.
    """
    args = dict(arguments)
    alias = alias or {}
    missing = []
    for key in needs:
        value = spec.get(key)
        if isinstance(value, str) and value.strip():
            args[alias.get(key, key)] = value
        else:
            missing.append(key)
    out = {"tool": tool, "arguments": args}
    if missing:
        out["needs"] = missing
    return out


def check_placement(spec):
    path = spec.get("path") or spec.get("audio")
    if not path:
        return {"ok": False, "error": "`path` is the placed sound file"}
    read, error = _read(spec, path)
    if error:
        return {"ok": False, "error": error}
    role, role_error = _role_name(spec.get("role") or read["role"])
    if role_error:
        return {"ok": False, "error": role_error}

    clip_start = _num(spec, "clipStart", 0.0)
    intended = _num(spec, "intendedAt", clip_start + read["peakAt"])
    tolerance = _num(spec, "toleranceMs", 40.0) / 1000.0
    gain = _num(spec, "gainDb", 0.0)
    intended_offset = intended - clip_start
    move_by = intended_offset - read["peakAt"]

    issues = []
    if abs(move_by) > tolerance:
        issues.append({
            "code": "sync_off",
            "severity": "fix",
            "evidence": (f"the intended moment sits {intended_offset:.3f} s into the clip and "
                         f"the file's peak measures at {read['peakAt']:.3f} s: "
                         f"{abs(move_by) * 1000:.0f} ms {'late' if move_by < 0 else 'early'}"),
            "fix": _fix(spec, "edit_apply", {
                "command": {"op": "moveClips", "moves": [dict(
                    {"start": _r(max(0.0, clip_start + move_by), 3)},
                    **{k: spec[k] for k in ("clipId", "trackId")
                       if isinstance(spec.get(k), str) and spec[k].strip()},
                )]},
            }, needs=("project", "clipId", "trackId"), alias={"project": "path"}),
        })

    fit = read["roles"][role]
    if fit < 0.35:
        issues.append({
            "code": "wrong_sound",
            "severity": "fix",
            "evidence": (f"placed as {role}, but it measures as {read['role']} "
                         f"(fit {fit:.2f} for {role}, confidence {read['confidence']:.2f})"),
            "fix": {
                "tool": "sound_sense rank_sounds",
                "arguments": {"role": role,
                              "maxDuration": _r(max(1.0, read["duration"] * 2), 2)},
                "needs": ["paths (sounds_search candidates) or dir"],
                "why": [f"{name} {value:.2f}" for name, value in
                        sorted(read["evidence"].items(), key=lambda kv: -kv[1])[:3]],
            },
        })

    voice_peak = spec.get("voicePeakDbfs")
    estimated = False
    if voice_peak is None and spec.get("voiceLufs") is not None:
        # Integrated loudness is not a peak: +6 dB is the honest worst case
        # for a limited voice take, and the reading says it is an estimate.
        voice_peak = _num(spec, "voiceLufs", -23.0) + 6.0
        estimated = True
    if voice_peak is not None:
        accent_peak = read["peakDbfs"] + gain
        ceiling = float(voice_peak) + 6.0
        if accent_peak > ceiling:
            issues.append({
                "code": "too_loud",
                "severity": "fix",
                "evidence": (f"at {gain:+.1f} dB this accent peaks {accent_peak:.1f} dBFS "
                             f"against a voice peak of {float(voice_peak):.1f} dBFS"
                             + (" (estimated from integrated loudness)" if estimated else "")),
                "fix": _fix(spec, "clip_audio_levels_db", {
                    "baseDb": _r(_clamp(gain - (accent_peak - ceiling), -60.0, 24.0), 1),
                }, needs=("project", "clipId"), alias={"project": "path"}),
            })

    recent = spec.get("recent") or []
    window = _num(spec, "repetitionWindowSec", 12.0)
    allowed = max(1, int(_num(spec, "maxSameRole", 3)))
    same = [entry for entry in recent
            if isinstance(entry, dict)
            and str(entry.get("role")) == role
            and abs(_num(entry, "at", 0.0) - intended) <= window]
    if len(same) >= allowed:
        # The oldest of the crowd is the one to lose: an accent three hits
        # back is the one the viewer has already stopped hearing.
        doomed = [entry["clipId"] for entry in same
                  if isinstance(entry.get("clipId"), str)][: len(same) - allowed + 1]
        drop = _fix(spec, "clips_remove", {"clipIds": doomed},
                    needs=("project",), alias={"project": "path"})
        if len(doomed) < len(same) - allowed + 1:
            drop["needs"] = drop.get("needs", []) + ["clipId on each recent entry"]
        drop["at"] = [entry.get("at") for entry in same]
        drop["keep"] = allowed
        issues.append({
            "code": "overused",
            "severity": "fix",
            "evidence": (f"{len(same) + 1} {role} accents inside {window:g} s: "
                         "the fourth hit in a short window reads as noise, not emphasis"),
            "fix": drop,
        })

    return {
        "ok": True,
        "schema": SCHEMA,
        "tool": "check_placement",
        "path": str(path),
        "role": role,
        "verdict": "as directed" if not issues else f"needs correction: {len(issues)} issue(s)",
        "issues": issues,
        "measured": {
            "peakAt": _r(read["peakAt"], 4),
            "intendedOffset": _r(intended_offset, 4),
            "moveBy": _r(move_by, 4),
            "roleFit": fit,
            "readsAs": read["role"],
            "peakDbfs": _r(read["peakDbfs"], 2),
        },
        "note": ("an issue is a measurement plus the edit that answers it; nothing here "
                 "claims a fix happened - the caller runs the fix and measures again"),
    }


def index_library(spec):
    root = spec.get("dir")
    if not root:
        return {"ok": False, "error": "`dir` is the folder of sounds to index"}
    root = Path(str(root))
    if not root.is_dir():
        return {"ok": False, "error": f"{root} is not a folder"}
    out_path = Path(str(spec.get("out") or (root / "sound-index.json")))
    previous = {}
    if out_path.is_file():
        try:
            data = json.loads(out_path.read_text())
            for entry in (data.get("sounds", [])
                          if data.get("version") == INDEX_VERSION else []):
                previous[entry.get("file")] = entry
        except (ValueError, OSError):
            previous = {}

    limit = int(_num(spec, "limit", 0.0)) or None
    started = time.time()
    sounds, failed, reused, measured = [], [], 0, 0
    for path, listed in _library_entries(root):
        stat = path.stat()
        stamp = {"size": stat.st_size, "mtime": int(stat.st_mtime)}
        known = previous.get(path.name)
        if known and known.get("stamp") == stamp:
            sounds.append(known)
            reused += 1
            continue
        if limit is not None and measured >= limit:
            break
        _PCM.clear()
        try:
            read, why = _read(spec, path)
        except Exception as exc:  # noqa: BLE001
            read, why = None, f"{type(exc).__name__}: {exc}"
        if why:
            failed.append({"file": path.name, "reason": why})
            continue
        drift = None
        if isinstance(listed.get("syncAt"), (int, float)):
            drift = _r(read["peakAt"] - float(listed["syncAt"]), 4)
        sounds.append({
            "id": listed.get("id") or path.stem,
            "file": path.name,
            "stamp": stamp,
            "duration": _r(read["duration"], 3),
            "role": read["role"],
            "confidence": read["confidence"],
            "roles": read["roles"],
            "measuredPeakAt": _r(read["peakAt"], 4),
            "manifestSyncAt": listed.get("syncAt"),
            "syncDriftSec": drift,
            "measured": {k: read[k] for k in (
                "peakDbfs", "crestDb", "attackMs", "bodySec", "transients",
                "centroidHz", "flatness", "bands", "pitchDirection",
                "energySlopeDbPerSec", "centroidDriftHz", "flux")},
        })
        measured += 1

    drift = [{"file": sound["file"], "manifestSyncAt": sound["manifestSyncAt"],
              "measuredPeakAt": sound["measuredPeakAt"], "driftSec": sound["syncDriftSec"]}
             for sound in sounds
             if sound.get("syncDriftSec") is not None and abs(sound["syncDriftSec"]) > 0.05]
    histogram = {}
    for sound in sounds:
        histogram[sound["role"]] = histogram.get(sound["role"], 0) + 1
    index = {
        "version": INDEX_VERSION,
        "schema": SCHEMA,
        "rate": RATE,
        "generatedAt": int(time.time()),
        "sounds": sounds,
    }
    try:
        out_path.write_text(json.dumps(index, indent=1))
        written = str(out_path)
    except OSError as exc:
        written = f"not written: {exc}"
    return {
        "ok": not failed,
        "schema": SCHEMA,
        "tool": "index_library",
        "index": written,
        "count": len(sounds),
        "measured": measured,
        "reused": reused,
        "failed": failed[:12],
        "roles": histogram,
        "syncDrift": drift[:24],
        "elapsedSec": _r(time.time() - started, 2),
        "note": ("one measurement per file, cached by size and mtime: the second run reads "
                 "nothing twice, and a sound placed again is never re-analysed on the "
                 "playback path"),
    }


# ---------------------------------------------------------------------------
# The selftest: synthetic sounds whose answer is known before measuring
# ---------------------------------------------------------------------------
#
# Five signals, each built from a formula so the expected read is arithmetic
# rather than opinion: air with a fast attack (a transition), a decaying
# sub tone (an impact/reveal), a rising tone (a build), an impulse (a click,
# and the peak-position test) and a slow swelling triad (a texture). No
# fixtures, no ffmpeg, no network.


def _write_wav(path, samples, rate=RATE):
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes(array.array("h", samples).tobytes())


def _clamp16(value):
    return int(max(-32767, min(32767, value)))


def _whoosh(duration=0.75):
    total = int(duration * RATE)
    seed, prev = 12345, 0.0
    out = []
    for i in range(total):
        t = i / RATE
        env = (1.0 - math.exp(-t / 0.02)) * math.exp(-t / 0.16)
        seed = (1103515245 * seed + 12345) % 2147483648
        noise = seed / 1073741824.0 - 1.0
        bright = noise - prev  # first difference: a gentle high-pass
        prev = noise
        out.append(_clamp16(bright * env * 9000.0))
    return out


def _boom(duration=0.55):
    total = int(duration * RATE)
    out, phase = [], 0.0
    for i in range(total):
        t = i / RATE
        phase += 2.0 * math.pi * (58.0 - 8.0 * (t / duration)) / RATE
        env = (1.0 - math.exp(-t / 0.004)) * math.exp(-t / 0.12)
        out.append(_clamp16(math.sin(phase) * env * 30000.0))
    return out


def _reveal(duration=2.4):
    """A deep sustained tone with a noise layer opening upward: the shape of
    a cinematic reveal - heavy, held, brightening - as opposed to the short
    heavy hit an impact scene wants."""
    total = int(duration * RATE)
    seed, prev = 99991, 0.0
    out = []
    for i in range(total):
        t = i / RATE
        body = math.sin(2.0 * math.pi * 58.0 * t)
        seed = (1103515245 * seed + 12345) % 2147483648
        noise = seed / 1073741824.0 - 1.0
        bright = noise - prev
        prev = noise
        opening = (t / duration) ** 2
        env = min(1.0, t / 0.25)
        out.append(_clamp16((body * 0.55 + bright * 0.30 * opening) * env * 26000.0))
    return out


def _riser(duration=2.6):
    total = int(duration * RATE)
    out, phase = [], 0.0
    for i in range(total):
        t = i / RATE
        hz = 180.0 * (1800.0 / 180.0) ** (t / duration)
        phase += 2.0 * math.pi * hz / RATE
        env = 10.0 ** (-30.0 / 20.0 * (1.0 - t / duration))
        out.append(_clamp16(math.sin(phase) * env * 30000.0))
    return out


def _click(duration=1.0, at=0.30):
    total = int(duration * RATE)
    out = [0] * total
    out[int(at * RATE)] = 32767
    return out


def _pad(duration=3.0):
    total = int(duration * RATE)
    out = []
    for i in range(total):
        t = i / RATE
        env = math.sin(math.pi * t / duration) ** 1.5
        value = (math.sin(2 * math.pi * 220.0 * t)
                 + math.sin(2 * math.pi * 277.2 * t)
                 + math.sin(2 * math.pi * 329.6 * t))
        out.append(_clamp16(value / 3.0 * env * 26000.0))
    return out


def selftest():
    checks = []

    def check(name, ok, error=None, extra=None):
        checks.append({"tool": name, "ok": bool(ok), "error": None if ok else error,
                       "extra": extra})

    with tempfile.TemporaryDirectory(prefix="sound-sense-") as scratch:
        root = Path(scratch)
        files = {
            "whoosh": root / "whoosh.wav",
            "boom": root / "boom.wav",
            "riser": root / "riser.wav",
            "click": root / "click.wav",
            "pad": root / "pad.wav",
            "reveal": root / "reveal.wav",
        }
        _write_wav(files["whoosh"], _whoosh())
        _write_wav(files["boom"], _boom())
        _write_wav(files["riser"], _riser())
        _write_wav(files["click"], _click())
        _write_wav(files["pad"], _pad())
        _write_wav(files["reveal"], _reveal())

        reads = {}
        for name, path in files.items():
            read, why = _read({}, path)
            reads[name] = read
            check(f"describe {name}", read is not None, why)

        expected = {"whoosh": "quick_transition", "boom": "action_impact",
                    "riser": "tension_build", "pad": "emotional_texture",
                    "reveal": "cinematic_reveal"}
        for name, role in expected.items():
            read = reads.get(name) or {}
            check(f"{name} reads as {role}", read.get("role") == role,
                  f"measured {read.get('role')} ({read.get('roles')})",
                  f"confidence {read.get('confidence')}")

        click = reads.get("click") or {}
        peak = click.get("peakAt")
        check("an impulse's peak is measured where it is",
              peak is not None and abs(peak - 0.30) <= 0.025,
              f"measured {peak} s for an impulse at 0.300 s")

        read_again, _ = _read({}, files["whoosh"])
        check("two reads of one file agree",
              _PCM and read_again == reads["whoosh"],
              "the second read differed from the first")

        every = [str(p) for p in files.values()]
        ranked = rank_sounds({"role": "quick transition", "paths": every, "limit": 6})
        order = [Path(hit["path"]).stem for hit in ranked.get("candidates", [])]
        check("a transition shops the air first, not the impulse",
              ranked.get("ok") and order and order[0] == "whoosh",
              f"order was {order}")
        check("the heavy hits are not offered as transitions",
              set(order[:2]) == {"whoosh", "click"}
              and {"boom", "reveal"}.issubset(set(order[2:])),
              f"order was {order}")

        ranked = rank_sounds({"role": "action impact", "paths": every, "limit": 6})
        order = [Path(hit["path"]).stem for hit in ranked.get("candidates", [])]
        check("an impact shops the heavy hit first",
              ranked.get("ok") and order and order[0] == "boom", f"order was {order}")

        late = check_placement({
            "path": str(files["click"]), "role": "quick_transition",
            "clipStart": 5.0, "intendedAt": 5.1,
        })
        codes = [issue["code"] for issue in late.get("issues", [])]
        move = (late.get("measured") or {}).get("moveBy")
        check("a peak 200 ms late is reported with the move that fixes it",
              "sync_off" in codes and move is not None and abs(move + 0.2) <= 0.03,
              f"codes {codes}, moveBy {move}")

        full = check_placement({
            "path": str(files["click"]), "role": "quick_transition",
            "clipStart": 5.0, "intendedAt": 5.1,
            "project": "/tmp/p.concut", "clipId": "c7", "trackId": "a1",
        })
        move_fix = next((i["fix"] for i in full.get("issues", [])
                         if i["code"] == "sync_off"), {})
        move = ((move_fix.get("arguments") or {}).get("command") or {}).get("moves") or [{}]
        check("with the clip named, the sync fix is one runnable edit",
              move_fix.get("needs") is None
              and (move_fix.get("arguments") or {}).get("path") == "/tmp/p.concut"
              and move[0].get("clipId") == "c7" and move[0].get("trackId") == "a1"
              and abs(move[0].get("start", 0) - 4.8) <= 0.03,
              f"fix was {move_fix}")

        wrong = check_placement({
            "path": str(files["boom"]), "role": "quick_transition",
            "clipStart": 0.0, "intendedAt": 0.05,
        })
        codes = [issue["code"] for issue in wrong.get("issues", [])]
        check("a sub boom placed as a whoosh is named as the wrong sound",
              "wrong_sound" in codes, f"codes {codes}")

        loud = check_placement({
            "path": str(files["click"]), "role": "quick_transition",
            "clipStart": 0.0, "intendedAt": 0.30, "gainDb": 6.0,
            "voicePeakDbfs": -20.0,
        })
        issues = {issue["code"]: issue for issue in loud.get("issues", [])}
        suggested = ((issues.get("too_loud", {}).get("fix") or {}).get("arguments") or {}).get("baseDb")
        check("an accent six dB over the voice is levelled down",
              "too_loud" in issues and suggested is not None and suggested < 6.0,
              f"codes {sorted(issues)}, suggested {suggested}")

        many = check_placement({
            "path": str(files["whoosh"]), "role": "quick_transition",
            "clipStart": 9.95, "intendedAt": 10.0,
            "recent": [{"role": "quick_transition", "at": 4.0},
                       {"role": "quick_transition", "at": 6.5},
                       {"role": "quick_transition", "at": 8.0}],
        })
        codes = [issue["code"] for issue in many.get("issues", [])]
        check("a fourth transition in ten seconds is called overused",
              "overused" in codes, f"codes {codes}")

        manifest = {"version": 1, "sounds": [
            {"id": "click", "name": "Click", "file": "click.wav", "syncAt": 0.50},
            {"id": "pad", "name": "Pad", "file": "pad.wav", "syncAt": 1.50},
        ]}
        (root / "manifest.json").write_text(json.dumps(manifest))
        first = index_library({"dir": str(root), "out": str(root / "index.json")})
        second = index_library({"dir": str(root), "out": str(root / "index.json")})
        drift = {entry["file"]: entry["driftSec"] for entry in first.get("syncDrift", [])}
        check("the index measures every file and reports a dishonest syncAt",
              first.get("ok") and first.get("count") == 6
              and drift.get("click.wav") is not None
              and abs(drift["click.wav"] + 0.2) <= 0.03,
              f"count {first.get('count')}, drift {drift}")
        check("a second index run re-measures nothing",
              second.get("reused") == 6 and second.get("measured") == 0,
              f"reused {second.get('reused')}, measured {second.get('measured')}")

        bad = describe_sound({"path": str(root / "nope.wav")})
        check("a missing file is an error with a reason, not a zero reading",
              bad.get("ok") is False and "could not decode" in str(bad.get("error")),
              f"answered {bad}")

    failed = [entry for entry in checks if not entry["ok"]]
    return {
        "ok": not failed,
        "schema": SCHEMA,
        "action": "selftest",
        "checks": checks,
        "failed": failed,
        "error": (f"{len(failed)} check(s) failed: "
                  + "; ".join(f"{c['tool']}: {c.get('error')}" for c in failed))
        if failed else None,
    }


TOOLS = {
    "describe_sound": describe_sound,
    "rank_sounds": rank_sounds,
    "check_placement": check_placement,
    "index_library": index_library,
}


def command(spec):
    action = str(spec.get("action") or "analyse")
    try:
        if action == "selftest":
            return selftest()
        tool = str(spec.get("tool") or "")
        handler = TOOLS.get(tool)
        if handler is None:
            return {"ok": False,
                    "error": f"unknown tool {tool!r}: one of {sorted(TOOLS)}"}
        return handler(spec)
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def main():
    parser = argparse.ArgumentParser(description="What a sound file actually is, measured.")
    parser.add_argument("cmd", choices=["analyse", "selftest"])
    parser.add_argument("--spec", default="{}")
    args = parser.parse_args()
    raw = sys.stdin.read() if args.spec == "-" else args.spec
    try:
        spec = json.loads(raw)
        if not isinstance(spec, dict):
            raise ValueError("spec must be a JSON object")
    except ValueError as exc:
        print(json.dumps({"ok": False, "error": f"bad spec: {exc}"}))
        sys.exit(2)
    spec.setdefault("action", args.cmd)
    out = command(spec)
    print(json.dumps(out, indent=1))
    sys.exit(0 if out.get("ok") else 4)


if __name__ == "__main__":
    main()
