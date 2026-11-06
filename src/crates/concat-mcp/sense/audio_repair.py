#!/usr/bin/env python3
"""concat-audio-repair: the take cleaned, levelled, and the music read.

Pure Python standard library, same contract as the other sense scripts:
`audio_repair.py <extract|selftest> --spec -` reads one JSON spec from
stdin and prints one JSON object on stdout. The spec names the tool in
`tool`.

THE SIX TOOLS THIS MODULE SERVES

  enhance_speech         the measured voice chain: highpass at the rumble,
                         de-esser, compressor, gate at the floor, limiter,
                         loudness to target - every decision measured from
                         THIS file, written to a new one
  remove_hiss_hum_rumble  find the hum (50/60 Hz + harmonics) and notch
                         it, cut the rumble, denoise the hiss - and prove
                         each removal with a before/after measurement
  normalize_loudness     two-pass ebur128 loudnorm to the platform target
                         (-14 streaming, -16 podcast, -23 broadcast), with
                         the honest before/after table
  detect_music_segments  music / speech / silence / mixed per span, the
                         beat strength and duty cycle that decide it, and
                         the editing answer: where BGM can go, where it
                         must duck, where it must not double
  identify_music         offline identification: tempo, key, mode,
                         instrumentation hints, energy, mood tags - the
                         honest answer that NAMING a song needs an online
                         fingerprint service, plus the descriptor string
                         that IS the music-library query
  analyze_music_structure  the bar grid, sections from self-similarity,
                         energy curve, the drops and the build - the map a
                         cut-to-music edit is drawn on

The three writing tools never touch the input: the output lands in `out`
(default: beside the input, with a suffix). Everything degrades loudly.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import sys
from pathlib import Path

try:
    import voice_signals as _vs
except ImportError:  # pragma: no cover
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import voice_signals as _vs

try:
    import voice_quality as _vq
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import voice_quality as _vq

SCHEMA = "funcut.audio-repair/1"
RATE = _vq.RATE

_CONTAINERS = (".mp4", ".mkv", ".mov", ".webm", ".m4v")


def _ok(tool, **fields):
    out = {"ok": True, "schema": SCHEMA, "tool": tool}
    out.update(fields)
    return out


def _probe_duration(ff, path):
    """Duration in seconds from ffmpeg's own banner read."""
    rc, _, err = _vs._run([ff, "-hide_banner", "-nostdin", "-i", str(path), "-f", "null", "-"], 120)
    m = re.search(r"Duration:\s*([0-9:.]+)", err.decode("utf-8", "replace"))
    if not m:
        return None
    parts = [float(p) for p in m.group(1).split(":")]
    return parts[0] * 3600 + parts[1] * 60 + parts[2]


def _run_ffmpeg(ff, args, timeout=480):
    rc, out, err = _vs._run([ff, "-hide_banner", "-nostdin", "-y"] + args, timeout)
    if rc != 0:
        tail = err.decode("utf-8", "replace").strip()[-400:]
        return False, tail
    return True, None


def _output_path(spec, source, suffix):
    """Where a writing tool lands its file: the caller's `out`, else beside
    the input. The input itself is never the target."""
    out = spec.get("out")
    if out:
        return str(out), bool(spec.get("overwrite"))
    p = Path(str(source))
    return str(p.with_name(p.stem + suffix + p.suffix)), False


def _measure_snr(spec, path):
    """Speech-ref minus floor, the same reading classify_recording_quality
    uses, so the before/after table is one instrument throughout."""
    samples, _ = _vq._pcm(spec, path)
    if samples is None or not len(samples):
        return None
    env = _vq._envelope(samples)
    win_frames = 25
    windows = []
    for i in range(0, len(env) - win_frames, win_frames):
        windows.append(max(env[i:i + win_frames]))
    windows.sort()
    if not windows:
        return None
    floor = _vq._db(windows[int(0.15 * len(windows))])
    speech = _vq._db(windows[int(0.9 * len(windows))] if len(windows) > 5 else windows[-1])
    return round(speech - floor, 1)


# ---------------------------------------------------------------------------
# 1. enhance_speech
# ---------------------------------------------------------------------------


def _lin(db):
    """A dBFS reading as the linear amplitude ffmpeg's dynamics filters
    take (acompressor, agate, alimiter all want 0..1, not dB)."""
    return min(1.0, max(0.001, 10 ** (db / 20.0)))


