#!/usr/bin/env python3
"""concat-voice-quality: the voice itself, measured - the take's health.

Pure Python standard library (numpy used when it happens to be installed,
never required), same contract as the other sense scripts: `voice_quality.py
<extract|selftest> --spec -` reads one JSON spec from stdin and prints one
JSON object on stdout. The spec names the tool it wants in `tool`.

THE SIX TOOLS THIS MODULE SERVES

  detect_vocal_events       laughs, sighs, breaths, coughs, throat clears,
                            mouth clicks - found in the envelope, named by
                            their shape, cross-checked against the script
  classify_voice_quality    resonant / breathy / tense / creaky / pressed -
                            jitter, shimmer, HNR and spectral tilt, each a
                            named proxy, not a lab instrument
  classify_recording_quality  noise floor, SNR, clipping, hum at 50/60 Hz
                            and harmonics, DC offset, bandwidth - the
                            verdict that decides whether BGM helps or
                            covers
  measure_reverb            RT60 from Schroeder decay on transient tails,
                            early/late reflection ratio, the room class
  detect_restarts           false starts, self-repairs, stalling repeats,
                            abandoned clauses - every one a cut candidate
                            with its safe-to-cut verdict
  classify_filler_type      beyond um/uh: discourse markers, hedges,
                            intensifiers, backchannels - what each one
                            costs the cut and what it earns the voice

The audio is decoded once per call to 16 kHz mono PCM. Every measurement
says what it measured and how; a file with no speech-shaped energy says
so instead of inventing a reading.
"""

from __future__ import annotations

import argparse
import io
import json
import math
import re
import struct
import sys
import wave
from array import array
from pathlib import Path

try:
    import voice_signals as _vs
except ImportError:  # pragma: no cover
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import voice_signals as _vs

try:
    import numpy as _np
except Exception:  # noqa: BLE001 - optional, never required
    _np = None

SCHEMA = "funcut.voice-quality/1"

RATE = 16000
HOP = 160  # the envelope answers at 100 Hz
WIN = 1024  # the FFT window at 16 kHz: 64 ms

_PCM_CACHE = {}


# ---------------------------------------------------------------------------
# The decode: one file, one 16 kHz mono buffer, cached for the call
# ---------------------------------------------------------------------------


def _pcm(spec, path):
    """Decode `path` to (samples, rate) mono. Cached: several tools in one
    process would otherwise each pay the decode."""
    key = str(path)
    if key in _PCM_CACHE:
        return _PCM_CACHE[key]
    ff, _ = _vs._ffmpeg(spec)
    if not Path(str(path)).is_file():
        return None, None
    cmd = [ff, "-hide_banner", "-nostdin", "-i", str(path),
           "-ac", "1", "-ar", str(RATE), "-c:a", "pcm_s16le", "-f", "wav", "-"]
    rc, out, _err = _vs._run(cmd, 240)
    if rc != 0 or not out:
        return None, None
    try:
        with wave.open(io.BytesIO(out)) as handle:
            rate = handle.getframerate()
            n = handle.getnframes()
            raw = handle.readframes(n)
        samples = array("h", raw)
    except Exception:  # noqa: BLE001
        return None, None
    if len(raw) % 2:
        samples = samples[:len(samples) - 1]
    result = (samples, rate)
    _PCM_CACHE[key] = result
    return result


def _envelope(samples, hop=HOP):
    """RMS per hop, in dBFS, as a list - the base layer every tool here
    reads. Pure-python loop over blocks, O(n)."""
    n = len(samples)
    frames = []
    for i in range(0, n - hop + 1, hop):
        acc = 0
        chunk = samples[i:i + hop]
        for s in chunk:
            acc += s * s
        frames.append(math.sqrt(acc / hop) / 32768.0)
    return frames


def _db(x, floor=1e-7):
    return 20.0 * math.log10(max(x, floor))


# ---------------------------------------------------------------------------
# The FFT: numpy when present, a radix-2 Cooley-Tukey otherwise
# ---------------------------------------------------------------------------