def _enhance_speech(spec):
    audio = spec.get("audio") or spec.get("path")
    if not audio or not Path(str(audio)).is_file():
        return {"ok": False, "error": f"no such audio file: {audio}"}
    ff, _ = _vs._ffmpeg(spec)
    samples, _ = _vq._pcm(spec, audio)
    if samples is None or not len(samples):
        return {"ok": False, "error": f"could not decode {audio}"}
    env = _vq._envelope(samples)

    # Every parameter measured from THIS take.
    win_frames = 25
    windows = []
    for i in range(0, len(env) - win_frames, win_frames):
        windows.append(max(env[i:i + win_frames]))
    windows.sort()
    floor_db = _vq._db(windows[int(0.15 * len(windows))]) if windows else -60.0
    peak_db = _vq._db(max(abs(s) for s in samples) / 32768.0)
    integrated, lra, tp = _vs._integrated(ff, audio)

    target_lufs = float(spec.get("targetLufs") or -16.0)
    target_tp = float(spec.get("targetTruePeakDbtp") or -1.5)
    # The gate sits just over the floor: it must not eat soft speech.
    gate_db = max(-70.0, floor_db + 8.0)
    # The compressor rides around the take's own level.
    speech_db = _vq._db(windows[int(0.75 * len(windows))] if len(windows) > 8 else (windows[-1] if windows else 0.1))
    threshold_db = speech_db - 6.0

    filters = [
        "highpass=f=75",                      # the rumble door
        "lowpass=f=14000",                    # the sibilance ceiling
        "deesser=i=0.5:m=0.5:f=0.55",         # measured sibilance tame
        f"acompressor=threshold={_lin(threshold_db):.4f}:ratio=2.5:attack=12:release=180:makeup=1.3",
        f"agate=threshold={_lin(gate_db):.4f}:ratio=2:attack=5:release=220:range=0.05",
        f"alimiter=limit={_lin(target_tp):.4f}:level=false",
        f"loudnorm=I={target_lufs}:TP={target_tp}:LRA=7",
    ]
    chain = ",".join(filters)

    out_path, overwrite = _output_path(spec, audio, "_enhanced")
    if Path(out_path).exists() and not overwrite:
        return {"ok": False, "error": f"{out_path} exists; pass overwrite: true or a new out"}
    is_video = Path(out_path).suffix.lower() in _CONTAINERS
    args = ["-i", str(audio)]
    if is_video:
        args += ["-map", "0:v", "-map", "0:a", "-c:v", "copy",
                 "-af", chain, "-c:a", "aac", "-b:a", "192k"]
    else:
        args += ["-af", chain, "-c:a", "pcm_s16le"]
    args += [str(out_path)]
    good, why = _run_ffmpeg(ff, args)
    if not good:
        return {"ok": False, "error": f"the enhance chain failed: {why}"}

    after_i, after_lra, after_tp = _vs._integrated(ff, out_path)
    snr_before = _measure_snr(spec, audio)
    snr_after = _measure_snr(spec, out_path)
    return _ok(
        "enhance_speech",
        file=out_path,
        before={"integratedLufs": integrated, "truePeakDbfs": tp, "snrDb": snr_before},
        after={"integratedLufs": after_i, "truePeakDbfs": after_tp, "snrDb": snr_after},
        chain=[
            {"filter": "highpass=75", "why": "voice lives above 75 Hz; the room lives below"},
            {"filter": "lowpass=14000", "why": "nothing a phone speaker can use above 14 kHz"},
            {"filter": "deesser", "why": "sibilance tamed before compression so it does not pump"},
            {"filter": f"acompressor@{threshold_db:.1f}dB", "why": "measured 6 dB under the take's speech level"},
            {"filter": f"agate@{gate_db:.1f}dB", "why": f"8 dB over the measured {floor_db:.1f} dBFS floor"},
            {"filter": "alimiter", "why": f"true-peak ceiling {target_tp} dBTP"},
            {"filter": f"loudnorm I={target_lufs}", "why": "the requested platform target"},
        ],
        duration=_probe_duration(ff, out_path),
        note="the input file was not touched",
        source="ffmpeg (measured filter chain) + ebur128 before/after",
    )


# ---------------------------------------------------------------------------
# 2. remove_hiss_hum_rumble
# ---------------------------------------------------------------------------


def _remove_hiss_hum_rumble(spec):
    audio = spec.get("audio") or spec.get("path")
    if not audio or not Path(str(audio)).is_file():
        return {"ok": False, "error": f"no such audio file: {audio}"}
    ff, _ = _vs._ffmpeg(spec)
    samples, _ = _vq._pcm(spec, audio)
    if samples is None or not len(samples):
        return {"ok": False, "error": f"could not decode {audio}"}
    env = _vq._envelope(samples)

    # Where the quiet lives: the windows to read the noise from.
    win_frames = 25
    windows = []
    for i in range(0, len(env) - win_frames, win_frames):
        windows.append((max(env[i:i + win_frames]), i))
    windows.sort(key=lambda w: w[0])
    quietest_idx = [i for _, i in windows[:max(3, len(windows) // 10)]]

    # The hum: FFT over the quietest window; 50/60 Hz families.
    hum_base = None
    hum_peaks = []
    for i in quietest_idx[:1]:
        mags = _vq._spectrum(samples, i * _vq.HOP)
        if not mags:
            continue
        per_bin = RATE / _vq.WIN
        for base in (50.0, 60.0):
            found = 0
            for harmonic in range(1, 5):
                k = int((base * harmonic) / per_bin)
                if k <= 1 or k >= len(mags) - 2:
                    continue
                around = (mags[k - 1] + mags[k + 1]) / 2 + 1e-9
                if mags[k] / around > 2.0:
                    found += 1
            if found >= 2:
                hum_base = base
                for harmonic in range(1, 5):
                    k = int((base * harmonic) / per_bin)
                    if 1 < k < len(mags) - 1:
                        hum_peaks.append(round(base * harmonic))
                break
        break

    # The hiss: high-band energy in the quiet windows vs the speech band.
    hiss_db = None
    highs, mids = [], []
    for i in quietest_idx[:4]:
        mags = _vq._spectrum(samples, i * _vq.HOP)
        if not mags:
            continue
        highs.append(_vq._band_energy(mags, 6000, 7500))
        mids.append(_vq._band_energy(mags, 300, 3000))
    if highs and mids:
        hiss_db = _vq._db(sum(highs) / sum(mids))

    # The rumble: sub-60 Hz energy in the loudest windows.
    rumble_db = None
    low, all_e = [], []
    for i in sorted(range(len(env)), key=lambda j: -env[j])[:4]:
        mags = _vq._spectrum(samples, i * _vq.HOP)
        if not mags:
            continue
        low.append(_vq._band_energy(mags, 20, 60))
        all_e.append(_vq._band_energy(mags, 20, 7500))
    if low and all_e:
        rumble_db = _vq._db(sum(low) / sum(all_e))

    filters = []
    findings = []
    if rumble_db is not None and rumble_db > -32:
        filters.append("highpass=f=70")
        findings.append({"found": "rumble", "measure": f"{rumble_db:.1f} dB of sub-60 Hz energy",
                         "fix": "highpass at 70 Hz"})
    else:
        filters.append("highpass=f=60")
        findings.append({"found": "no significant rumble", "measure": f"{rumble_db} dB",
                         "fix": "conservative 60 Hz highpass"})
    if hum_base:
        for hz in hum_peaks[:4]:
            filters.append(f"equalizer=frequency={hz}:t=q:w=8:g=-14")
        findings.append({"found": f"hum at {hum_base:.0f} Hz + harmonics",
                         "measure": f"{hum_peaks} Hz standing 6 dB over their neighbourhood",
                         "fix": "14 dB notches, Q 8"})
    if hiss_db is not None and hiss_db > -18:
        strength = min(24, max(12, int(-hiss_db)))
        filters.append(f"afftdn=nr={strength}:nf=-{max(30, int(-hiss_db) + 6)}")
        findings.append({"found": "hiss", "measure": f"high band {hiss_db:.1f} dB over the speech band in quiet",
                         "fix": f"afftdn nr={strength}"})
    else:
        findings.append({"found": "no significant hiss",
                         "measure": f"{hiss_db} dB" if hiss_db is not None else "n/a",
                         "fix": "nothing"})

    if len(filters) == 1 and filters[0] == "highpass=f=60":
        return _ok(
            "remove_hiss_hum_rumble",
            file=None,
            findings=findings,
            note="nothing worth removing: the take is cleaner than the cure "
                 "(denoise artefacts cost more than this noise). "
                 "Pass force: true to clean anyway.",
            forced=False,
            source="python (quiet-window FFT scan) + ffmpeg",
        ) if not spec.get("force") else _enhance_with(ff, spec, audio, filters, findings)

    return _enhance_with(ff, spec, audio, filters, findings)


def _enhance_with(ff, spec, audio, filters, findings):
    chain = ",".join(filters)
    out_path, overwrite = _output_path(spec, audio, "_clean")
    if Path(out_path).exists() and not overwrite:
        return {"ok": False, "error": f"{out_path} exists; pass overwrite: true or a new out"}
    is_video = Path(out_path).suffix.lower() in _CONTAINERS
    args = ["-i", str(audio)]
    if is_video:
        args += ["-map", "0:v", "-map", "0:a", "-c:v", "copy", "-af", chain,
                 "-c:a", "aac", "-b:a", "192k"]
    else:
        args += ["-af", chain, "-c:a", "pcm_s16le"]
    args += [str(out_path)]
    good, why = _run_ffmpeg(ff, args)
    if not good:
        return {"ok": False, "error": f"the cleanup chain failed: {why}"}
    snr_before = _measure_snr(spec, audio)
    snr_after = _measure_snr(spec, out_path)
    return _ok(
        "remove_hiss_hum_rumble",
        file=out_path,
        findings=findings,
        filterChain=filters,
        before={"snrDb": snr_before},
        after={"snrDb": snr_after},
        note="the input file was not touched",
        source="python (quiet-window FFT detection) + ffmpeg (notches, pass filters, afftdn)",
    )


# ---------------------------------------------------------------------------
# 3. normalize_loudness
# ---------------------------------------------------------------------------

_PLATFORMS = {
    "youtube": (-14.0, -1.0), "spotify": (-14.0, -1.0),
    "streaming": (-14.0, -1.0), "podcast": (-16.0, -1.5),
    "apple-podcast": (-16.0, -1.0), "broadcast-eu": (-23.0, -1.0),
    "broadcast-atsc": (-24.0, -2.0), "reels-tiktok": (-14.0, -1.0),
    "quiet": (-20.0, -2.0),
}


def _normalize_loudness(spec):
    audio = spec.get("audio") or spec.get("path")
    if not audio or not Path(str(audio)).is_file():
        return {"ok": False, "error": f"no such audio file: {audio}"}
    ff, _ = _vs._ffmpeg(spec)

    platform = str(spec.get("platform") or "").lower()
    if platform and platform in _PLATFORMS:
        target_i, target_tp = _PLATFORMS[platform]
    else:
        target_i = float(spec.get("targetLufs") or -14.0)
        target_tp = float(spec.get("targetTruePeakDbtp") or -1.0)

    # Pass one: what the take is now.
    integrated, lra, tp = _vs._integrated(ff, audio)
    if integrated is None:
        return {"ok": False, "error": "no audio stream to measure"}

    # Linear when the take is close enough for it; dynamic when it is not
    # (loudnorm's own rule: linear only when within range, and LRA 1-50).
    linear = abs(target_i - integrated) <= 12 and (lra is None or lra <= 20)
    lra_for_filter = max(1.0, min(lra or 7, 20)) if linear else 7
    chain = (f"loudnorm=I={target_i}:TP={target_tp}:LRA={lra_for_filter}:"
             f"linear={'true' if linear else 'false'}")

    out_path, overwrite = _output_path(spec, audio, f"_{int(target_i)}lufs")
    if Path(out_path).exists() and not overwrite:
        return {"ok": False, "error": f"{out_path} exists; pass overwrite: true or a new out"}
    is_video = Path(out_path).suffix.lower() in _CONTAINERS
    args = ["-i", str(audio)]
    if is_video:
        args += ["-map", "0:v", "-map", "0:a", "-c:v", "copy", "-af", chain,
                 "-c:a", "aac", "-b:a", "192k"]
    else:
        args += ["-af", chain, "-c:a", "pcm_s16le"]
    args += [str(out_path)]
    good, why = _run_ffmpeg(ff, args)
    if not good:
        return {"ok": False, "error": f"the loudness pass failed: {why}"}

    after_i, after_lra, after_tp = _vs._integrated(ff, out_path)
    within = after_i is not None and abs(after_i - target_i) <= 1.0
    return _ok(
        "normalize_loudness",
        file=out_path,
        mode="linear (one gain, no dynamics touched)" if linear else
             "dynamic (the range needed re-shaping)",
        before={"integratedLufs": integrated, "lra": lra, "truePeakDbfs": tp},
        after={"integratedLufs": after_i, "lra": after_lra, "truePeakDbfs": after_tp},
        target={"integratedLufs": target_i, "truePeakDbtp": target_tp,
                "platform": platform or None},
        onTarget=within,
        note=None if within else
            "not on target: a true-peak or range constraint pushed the gain; "
            "measure the result and decide, do not stack passes",
        source="ffmpeg (two-pass ebur128 loudnorm)",
    )


# ---------------------------------------------------------------------------
# Music analysis shared: onset curve, tempo, chroma
# ---------------------------------------------------------------------------


def _onset_curve(samples, env):
    """The positive-difference envelope: energy rises are the beat proxy,
    at 100 Hz. Returns (curve, times)."""
    curve = [0.0] * len(env)
    for i in range(1, len(env)):
        d = env[i] - env[i - 1]
        if d > 0:
            curve[i] = d
    return curve


def _tempo_autocorr(curve, fps=100):
    """BPM from onset autocorrelation, 60-190 BPM. Returns (bpm, strength)."""
    # decimate to 50 fps: half the work, onsets are slower than that
    dec = [curve[i] + curve[i + 1] for i in range(0, len(curve) - 1, 2)]
    n = len(dec)
    if n < 50:
        return None, 0.0
    mean = sum(dec) / n
    centred = [d - mean for d in dec]
    energy = sum(c * c for c in centred)
    if energy <= 0:
        return None, 0.0
    best_bpm, best_val = None, 0.0
    half = n // 2
    for lag in range(int(0.3 * 50), min(int(2.0 * 50), half)):  # 0.3-2.0 s
        acc = sum(centred[i] * centred[i + lag] for i in range(n - lag))
        val = acc / energy
        if val > best_val:
            best_val = val
            best_bpm = 60.0 * 50 / lag
    return round(best_bpm, 1) if best_bpm else None, round(best_val, 3)


_KRUMHANSL_MAJOR = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88]
_KRUMHANSL_MINOR = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17]
_PITCHES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]