def _fft_pow2(re):
    """A power-of-two complex FFT, in-place style, returning (re, im).
    Window must already be a power of two."""
    if _np is not None:
        spec = _np.fft.rfft(_np.array(re, dtype=_np.float64))
        return spec.real.tolist(), spec.imag.tolist()
    n = len(re)
    im = [0.0] * n
    # bit reversal
    j = 0
    for i in range(1, n):
        bit = n >> 1
        while j & bit:
            j ^= bit
            bit >>= 1
        j |= bit
        if i < j:
            re[i], re[j] = re[j], re[i]
    length = 2
    while length <= n:
        ang = -2.0 * math.pi / length
        wlen_re, wlen_im = math.cos(ang), math.sin(ang)
        for i in range(0, n, length):
            w_re, w_im = 1.0, 0.0
            for k in range(length // 2):
                u_re, u_im = re[i + k], im[i + k]
                v_re = re[i + k + length // 2] * w_re - im[i + k + length // 2] * w_im
                v_im = re[i + k + length // 2] * w_im + im[i + k + length // 2] * w_re
                re[i + k] = u_re + v_re
                im[i + k] = u_im + v_im
                re[i + k + length // 2] = u_re - v_re
                im[i + k + length // 2] = u_im - v_im
                nw_re = w_re * wlen_re - w_im * wlen_im
                w_im = w_re * wlen_im + w_im * wlen_re
                w_re = nw_re
        length *= 2
    return re, im


def _spectrum(samples, offset, size=WIN):
    """Magnitude spectrum of one window (Hann), as a list of length
    size//2 + 1, bins of rate/size Hz each."""
    if offset + size > len(samples):
        offset = max(0, len(samples) - size)
        if offset < 0:
            return None
    re = []
    for i in range(size):
        s = samples[offset + i] / 32768.0
        window = 0.5 - 0.5 * math.cos(2.0 * math.pi * i / size)
        re.append(s * window)
    fre, fim = _fft_pow2(re)
    mags = [math.hypot(fre[k], fim[k]) for k in range(size // 2 + 1)]
    return mags


def _band_energy(mags, lo_hz, hi_hz, rate=RATE, size=WIN):
    """Mean magnitude in a band - the building block of tilt, hum and
    breathiness reads."""
    per_bin = rate / size
    lo = max(0, int(lo_hz / per_bin))
    hi = min(len(mags) - 1, int(hi_hz / per_bin))
    if hi <= lo:
        return 0.0
    return sum(mags[lo:hi + 1]) / (hi - lo + 1)


# ---------------------------------------------------------------------------
# F0: autocorrelation on sparse voiced frames (the jitter/shimmer base)
# ---------------------------------------------------------------------------


def _f0_frame(samples, offset, rate=RATE):
    """One autocorrelation F0 estimate on a 30 ms window decimated x2.
    Returns (f0_hz, strength 0-1) or (None, strength)."""
    size = int(0.030 * rate)  # 480
    if offset + size > len(samples):
        return None, 0.0
    frame = [samples[offset + i] / 32768.0 for i in range(size)]
    # decimate by 2: 8 kHz effective, F0 lag range 40-400 Hz -> 20..100 samples
    dec = frame[::2]
    mean = sum(dec) / len(dec)
    dec = [d - mean for d in dec]
    energy = sum(d * d for d in dec)
    if energy < 1e-4:
        return None, 0.0
    best_lag, best_val = 0, 0.0
    for lag in range(20, min(100, len(dec) - 1)):
        acc = sum(dec[i] * dec[i + lag] for i in range(len(dec) - lag))
        norm = acc / energy
        if norm > best_val:
            best_val, best_lag = norm, lag
    if best_lag == 0 or best_val < 0.35:
        return None, best_val
    # parabolic interpolation on the autocorrelation peak
    f0 = (rate / 2.0) / best_lag
    return min(400.0, max(60.0, f0)), best_val


def _voiced_frames(samples, env, rate=RATE, cap=150):
    """Frames to read F0 from: the loudest runs of envelope, capped so the
    pure-python autocorrelation stays inside its budget."""
    if not env:
        return []
    ordered = sorted(range(len(env)), key=lambda i: -env[i])
    take = sorted(ordered[:cap])
    return [(i * HOP, env[i]) for i in take if env[i] > 0.02]


def _f0_track(samples, env):
    """[(time, f0, strength, amp)] for the voiced frames."""
    out = []
    for offset, amp in _voiced_frames(samples, env):
        f0, strength = _f0_frame(samples, offset)
        if f0:
            out.append((offset / RATE, f0, strength, amp))
    return out


def _median(values):
    vals = sorted(v for v in values if v is not None)
    if not vals:
        return None
    mid = len(vals) // 2
    return vals[mid] if len(vals) % 2 else 0.5 * (vals[mid - 1] + vals[mid])


def _ok(tool, **fields):
    out = {"ok": True, "schema": SCHEMA, "tool": tool}
    out.update(fields)
    return out


def _need_audio(spec):
    """The common door: audio in, PCM out. Returns (error, samples, env)."""
    audio = spec.get("audio") or spec.get("path")
    if not audio:
        return "no audio: pass the voiceover file in `audio`", None, None
    samples, rate = _pcm(spec, audio)
    if samples is None or not len(samples):
        return f"could not decode {audio} (is it a file ffmpeg reads?)", None, None
    env = _envelope(samples)
    if not env:
        return "the audio decoded to nothing", None, None
    return None, samples, env


# ---------------------------------------------------------------------------
# 1. detect_vocal_events
# ---------------------------------------------------------------------------

_EVENT_TEXT_CUES = {
    "laugh": ("[laughs]", "*laughs*", "(laughs)", "haha", "hahaha", "hehe", "lol", "[laughing]"),
    "sigh": ("[sighs]", "*sigh*", "(sighs)", "[sigh]", "sighs,", "exhales"),
    "cough": ("[coughs]", "*cough*", "(coughs)", "[cough]"),
    "breath": ("[breath]", "*breathes*", "(inhales)"),
}


def _detect_vocal_events(spec):
    lines = _vs._collect_lines(spec)
    err, samples, env = _need_audio(spec)
    if err:
        return {"ok": False, "error": err}

    duration = len(samples) / RATE
    # The energy thresholds the shapes are judged against.
    loud = sorted(env, reverse=True)
    speech_ref = loud[max(0, int(0.05 * len(loud)))] if loud else 0.0
    quiet = sorted(env)
    floor = quiet[int(0.25 * len(quiet))] if quiet else 0.0
    thresh = max(floor * 1.8, speech_ref * 0.04)

    # Pulse trains in the envelope: the laugh signature (4-8 Hz quaver).
    events = []
    active = (env[0] > thresh) if env else False
    runs = []
    start = 0 if active else -1
    for i in range(1, len(env)):
        now = env[i] > thresh
        if now and not active:
            start = i
        if not now and active and start >= 0:
            runs.append((start / 100.0, i / 100.0))
            start = -1
        active = now
    if start >= 0:
        runs.append((start / 100.0, len(env) / 100.0))

    # Clusters: short runs separated by sub-220 ms gaps are one gesture -
    # a laugh is exactly such a train of quaver pulses, and the phrases
    # between them breathe for at least a quarter second.
    clusters = []
    for begin, end in runs:
        if clusters and begin - clusters[-1][1] < 0.22:
            clusters[-1][1] = end
        else:
            clusters.append([begin, end])

    for begin, end in clusters:
        span = end - begin
        if span < 0.08:
            continue
        # quaver check: the cluster's member runs are the pulses
        member = [r for r in runs if begin <= r[0] < end]
        pulses = len(member)
        quaver_hz = (pulses - 1) / max(span, 0.05) if pulses > 1 else 0.0
        seg = env[int(begin * 100):int(end * 100)]
        amp = max(seg) if seg else 0.0
        # voicing: autocorrelation strength at the loudest member's centre
        # (the cluster centre can sit in the silent gap between pulses)
        loudest_mid = int((begin + end) / 2 * RATE)
        best_amp = -1.0
        for r_begin, r_end in member:
            r_mid = int((r_begin + r_end) / 2 * RATE)
            r_amp = env[min(len(env) - 1, int((r_begin + r_end) / 2 * 100))] if env else 0.0
            if r_amp > best_amp:
                best_amp = r_amp
                loudest_mid = r_mid
        f0, strength = _f0_frame(samples, loudest_mid)
        rel = amp / max(speech_ref, 1e-6)
        if (3.0 <= quaver_hz <= 9.0 and strength > 0.3 and rel > 0.15
                and span > 0.3 and pulses >= 3):
            kind, why = "laugh", f"{pulses} quaver pulses at {quaver_hz:.1f} Hz, voiced"
            confidence = 0.7
        elif strength > 0.35 and span >= 0.5 and rel < 0.3:
            kind, why = "sigh", "long voiced low-energy span"
            confidence = 0.5
        elif strength > 0.25 and 0.15 <= span <= 0.5 and rel < 0.25:
            kind, why = "breath", "short low-energy span beside speech"
            confidence = 0.45
        elif strength < 0.2 and rel > 0.5 and span < 0.6:
            kind, why = "cough_or_throat_clear", "sharp unvoiced transient"
            confidence = 0.4
        else:
            continue
        events.append({
            "start": round(begin, 3), "end": round(end, 3),
            "duration": round(span, 3), "kind": kind, "confidence": confidence,
            "cues": [why], "f0": round(f0, 1) if f0 else None,
        })

    # Merge coughs that double within 400 ms: the throat clear.
    merged = []
    for ev in events:
        if (ev["kind"] == "cough_or_throat_clear" and merged
                and merged[-1]["kind"] == "cough_or_throat_clear"
                and ev["start"] - merged[-1]["end"] < 0.4):
            merged[-1]["end"] = ev["end"]
            merged[-1]["kind"] = "throat_clear"
            merged[-1]["confidence"] = 0.55
            merged[-1]["cues"].append("doubled transient")
        else:
            merged.append(ev)
    events = merged

    # The script cross-check: a written [laughs] confirms what the envelope
    # only suggests, and names events too short to see.
    scripted = []
    for idx, line in enumerate(lines):
        text = str(line.get("text") or "").lower()
        start_t, end_t = line.get("start"), line.get("end")
        for kind, cues in _EVENT_TEXT_CUES.items():
            if any(cue in text for cue in cues):
                scripted.append({"start": start_t, "end": end_t, "kind": kind,
                                 "confidence": 0.9, "cues": [f"written cue in line {idx}"],
                                 "fromScript": True})
    for ev in scripted:
        if not any(abs((e.get("start") or -9) - (ev["start"] or -9)) < 1.0 and e["kind"] == ev["kind"]
                   for e in events):
            events.append(ev)
    events.sort(key=lambda e: e.get("start") if e.get("start") is not None else 0.0)

    counts = {}
    for ev in events:
        counts[ev["kind"]] = counts.get(ev["kind"], 0) + 1
    cut_candidates = [e for e in events if e["kind"] in ("cough_or_throat_clear", "throat_clear")]
    keep = [e for e in events if e["kind"] in ("laugh", "sigh")]
    return _ok(
        "detect_vocal_events",
        duration=round(duration, 2),
        events=events,
        counts=counts,
        cutCandidates=cut_candidates,
        keepForAuthenticity=keep,
        thresholds={"floorDbfs": round(_db(floor), 1), "speechRefDbfs": round(_db(speech_ref), 1)},
        directorHints=[
            "cut the throat clears and coughs - they read as mistakes, not texture",
            "a laugh or a sigh is authentic delivery: keep it, let the visual react to it",
        ] if events else ["no vocal events outside the phrases themselves"],
        source="python (100 Hz envelope shapes + autocorrelation voicing, "
               "cross-checked against written cues)",
    )


# ---------------------------------------------------------------------------
# 2. classify_voice_quality
# ---------------------------------------------------------------------------


def _classify_voice_quality(spec):
    err, samples, env = _need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    track = _f0_track(samples, env)
    if len(track) < 6:
        return {"ok": False,
                "error": "not enough voiced audio to read voice quality "
                         "(need a few seconds of actual speech)"}

    f0s = [t[1] for t in track]
    periods = [1.0 / f for f in f0s]
    # jitter proxy: consecutive period variability
    jitters = []
    for i in range(1, len(periods)):
        if abs(track[i][0] - track[i - 1][0]) < 0.06:  # adjacent frames only
            jitters.append(abs(periods[i] - periods[i - 1]) / max(periods[i - 1], 1e-6))
    jitter = (sum(jitters) / len(jitters)) if jitters else None
    # shimmer proxy: consecutive amplitude variability
    shimmers = []
    amps = [t[3] for t in track]
    for i in range(1, len(amps)):
        if abs(track[i][0] - track[i - 1][0]) < 0.06:
            shimmers.append(abs(amps[i] - amps[i - 1]) / max(amps[i - 1], 1e-6))
    shimmer = (sum(shimmers) / len(shimmers)) if shimmers else None
    hnr_proxy = _median([t[2] for t in track]) or 0.0

    # Spectral tilt + breathiness: mean spectrum over the loudest frames.
    loudest = sorted(range(len(env)), key=lambda i: -env[i])[:8]
    lows, highs, hf_noises = [], [], []
    for i in loudest:
        mags = _spectrum(samples, i * HOP)
        if not mags:
            continue
        lows.append(_band_energy(mags, 80, 1000))
        highs.append(_band_energy(mags, 1500, 4000))
        hf_noises.append(_band_energy(mags, 5000, 7500))
    tilt = _db(sum(highs) / max(1e-9, sum(lows))) if lows else None
    breath_noise = _db(sum(hf_noises) / max(1e-9, sum(highs))) if highs else None

    creak = sum(1 for f in f0s if f < 85) / len(f0s)
    f0_mean = sum(f0s) / len(f0s)
    f0_spread = (max(f0s) - min(f0s))

    # The classification: each class needs its cue present, and the reading
    # names every cue so a caller can disagree.
    classes = []
    if hnr_proxy > 0.62 and (tilt is not None and tilt < -12):
        classes.append(("resonant", "strong periodicity, dark tilt"))
    if breath_noise is not None and breath_noise > -6 and hnr_proxy < 0.55:
        classes.append(("breathy", "high-band noise over voiced frames"))
    if jitter and jitter > 0.045 and shimmer and shimmer > 0.35:
        classes.append(("tense", "elevated period and amplitude variability"))
    if creak > 0.25:
        classes.append(("creaky", f"{int(creak * 100)}% of frames under 85 Hz"))
    if len(classes) == 0:
        classes.append(("natural", "no extreme cue: the default reading"))
    primary, primary_why = classes[0]

    genre = []
    if primary in ("resonant", "natural") and jitter and jitter < 0.03:
        genre.append("documentary and long-form narration carry well")
    if tilt is not None and tilt > -8:
        genre.append("commercial and short-form: the bright cut reads on phones")
    if primary in ("breathy", "creaky"):
        genre.append("character and ASMR registers; subtitles will carry clarity")

    return _ok(
        "classify_voice_quality",
        primaryClass=primary,
        classes=[{"class": c, "why": w} for c, w in classes],
        measures={
            "f0MeanHz": round(f0_mean, 1),
            "f0RangeHz": round(f0_spread, 1),
            "jitterProxy": round(jitter, 4) if jitter is not None else None,
            "shimmerProxy": round(shimmer, 3) if shimmer is not None else None,
            "voicingStrengthMedian": round(hnr_proxy, 3),
            "spectralTiltDb": round(tilt, 1) if tilt is not None else None,
            "breathNoiseDb": round(breath_noise, 1) if breath_noise is not None else None,
            "creakFraction": round(creak, 3),
            "voicedFrames": len(track),
        },
        genreFit=genre or ["a neutral take: fits any genre with EQ and pacing"],
        note="jitter, shimmer and voicing strength are autocorrelation proxies "
             "at 16 kHz mono, not clinical instruments - they rank takes, they "
             "do not diagnose voices",
        directorHints=[
            "a breathy or tense take wants warmer grade and fewer cuts - let it breathe",
            "a resonant take carries slow holds and long dissolves",
        ],
        source="python (sparse autocorrelation F0, spectral bands; numpy when installed)",
    )


# ---------------------------------------------------------------------------
# 3. classify_recording_quality
# ---------------------------------------------------------------------------


def _classify_recording_quality(spec):
    err, samples, env = _need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    # Noise floor: the quietest quartile of 250 ms windows.
    win_frames = 25  # 250 ms at 100 fps
    windows = []
    for i in range(0, len(env) - win_frames, win_frames):
        windows.append(max(env[i:i + win_frames]))
    windows.sort()
    if not windows:
        return {"ok": False, "error": "the audio is too short to grade"}
    floor_amp = windows[int(0.15 * len(windows))]
    speech_amp = windows[int(0.9 * len(windows))] if len(windows) > 5 else windows[-1]
    floor_db = _db(floor_amp)
    snr = round(_db(speech_amp) - floor_db, 1)

    # Clipping: samples at the rails, in runs (one-off full-scale is noise).
    clipped_runs = 0
    run = 0
    for s in samples:
        if abs(s) >= 32735:
            run += 1
        else:
            if run >= 4:
                clipped_runs += 1
            run = 0
    if run >= 4:
        clipped_runs += 1
    peak = max(abs(s) for s in samples) / 32768.0

    # DC offset.
    dc = sum(samples) / len(samples) / 32768.0

    # Hum: FFT of the quietest 64 ms windows; peaks at 50/60 Hz harmonics
    # that stand 6 dB over their neighbourhood.
    hums = []
    quietest = sorted(range(len(env)), key=lambda i: env[i])[:6]
    for i in quietest:
        mags = _spectrum(samples, i * HOP)
        if not mags:
            continue
        for base in (50.0, 60.0):
            for harmonic in range(1, 5):
                hz = base * harmonic
                per_bin = RATE / WIN
                k = int(hz / per_bin)
                if k <= 1 or k >= len(mags) - 2:
                    continue
                centre = mags[k]
                around = (mags[k - 1] + mags[k + 1]) / 2 + 1e-9
                if centre / around > 2.0:  # >6 dB over its neighbours
                    hums.append({"hz": hz, "excessDb": round(_db(centre / around), 1)})
        break  # one quiet window is enough; harmonics repeat if real
    hum = hums[:4]

    # Bandwidth: from the loudest window, where the spectrum rolls off.
    mags = _spectrum(samples, max(range(len(env)), key=lambda i: env[i]) * HOP)
    bandwidth = None
    if mags:
        peak_mag = max(mags[2:])
        for k in range(2, len(mags)):
            if mags[k] > peak_mag * 0.1:
                bandwidth = int(k * RATE / WIN)
        if bandwidth:
            bandwidth = min(bandwidth, RATE // 2)

    if floor_db < -60 and snr > 30 and not hum and clipped_runs == 0:
        verdict, fixes = "broadcast", ["no repair needed; keep this chain"]
    elif floor_db < -50 and snr > 24 and clipped_runs == 0:
        verdict, fixes = "clean", ["BGM sits easily on this floor"]
    elif floor_db < -40:
        verdict = "home"
        fixes = ["run remove_hiss_hum_rumble, then enhance_speech"]
        if hum:
            fixes.insert(0, f"notch the {int(hum[0]['hz'])} Hz hum first")
        if clipped_runs:
            fixes.append(f"{clipped_runs} clipped run(s): re-record or de-clip; EQ cannot fix it")
    else:
        verdict = "rough"
        fixes = ["the floor is high: denoise, or lean into it (podcast/verite) and add room-matched BGM"]
        if snr < 12:
            fixes.append("SNR under 12 dB: subtitles stop being optional")

    return _ok(
        "classify_recording_quality",
        verdict=verdict,
        measures={
            "noiseFloorDbfs": round(floor_db, 1),
            "snrDb": snr,
            "peakDbfs": round(_db(peak), 1),
            "clippedRuns": clipped_runs,
            "dcOffset": round(dc, 4),
            "humTones": hum,
            "bandwidthHz": bandwidth,
            "durationSeconds": round(len(samples) / RATE, 2),
        },
        bgmDecision=("a floor this clean lets BGM whisper: -28 LUFS bed, ducked 12 dB under speech"
                     if verdict in ("broadcast", "clean") else
                     "the floor is audible: BGM at or above the floor masks it - "
                     "or clean first with remove_hiss_hum_rumble"),
        fixes=fixes,
        source="python (window statistics, rail-run clipping, quiet-window FFT hum scan)",
    )


# ---------------------------------------------------------------------------
# 4. measure_reverb
# ---------------------------------------------------------------------------


def _measure_reverb(spec):
    err, samples, env = _need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    # Onsets: envelope rises well above the running median.
    sorted_env = sorted(env)
    median = sorted_env[len(sorted_env) // 2] if sorted_env else 0.0
    loud_ref = sorted_env[-1] if sorted_env else 0.0
    onset_thresh = max(median * 2.0, loud_ref * 0.25)
    onsets = []
    above = False
    for i in range(1, len(env)):
        now = env[i] > onset_thresh
        if now and not above and env[i] > env[i - 1] * 1.5:
            onsets.append(i)
        above = now
    onsets = onsets[:200]

    # Schroeder backward integration after each strong onset: the time the
    # tail takes to fall 15 dB, x4 for the RT60 the room implies.
    rt60s = []
    early_ratios = []
    for onset in onsets:
        start = onset * HOP
        seg = []
        for i in range(onset, min(len(env), onset + 100)):  # 1 s of tail
            acc = 0.0
            chunk = samples[i * HOP:(i + 1) * HOP]
            for s in chunk:
                acc += s * s
            seg.append(acc)
        if len(seg) < 20 or not seg[0]:
            continue
        # backward-integrated energy, then the 15 dB point
        total = sum(seg)
        running = total
        drop_t = None
        for i, e in enumerate(seg):
            if total > 0 and running / total < 10 ** (-15 / 10):
                drop_t = i / 100.0
                break
            running -= e
        if drop_t and 0.03 < drop_t < 1.0:
            rt60s.append(min(2.5, drop_t * 4.0))
        # early/late: 0-50 ms vs 50-400 ms after the onset
        early = sum(seg[:5])
        late = sum(seg[5:40])
        if early + late > 0:
            early_ratios.append(early / (early + late))
    if not rt60s:
        return _ok(
            "measure_reverb",
            rt60=None,
            roomClass="none detected",
            note="no strong transient tails to measure: continuous speech or a "
                 "very dry take - RT60 needs an onset to decay from",
            source="python (Schroeder backward integration on envelope tails)",
        )
    rt60 = _median(rt60s)
    drr = _median(early_ratios)
    room = ("dry" if rt60 < 0.25 else "studio/vocal booth" if rt60 < 0.4 else
            "small room" if rt60 < 0.7 else "large room/hall")
    return _ok(
        "measure_reverb",
        rt60=round(rt60, 2),
        roomClass=room,
        rt60Spread=[round(min(rt60s), 2), round(max(rt60s), 2)],
        directToReverbProxy=round(drr, 2) if drr is not None else None,
        onsetsMeasured=len(rt60s),
        editHints=[
            f"gate or music attacks want release < {max(0.05, rt60 / 4):.2f} s so the tail is kept, not chopped",
            ("this room is dry enough that ADR inserts will match" if rt60 < 0.4
             else "this room rings: matching B-roll audio will clash - cut to music, not to room sound"),
        ],
        source="python (Schroeder backward integration, median over onset tails)",
    )


# ---------------------------------------------------------------------------
# 5. detect_restarts
# ---------------------------------------------------------------------------


def _detect_restarts(spec):
    lines = _vs._collect_lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript (with word timings), lines or script"}
    has_times = any(w.get("start") is not None for line in lines for w in _vs._words_of(line))
    restarts = []
    all_words = []  # (line_idx, word)
    for idx, line in enumerate(lines):
        words = _vs._words_of(line)
        text = str(line.get("text") or "")
        # abandoned clause: no terminal punctuation, next line re-anchors
        if (idx + 1 < len(lines) and text.strip()
                and not re_terminated(text)
                and re_anchor(str(lines[idx + 1].get("text") or ""))):
            restarts.append({
                "line": idx, "kind": "abandoned_clause",
                "text": text[:120],
                "span": {"start": line.get("start"), "end": line.get("end")},
                "cutSafe": False,
                "why": "the clause is left unfinished and the next line restarts the thought",
            })
        all_words.extend((idx, w) for w in words)

    # repeated 1-3 word n-grams back to back
    for i in range(len(all_words) - 1):
        li, w = all_words[i]
        for n in (3, 2, 1):
            if i + 2 * n > len(all_words):
                continue
            first = [t for _, t in all_words[i:i + n]]
            second = [t for _, t in all_words[i + n:i + 2 * n]]
            clean = [re_clean(t.get("text") or "") for t in first]
            clean2 = [re_clean(t.get("text") or "") for t in second]
            if (all(clean) and clean == clean2 and len("".join(clean)) > 1
                    and not any(c in _GRAMMAR_REPEATS for c in clean)):
                gap = None
                if first[-1].get("end") is not None and second[0].get("start") is not None:
                    gap = second[0]["start"] - first[-1]["end"]
                    if gap > 0.6 or gap < -0.05:
                        continue
                restarts.append({
                    "line": li, "kind": "stalling_repeat",
                    "words": clean, "repeat": clean2,
                    "span": {"start": first[0].get("start"), "end": second[-1].get("end")},
                    "gap": round(gap, 3) if gap is not None else None,
                    "cutSafe": True,
                    "why": "a 1-3 word n-gram repeated back to back",
                })
                break
        # stall elongation: a word held >2x its length class
        word = w.get("text") or ""
        start, end = w.get("start"), w.get("end")
        if start is not None and end is not None and len(word) >= 2:
            dur = end - start
            expected = 0.08 + 0.055 * len(re_clean(word) or word)
            if dur > 2.2 * expected and dur > 0.45:
                restarts.append({
                    "line": li, "kind": "elongation",
                    "words": [word],
                    "span": {"start": start, "end": end},
                    "duration": round(dur, 3),
                    "cutSafe": True,
                    "why": f"held {dur:.2f}s vs {expected:.2f}s expected for its length",
                })
    # fragment tokens
    for li, line in enumerate(lines):
        for w in _vs._words_of(line):
            token = str(w.get("text") or "")
            if token.endswith("-") and len(token) > 1:
                restarts.append({
                    "line": li, "kind": "fragment",
                    "words": [token],
                    "span": {"start": w.get("start"), "end": w.get("end")},
                    "cutSafe": True,
                    "why": "a hyphen-cut fragment the speaker abandoned",
                })
    seen_spans = set()
    unique = []
    for r in restarts:
        key = (r["kind"], json.dumps(r.get("span"), sort_keys=True))
        if key in seen_spans:
            continue
        seen_spans.add(key)
        unique.append(r)
    unique.sort(key=lambda r: (r.get("span") or {}).get("start") if (r.get("span") or {}).get("start") is not None else 0)
    saveable = [r for r in unique if r["cutSafe"] and (r.get("span") or {}).get("start") is not None]
    seconds = sum((r["span"]["end"] or 0) - (r["span"]["start"] or 0)
                  for r in saveable if r["span"].get("end") is not None)
    return _ok(
        "detect_restarts",
        restarts=unique,
        counts={k: sum(1 for r in unique if r["kind"] == k) for k in
                {r["kind"] for r in unique}},
        punchUpPotentialSeconds=round(seconds, 2),
        timingBased=bool(has_times),
        note=("spans and gaps are measured from the word timings" if has_times
              else "no word timings: line-level readings only, spans approximate"),
        directorHints=[
            "every stalling_repeat and elongation is a safe cut when its neighbours are clean",
            "an abandoned clause is a rewrite or a pickup re-record, not a cut - the thought breaks",
        ] if unique else ["clean take: no restarts to punch up"],
        source="python (n-gram repeat scan, duration model, clause-termination grammar)",
    )


def re_terminated(text):
    return bool(str(text).rstrip().endswith((".", "!", "?", ":", ";", "...")))


def re_anchor(text):
    return bool(re.match(r"\s*(and|but|so|because|which|that|i|we|the|it|this)\b",
                         str(text or "").lower()))


def re_clean(token):
    return re.sub(r"[^\w']", "", str(token).lower())


# Back-to-back repeats that are GRAMMAR, not stalls: "I know that that is
# true", "I had had enough". Everything else repeated adjacent - "the the",
# "and and", "I I" - is the speaker restarting.
_GRAMMAR_REPEATS = {"that", "had"}


# ---------------------------------------------------------------------------
# 6. classify_filler_type
# ---------------------------------------------------------------------------

_FILLER_CLASSES = [
    ("filled_pause", [r"^\bum+$", r"^\buh+$", r"^\berm+$", r"^\buhm+$", r"^\bhmm+$", r"^\bmhm+$", r"^\bah+$", r"^\beh+$"],
     "safe removal", "pure sound, no meaning carried"),
    ("discourse_marker", [r"^(well|so|anyway|basically|actually|literally|like|right|okay|ok|now|anyways|i mean)$"],
     "style call", "connects thoughts - keep for conversational, cut for punchy"),
    ("hedge", [r"^(kinda|kind|sorta|sort)$", r"^(maybe|probably|perhaps|possibly)$", r"^(just|somewhat|rather|fairly|quite)$"],
     "style call", "softens the claim - cut for authority, keep for honesty"),
    ("intensifier", [r"^(really|very|super|totally|extremely|absolutely|insanely|actually)$"],
     "punch candidate", "amplifiers the energy can ride instead"),
    ("backchannel", [r"^(mm|hmm|yeah|yep|yes|right|sure|uh-?huh)$"],
     "keep in dialogue", "the listener's half of the conversation"),
    ("stalling_repeat", None, "safe removal", "the the / and and: caught by detect_restarts too"),
]


def _classify_filler_type(spec):
    lines = _vs._collect_lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript (with word timings), lines or script"}
    compiled = [(cls, [re.compile(p, re.IGNORECASE) for p in pats], rec, why)
                for cls, pats, rec, why in _FILLER_CLASSES if pats]
    instances = []
    for idx, line in enumerate(lines):
        words = _vs._words_of(line)
        text = str(line.get("text") or "")
        # phrase-level hedges the word scan misses
        for phrase, cls in (("kind of", "hedge"), ("sort of", "hedge"),
                            ("i think", "hedge"), ("i guess", "hedge"),
                            ("you know", "discourse_marker"), ("i mean", "discourse_marker")):
            if phrase in text.lower():
                instances.append({
                    "line": idx, "text": phrase, "class": cls,
                    "start": None, "end": None,
                    "recommendation": next(r for c, _p, r, _w in compiled if c == cls),
                    "note": "phrase cue",
                })
        for w in words:
            token = re_clean(w.get("text") or "")
            if not token or len(token) > 12:
                continue
            for cls, pats, rec, why in compiled:
                if any(p.match(token) for p in pats):
                    instances.append({
                        "line": idx, "text": token, "class": cls,
                        "start": w.get("start"), "end": w.get("end"),
                        "recommendation": rec, "note": why,
                    })
                    break
    counts = {}
    removable = 0.0
    for inst in instances:
        counts[inst["class"]] = counts.get(inst["class"], 0) + 1
        if inst["recommendation"] == "safe removal" and inst.get("start") is not None and inst.get("end") is not None:
            removable += max(0.0, inst["end"] - inst["start"])
    profile = ("conversational and warm" if counts.get("discourse_marker", 0) + counts.get("hedge", 0) >= 3
               else "tight" if not counts else "light")
    return _ok(
        "classify_filler_type",
        instances=instances,
        counts=counts,
        safeRemovalSeconds=round(removable, 2),
        profile=profile,
        directorHints=[
            "filled pauses are safe cuts when the neighbours are clean - call detect_restarts for the spans",
            "discourse markers are a style dial: punchy cut removes them, authentic cut keeps them",
            "intensifiers are where SFX and zoom punches land",
        ] if instances else ["no fillers: the take is already tight"],
        source="python (filler taxonomy over word timings)",
    )


# ---------------------------------------------------------------------------
# Dispatch, selftest, entry
# ---------------------------------------------------------------------------

TOOLS = {
    "detect_vocal_events": _detect_vocal_events,
    "classify_voice_quality": _classify_voice_quality,
    "classify_recording_quality": _classify_recording_quality,
    "measure_reverb": _measure_reverb,
    "detect_restarts": _detect_restarts,
    "classify_filler_type": _classify_filler_type,
}


def _synth(path):
    """A synthetic take with the events baked in: two clean phrases, a
    laugh (quavering voiced burst), a breath gap, a cough (sharp transient),
    a hum at 100 Hz under quiet noise, and a voiced tail with real decay."""
    rate = RATE
    total = []
    tone = lambda f, t, a=0.4: [a * math.sin(2 * math.pi * f * i / rate) for i in range(int(t * rate))]
    quiet = lambda t, a=0.002: [(a * (0.5 + 0.5 * ((i % 97) / 97.0))) * (1 if (i * 7919) % 3 else -1)
                                for i in range(int(t * rate))]
    # phrase 1: 180 Hz, amplitude-modulated
    ph = tone(180.0, 1.2, 0.4)
    ph = [0.4 * (0.7 + 0.3 * math.sin(2 * math.pi * 3 * i / rate)) * math.sin(2 * math.pi * 180 * i / rate)
          for i in range(len(ph))]
    total += ph
    total += quiet(0.25, 0.004)  # breath-ish gap
    # laugh: 4.5 Hz quaver on 220 Hz
    for k in range(5):
        total += [0.3 * math.sin(2 * math.pi * 220 * i / rate) for i in range(int(0.11 * rate))]
        total += [0.0] * int(0.11 * rate)
    total += quiet(0.3, 0.003)
    # phrase 2
    ph2 = [0.35 * (0.7 + 0.3 * math.sin(2 * math.pi * 2.5 * i / rate)) * math.sin(2 * math.pi * 150 * i / rate)
           for i in range(int(1.1 * rate))]
    total += ph2
    # cough: sharp decaying noise burst x2
    for k in range(2):
        burst = [0.8 * math.exp(-i / (0.01 * rate)) * (1 if (i * 104729) % 5 else -1)
                 for i in range(int(0.12 * rate))]
        total += burst
        total += quiet(0.18, 0.002)
    # hum + quiet tail with decay (reverb-ish)
    tail = [0.05 * math.sin(2 * math.pi * 100 * i / rate) +
            0.02 * math.exp(-i / (0.5 * rate)) * math.sin(2 * math.pi * 160 * i / rate)
            for i in range(int(1.2 * rate))]
    total += tail
    with wave.open(path, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes(b"".join(
            int(max(-1.0, min(1.0, s)) * 32000).to_bytes(2, "little", signed=True)
            for s in total))
    return len(total) / rate


def selftest(only=None):
    import tempfile
    checks = []
    with tempfile.TemporaryDirectory(prefix="voiceq-") as tmp:
        wav = str(Path(tmp) / "take.wav")
        duration = _synth(wav)
        audio_spec = {"audio": wav}
        for name, handler in TOOLS.items():
            if only and name != only:
                continue
            # detect_restarts / classify_filler_type need words, not raw audio
            spec = dict(audio_spec)
            if name in ("detect_restarts", "classify_filler_type"):
                spec = {"transcript": {"segments": [
                    {"text": "So the the first thing is, um, we look at the data and",
                     "start": 0.1, "end": 1.3,
                     "words": [{"text": "so", "start": 0.10, "end": 0.22},
                               {"text": "the", "start": 0.25, "end": 0.33},
                               {"text": "the", "start": 0.35, "end": 0.43},
                               {"text": "first", "start": 0.45, "end": 0.62},
                               {"text": "thing", "start": 0.64, "end": 0.85},
                               {"text": "is,", "start": 0.87, "end": 1.02},
                               {"text": "um,", "start": 1.05, "end": 1.18},
                               {"text": "we", "start": 1.19, "end": 1.26}]},
                    {"text": "And basically the numbers basically tell the story here.",
                     "start": 1.6, "end": 2.6,
                     "words": [{"text": "and", "start": 1.60, "end": 1.72},
                               {"text": "basically", "start": 1.75, "end": 2.10},
                               {"text": "the", "start": 2.12, "end": 2.20},
                               {"text": "numbers", "start": 2.22, "end": 2.50},
                               {"text": "tell", "start": 2.52, "end": 2.60}]},
                ]}}
            try:
                out = handler(spec)
                checks.append({"tool": name, "ok": bool(out.get("ok")),
                               "error": out.get("error")})
            except Exception as exc:  # noqa: BLE001
                checks.append({"tool": name, "ok": False,
                               "error": f"{type(exc).__name__}: {exc}"})
    failed = [c for c in checks if not c.get("ok")]
    return {
        "ok": not failed,
        "schema": SCHEMA,
        "action": "selftest",
        "syntheticSeconds": round(duration, 2),
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
    parser = argparse.ArgumentParser(description="The voice itself, measured.")
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