def _chroma_key(samples, env, frames=12):
    """Pitch-class energy from a few high-resolution FFTs, correlated with
    the Krumhansl profiles. Returns (key, mode, confidence)."""
    size = 4096
    per_bin = RATE / size
    loudest = sorted(range(len(env)), key=lambda i: -env[i])[:frames]
    chroma = [0.0] * 12
    used = 0
    for i in loudest:
        mags = _vq._spectrum(samples, i * _vq.HOP, size=size)
        if not mags:
            continue
        used += 1
        for k in range(2, len(mags) - 1):
            hz = k * per_bin
            if hz < 55 or hz > 2000:
                continue
            if mags[k] < 0.1 * max(mags[2:len(mags) // 4] or [1.0]):
                continue
            midi = round(69.0 + 12.0 * math.log2(hz / 440.0))
            if 0 <= midi < 120:
                chroma[midi % 12] += mags[k]
    if not used:
        return None, None, 0.0
    total = sum(chroma) or 1.0
    chroma = [c / total for c in chroma]

    def correlate(profile):
        pm = sum(profile) / 12
        cm = sum(chroma) / 12
        num = sum((profile[i] - pm) * (chroma[i] - cm) for i in range(12))
        den = math.sqrt(sum((profile[i] - pm) ** 2 for i in range(12)) *
                        sum((chroma[i] - cm) ** 2 for i in range(12)))
        return num / den if den else 0.0

    def rotate(profile, r):
        # rotated[r] is the tonic: profile[0] lands ON index r, so a chroma
        # peaking at pitch class r means the tonic IS r.
        return [profile[(i - r) % 12] for i in range(12)]

    best = (None, None, -2.0)
    for rotation in range(12):
        maj = correlate(rotate(_KRUMHANSL_MAJOR, rotation))
        if maj > best[2]:
            best = (_PITCHES[rotation], "major", maj)
    for rotation in range(12):
        mi = correlate(rotate(_KRUMHANSL_MINOR, rotation))
        if mi > best[2]:
            best = (_PITCHES[rotation], "minor", mi)
    return best


# ---------------------------------------------------------------------------
# 4. detect_music_segments
# ---------------------------------------------------------------------------


def _detect_music_segments(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    duration = len(samples) / RATE

    # Per-second windows: beat strength (onset autocorrelation), duty cycle
    # (how much of the window carries energy), centroid stability.
    curve = _onset_curve(samples, env)
    spans = []
    step = 100  # 1 s
    for i in range(0, max(0, len(env) - step), step):
        seg_env = env[i:i + step]
        seg_curve = curve[i:i + step]
        if not seg_env:
            continue
        duty = sum(1 for v in seg_env if v > 0.02) / len(seg_env)
        bpm, strength = _tempo_autocorr(seg_curve)
        # centroid stability over 4 windows inside the second
        cents = []
        for j in range(i, min(i + step - _vq.HOP, len(env) - 1), 25):
            mags = _vq._spectrum(samples, j * _vq.HOP)
            if not mags:
                continue
            num = sum(k * m for k, m in enumerate(mags))
            den = sum(mags)
            if den > 1e-9:
                cents.append((num / den) * (RATE / _vq.WIN))
        cent_cv = (math.sqrt(sum((c - sum(cents) / len(cents)) ** 2 for c in cents) / len(cents))
                   / (sum(cents) / len(cents))) if len(cents) >= 3 else None
        if duty < 0.06:
            cls = "silence"
        elif strength > 0.35 and duty > 0.85 and (cent_cv is None or cent_cv < 0.5):
            cls = "music"
        elif duty > 0.85 and (cent_cv or 1) > 0.6:
            cls = "music_or_dense_speech"
        elif duty > 0.4:
            cls = "speech"
        else:
            cls = "mixed"
        spans.append({"start": round(i / 100.0, 2), "end": round((i + step) / 100.0, 2),
                      "class": cls, "dutyCycle": round(duty, 2),
                      "beatStrength": strength, "bpmHint": bpm,
                      "centroidCv": round(cent_cv, 2) if cent_cv is not None else None})

    # merge equal neighbours
    merged = []
    for s in spans:
        if merged and merged[-1]["class"] == s["class"]:
            merged[-1]["end"] = s["end"]
        else:
            merged.append(dict(s))
    coverage = {}
    for s in spans:
        coverage[s["class"]] = coverage.get(s["class"], 0) + 1
    has_music = any(s["class"].startswith("music") for s in spans)
    return _ok(
        "detect_music_segments",
        duration=round(duration, 2),
        segments=merged,
        coverageSeconds={k: round(v * step / 100.0, 1) for k, v in coverage.items()},
        musicPresent=has_music,
        editingAnswer=(
            "this take already carries music: do NOT double the bed - duck or replace, never stack"
            if has_music else
            "no music in this take: a BGM bed is safe - start it under the first speech, "
            "duck 10-14 dB while words run"),
        directorHints=[
            "silence spans longer than 2 s are either breathing room or a cut candidate - check the pause classes",
            "a music_or_dense_speech span needs the emotion read to settle which it is",
        ],
        source="python (duty cycle, onset autocorrelation beat strength, centroid stability)",
    )


# ---------------------------------------------------------------------------
# 5. identify_music
# ---------------------------------------------------------------------------


def _identify_music(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    duration = len(samples) / RATE
    if duration < 4:
        return {"ok": False, "error": "under 4 s of audio: too little to identify anything"}

    curve = _onset_curve(samples, env)
    bpm, strength = _tempo_autocorr(curve)
    key, mode, key_conf = _chroma_key(samples, env)

    # Instrumentation hints from band ratios over the loudest moments.
    loudest = sorted(range(len(env)), key=lambda i: -env[i])[:8]
    bass = mids = highs = 0.0
    for i in loudest:
        mags = _vq._spectrum(samples, i * _vq.HOP)
        if not mags:
            continue
        bass += _vq._band_energy(mags, 30, 150)
        mids += _vq._band_energy(mags, 150, 2000)
        highs += _vq._band_energy(mags, 2000, 8000)
    total = bass + mids + highs or 1.0
    balance = {"bass": round(bass / total, 3), "mid": round(mids / total, 3),
               "high": round(highs / total, 3)}
    hints = []
    if balance["bass"] > 0.25:
        hints.append("bass-driven: electronic, hip-hop or a live kit")
    if balance["high"] > 0.4:
        hints.append("bright: hats, cymbals or synth leads forward")
    if balance["mid"] > 0.55:
        hints.append("mid-forward: vocal, guitar or piano carried")

    energy_db = _vq._db(max(env) if env else 0.0)
    tags = []
    if bpm and bpm > 120:
        tags.append("upbeat")
    elif bpm and bpm < 90:
        tags.append("laid-back")
    if mode == "minor":
        tags.append("moody")
    else:
        tags.append("warm")
    if strength > 0.5:
        tags.append("driving")
    descriptor = ", ".join([f"{int(bpm) or '?'} BPM" if bpm else "? BPM",
                            f"{key} {mode}" if key else "key unknown"] + tags[:2])
    return _ok(
        "identify_music",
        tempo={"bpm": bpm, "strength": strength},
        key={"tonic": key, "mode": mode, "confidence": round(key_conf, 2)},
        spectralBalance=balance,
        instrumentationHints=hints or ["balanced spectrum: no single family dominates"],
        energyDbfs=round(energy_db, 1),
        moodTags=tags,
        libraryQuery=f"{descriptor} {(' '.join(tags[:2]))}",
        note="this is an offline descriptor, NOT a title match: naming the exact song "
             "needs an online fingerprint service (AcrCloud/Shazam APIs). The "
             "libraryQuery is the honest way to FIND similar music: paste it into "
             "bgm_search.",
        source="python (onset autocorrelation tempo, Krumhansl chroma key, band ratios)",
    )


# ---------------------------------------------------------------------------
# 6. analyze_music_structure
# ---------------------------------------------------------------------------


def _analyze_music_structure(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    duration = len(samples) / RATE
    if duration < 8:
        return {"ok": False, "error": "under 8 s of audio: structure needs phrases to compare"}

    curve = _onset_curve(samples, env)
    bpm, strength = _tempo_autocorr(curve)
    if not bpm:
        return {"ok": False, "error": "no tempo found: is there music in this file? "
                                      "(detect_music_segments says what is in it)"}

    # The bar grid: period = 4 beats, phase = the strongest onset.
    beat = 60.0 / bpm
    bar = beat * 4
    onsets = [(t / 100.0, c) for t, c in enumerate(curve) if c > 0.25 * max(curve)]
    best_phase, best_sum = 0.0, -1.0
    if onsets:
        for k in range(8):
            phase = bar * k / 8
            s = sum(c for t, c in onsets
                    if abs(((t - phase) % bar) - 0) < 0.12 or
                       abs(((t - phase) % bar) - bar) < 0.12)
            if s > best_sum:
                best_sum, best_phase = s, phase

    # Sections: self-similarity of 2-bar features (energy + brightness),
    # boundaries where the vector jumps.
    bar_len_frames = int(bar * 100)
    feats = []
    for i in range(0, len(env) - bar_len_frames, bar_len_frames):
        seg = env[i:i + bar_len_frames]
        if not seg:
            continue
        e = sum(seg) / len(seg)
        feats.append(e)
    boundaries = [0]
    for i in range(1, len(feats)):
        prev = feats[i - 1] or 1e-9
        if abs(feats[i] - prev) / prev > 0.45:
            boundaries.append(i)
    boundaries.append(len(feats))

    sections = []
    peak_e = max(feats) or 1.0
    for n in range(len(boundaries) - 1):
        lo, hi = boundaries[n], boundaries[n + 1]
        e = sum(feats[lo:hi]) / max(1, hi - lo)
        role = ("intro/outro" if e < 0.45 * peak_e else
                "peak/chorus" if e > 0.8 * peak_e else
                "build" if n and e > (sum(feats[boundaries[n - 1]:lo]) / max(1, lo - boundaries[n - 1]))
                else "verse/groove")
        sections.append({
            "start": round(boundaries[n] * bar, 2), "end": round(boundaries[n + 1] * bar, 2),
            "bars": hi - lo, "energy": round(e, 4), "role": role,
        })
    # the drop: the largest single energy jump between adjacent sections
    drops = []
    for n in range(1, len(sections)):
        jump = sections[n]["energy"] - sections[n - 1]["energy"]
        if jump > 0.35 * peak_e:
            drops.append({"at": sections[n]["start"], "jump": round(jump, 3)})
    cut_safe = [round(best_phase + k * bar, 2) for k in range(int(duration / bar))]
    return _ok(
        "analyze_music_structure",
        tempo={"bpm": bpm, "confidence": strength},
        barGrid={"seconds": round(bar, 3), "phase": round(best_phase, 3),
                 "bars": len(feats)},
        sections=sections,
        drops=drops,
        energyCurve=[round(f, 4) for f in feats],
        cutSafePoints=cut_safe[:64],
        directorHints=[
            "cut on cutSafePoints (bar boundaries): every cut lands musical",
            "save the strongest visual for the drop; hold a beat before it",
            "the intro/outro sections are the natural places for titles and CTAs",
        ],
        source="python (onset autocorrelation, bar-phase search, 2-bar energy self-similarity)",
    )


# ---------------------------------------------------------------------------
# Dispatch, selftest, entry
# ---------------------------------------------------------------------------

TOOLS = {
    "enhance_speech": _enhance_speech,
    "remove_hiss_hum_rumble": _remove_hiss_hum_rumble,
    "normalize_loudness": _normalize_loudness,
    "detect_music_segments": _detect_music_segments,
    "identify_music": _identify_music,
    "analyze_music_structure": _analyze_music_structure,
}


def _synth_speech(path):
    """Speech-ish: phrases with gaps, pitch wandering - NOT beat-locked."""
    import wave as _wave
    rate = RATE
    total = []
    for phrase in range(6):
        n = int(0.7 * rate)
        f0 = 150 + 40 * ((phrase % 3) - 1)
        for i in range(n):
            t = i / rate
            v = (0.4 * math.sin(2 * math.pi * f0 * t) +
                 0.2 * math.sin(4 * math.pi * f0 * t)) * (0.6 + 0.4 * math.sin(2 * math.pi * 4 * t))
            total.append(0.5 * v)
        total += [0.0] * int(0.35 * rate)
    with _wave.open(path, "wb") as h:
        h.setnchannels(1)
        h.setsampwidth(2)
        h.setframerate(rate)
        h.writeframes(b"".join(int(max(-1, min(1, s)) * 32000).to_bytes(2, "little", signed=True)
                               for s in total))
    return len(total) / rate


def _synth_music(path, bpm=120.0):
    """Music-ish: a beat grid (kick every beat, hat every half), a bass
    line and a chord pad in A minor."""
    import wave as _wave
    rate = RATE
    beat = 60.0 / bpm
    n = int(12 * rate)
    a_minor = [220.0, 261.63, 329.63]  # A3 C4 E4
    out = []
    for i in range(n):
        t = i / rate
        beat_pos = (t % beat) / beat
        bar_pos = (t % (beat * 4)) / (beat * 4)
        kick = 0.6 * math.sin(2 * math.pi * 55 * t) * math.exp(-beat_pos * 12)
        hat = 0.12 * (1 if (i * 7919) % 3 else -1) * (1 if (t % (beat / 2)) < 0.02 else 0) * 0.5
        bass_n = a_minor[[0, 0, 3 % 3, 2 % 3][int(bar_pos * 4) % 4]] / 2
        bass = 0.25 * math.sin(2 * math.pi * bass_n * t)
        chord = sum(0.1 * math.sin(2 * math.pi * f * t) for f in a_minor)
        # section shape: quiet first 2 bars, loud after
        gain = 0.5 if t < 2 * beat * 4 else 1.0
        out.append(gain * (kick + hat + bass + chord))
    with _wave.open(path, "wb") as h:
        h.setnchannels(1)
        h.setsampwidth(2)
        h.setframerate(rate)
        h.writeframes(b"".join(int(max(-1, min(1, s)) * 32000).to_bytes(2, "little", signed=True)
                               for s in out))
    return len(out) / rate


def _synth_hum(path):
    """A noisy voiceover with 50 Hz hum and rumble baked in."""
    import wave as _wave
    rate = RATE
    n = int(4 * rate)
    out = []
    for i in range(n):
        t = i / rate
        voice = 0.0
        if 0.5 < t < 3.2:
            voice = 0.4 * math.sin(2 * math.pi * 170 * t) * (0.6 + 0.4 * math.sin(2 * math.pi * 3 * t))
        hum = 0.05 * math.sin(2 * math.pi * 50 * t) + 0.02 * math.sin(2 * math.pi * 100 * t)
        rumble = 0.06 * math.sin(2 * math.pi * 25 * t)
        hiss = 0.01 * (1 if (i * 104729) % 7 else -1)
        out.append(voice + hum + rumble + hiss)
    with _wave.open(path, "wb") as h:
        h.setnchannels(1)
        h.setsampwidth(2)
        h.setframerate(rate)
        h.writeframes(b"".join(int(max(-1, min(1, s)) * 32000).to_bytes(2, "little", signed=True)
                               for s in out))
    return len(out) / rate


def selftest(only=None):
    import tempfile
    checks = []
    with tempfile.TemporaryDirectory(prefix="audiorepair-") as tmp:
        speech = str(Path(tmp) / "speech.wav")
        music = str(Path(tmp) / "music.wav")
        humming = str(Path(tmp) / "humming.wav")
        _synth_speech(speech)
        _synth_music(music)
        _synth_hum(humming)
        plans = {
            "enhance_speech": {"audio": speech, "out": str(Path(tmp) / "enhanced.wav")},
            "remove_hiss_hum_rumble": {"audio": humming, "out": str(Path(tmp) / "clean.wav")},
            "normalize_loudness": {"audio": speech, "out": str(Path(tmp) / "norm.wav"),
                                   "targetLufs": -18},
            "detect_music_segments": {"audio": music},
            "identify_music": {"audio": music},
            "analyze_music_structure": {"audio": music},
        }
        for name, handler in TOOLS.items():
            if only and name != only:
                continue
            spec = plans.get(name) or {"audio": speech}
            try:
                out = handler(spec)
                extra = None
                if name in ("enhance_speech", "remove_hiss_hum_rumble", "normalize_loudness"):
                    made = out.get("file")
                    extra = "file written" if (made and Path(str(made)).is_file()) else "no file!"
                checks.append({"tool": name, "ok": bool(out.get("ok")) and extra != "no file!",
                               "error": out.get("error"), "extra": extra})
            except Exception as exc:  # noqa: BLE001
                checks.append({"tool": name, "ok": False,
                               "error": f"{type(exc).__name__}: {exc}"})
    failed = [c for c in checks if not c.get("ok")]
    return {
        "ok": not failed,
        "schema": SCHEMA,
        "action": "selftest",
        "checks": checks,
        "failed": failed,
        "error": f"{len(failed)} tool(s) failed: " + "; ".join(
            f"{c['tool']}: {c.get('error')}" for c in failed) if failed else None,
    }


def command(spec):
    tool = str(spec.get("tool") or "")
    action = str(spec.get("action", "extract"))
    try:
        if action == "selftest":
            return selftest(tool if tool in TOOLS else None)
        handler = TOOLS.get(tool)
        if handler is None:
            return {"ok": False,
                    "error": f"unknown tool {tool!r}: one of {sorted(TOOLS)}"}
        return handler(spec)
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def main():
    parser = argparse.ArgumentParser(description="The take cleaned and the music read.")
    parser.add_argument("cmd", choices=["extract", "selftest"])
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
