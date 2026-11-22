#!/usr/bin/env python3
"""concat-audio-repair: the take cleaned, levelled, and the music read.

Pure Python standard library, same contract as the other sense scripts:
`audio_repair.py <extract|selftest> --spec -` reads one JSON spec from
stdin and prints one JSON object on stdout. The spec names the tool in
`tool`.

THE EIGHT TOOLS THIS MODULE SERVES

  enhance_speech         the measured voice chain: highpass at the rumble,
                         de-esser, compressor, gate at the floor, limiter,
                         loudness to target - every decision measured from
                         THIS file, written to a new one
  remove_hiss_hum_rumble  find the hum (50/60 Hz + harmonics) and notch
                         it, cut the rumble, denoise the hiss - and prove
                         each removal with a before/after measurement
  diagnose_take          the repair decision measured before it is made:
                         floor, SNR, hum, hiss, rumble, DC offset,
                         clipping, flatness, rolloff, sibilance, loudness
                         - one read, and the ordered shopping list of
                         repairs the numbers justify
  repair_by_diagnosis    re-measure (never trust a stale diagnosis), then
                         apply exactly the chain the measurements ordered
                         - DC blocker, hum notches, rumble door, de-esser,
                         presence lift or denoise, limiter - and decline
                         when nothing fired
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

The four writing tools never touch the input: the output lands in `out`
(default: beside the input, with a suffix). Everything degrades loudly.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
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


def _encode_args(out_path, chain):
    """The output arguments for the file the caller named.

    A video container keeps its video stream and takes AAC. A lossy
    audio format takes a codec its own muxer accepts - a pcm_s16le stream
    into an .mp3 is a refusal ("could not find tag for codec"), which
    used to cost the whole repair on any non-wav take. Everything else -
    .wav, .flac, a name with no extension - is PCM.
    """
    suffix = Path(str(out_path)).suffix.lower()
    if suffix in _CONTAINERS:
        return ["-map", "0:v", "-map", "0:a", "-c:v", "copy", "-af", chain,
                "-c:a", "aac", "-b:a", "192k"]
    if suffix == ".mp3":
        return ["-af", chain, "-c:a", "libmp3lame", "-b:a", "192k"]
    if suffix in (".m4a", ".aac"):
        return ["-af", chain, "-c:a", "aac", "-b:a", "192k"]
    if suffix in (".ogg", ".oga", ".opus"):
        return ["-af", chain, "-c:a", "libopus", "-b:a", "128k"]
    return ["-af", chain, "-c:a", "pcm_s16le"]


def _write_atomic(ff, args, out_path):
    """Run one ffmpeg write so the target is never a half-file: the encode
    lands on a .part sibling - named `<stem>.part<suffix>` so the decoder
    still reads the format off the extension - and replaces the target
    only when it finished. A crash, a full disk or a killed timeout used
    to leave a truncated take that the run then went on transcribing."""
    target = Path(str(out_path))
    part = target.with_name(target.stem + ".part" + target.suffix)
    good, why = _run_ffmpeg(ff, args + [str(part)])
    if not good:
        try:
            os.remove(part)
        except OSError:
            pass
        return False, why
    os.replace(part, target)
    return True, None


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


def _source_rate_args(ffprobe, audio):
    """Pin the output sample rate to the source's own, as `-ar`.

    Without it every PCM/lossy write lands at ffmpeg's default 48 kHz -
    and after a loudnorm pass the filter graph renegotiates to 192 kHz,
    so a 10-minute 48 kHz take comes back as a ~460 MB 192 kHz file the
    timeline then has to carry. A probe that fails changes nothing: the
    default is the old behaviour, not a refusal."""
    try:
        rc, out, _ = _vs._run(
            [ffprobe, "-v", "error", "-select_streams", "a:0",
             "-show_entries", "stream=sample_rate", "-of", "csv=p=0", str(audio)], 20)
        if rc == 0:
            rate = int(out.decode("utf-8", "replace").strip().splitlines()[0])
            if 8000 <= rate <= 384000:
                return ["-ar", str(rate)]
    except (OSError, ValueError, IndexError):
        pass
    return []


def _needs_ffmpeg(spec):
    """The writing tools filter through ffmpeg; without a decoder they
    refuse cleanly, naming the way to one, instead of dying on a missing
    binary. The reading tools decode WAV natively and never get here."""
    ff, _ = _vs._ffmpeg(spec)
    rc, _, _ = _vs._run([ff, "-hide_banner", "-version"], 10)
    if rc != 0:
        return {"ok": False,
                "error": "no ffmpeg on this machine: set FUNCUT_FFMPEG to a decoder "
                         "(or install one) - the writing tools filter through it"}
    return None




def _output_path(spec, source, suffix):
    """Where a writing tool lands its file: the caller's `out`, else beside
    the input. The input itself is never the target - enforced here, not
    just promised: a relative `out` naming the source, or one climbing
    with `..`, used to reach _write_atomic unchecked (the Rust door's
    confinement only inspects absolute paths), and `overwrite: true`
    then replaced the very take the caller asked to repair."""
    out = spec.get("out")
    if out:
        out = str(out)
        parts = Path(out).parts
        if ".." in parts:
            raise ValueError(
                f"the write path may not climb with '..': {out}")
        try:
            if os.path.realpath(out) == os.path.realpath(str(source)):
                raise ValueError(
                    "the input itself is never the write target: pass a "
                    "new `out` (or drop it to land beside the input)")
        except OSError:
            pass
        return out, bool(spec.get("overwrite"))
    p = Path(str(source))
    return str(p.with_name(p.stem + suffix + p.suffix)), False


def _window_peaks(env):
    """The 50 ms window maxima of the envelope, sorted quietest to
    loudest: the one instrument every floor and speech-level read in
    this module goes through. 50 ms because a 250 ms max-window cannot
    see a gap shorter than itself, and a gated/TTS take's
    tenth-of-a-second breaths measured its softest SPEECH as the floor -
    the before/after tables were one instrument in name only until the
    window shrank to what a breath actually is."""
    win_frames = 5
    peaks = []
    for i in range(0, len(env) - win_frames, win_frames):
        peaks.append(max(env[i:i + win_frames]))
    peaks.sort()
    return peaks


def _measure_snr(spec, path):
    """Speech-ref minus floor, the same reading classify_recording_quality
    uses, so the before/after table is one instrument throughout."""
    samples, _ = _vq._pcm(spec, path)
    if samples is None or not len(samples):
        return None
    windows = _window_peaks(_vq._envelope(samples))
    if not windows:
        return None
    floor = _vq._db(windows[int(0.15 * len(windows))])
    speech = _vq._db(windows[int(0.9 * len(windows))] if len(windows) > 5 else windows[-1])
    return round(speech - floor, 1)


# The three noise readings - hum, hiss, rumble - are one instrument
# shared by the generic clean (section 2) and the diagnosis (section 3):
# a repair decided from numbers the clean measured differently would be
# two opinions wearing one name, and the caller could not tell which
# one lied.


def _quietest_windows(env):
    """The frame indices of the quietest tenth of the 50 ms windows (at
    least three): where the noise lives when the voice pauses. Frame
    indices, not times, so a caller can aim a spectrum or a Goertzel
    probe straight at the span."""
    win_frames = 5
    windows = []
    for i in range(0, len(env) - win_frames, win_frames):
        windows.append((max(env[i:i + win_frames]), i))
    windows.sort(key=lambda w: w[0])
    return [i for _, i in windows[:max(3, len(windows) // 10)]]


def _hum_reading(samples, env):
    """The mains hum, as the notch chain spends it: the base frequency
    (50 or 60 Hz) and its harmonic peaks, or (None, []) when no family
    earned its notches. A Goertzel pass at the exact mains candidates
    over the quietest half-second windows; a tone 6 dB or more over its
    guards counts, and a family needs two of its harmonics before it
    earns its notch chain. The old bin-truncated FFT probe could not
    resolve 50 from 60 Hz in a 64 ms window and never found a hum at
    all - the notch chain it fed was dead code."""
    hum_base = None
    hum_peaks = []
    for i in _quietest_windows(env)[:3]:
        centre = i * _vq.HOP + _vq.WIN // 2 - _vq.HUM_WIN // 2
        found_any = False
        for base in (50.0, 60.0):
            found = []
            for harmonic in range(1, 5):
                hz = base * harmonic
                tone = _vq._goertzel_power(samples, centre, _vq.HUM_WIN, hz)
                low = _vq._goertzel_power(samples, centre, _vq.HUM_WIN, hz - 12.0)
                high = _vq._goertzel_power(samples, centre, _vq.HUM_WIN, hz + 12.0)
                if tone is None or low is None or high is None:
                    continue
                guard = (low + high) / 2.0 + 1e-12
                # The probe answers POWER: 6 dB of power is a ratio of
                # ~4.0, not 2.0 (a power ratio of 2 is 3 dB - the old
                # threshold notched borderline tones at half the head
                # the comment claimed).
                if tone / guard > 4.0:
                    found.append(round(hz))
            if len(found) >= 2:
                hum_base = base
                hum_peaks = found
                found_any = True
                break
        if found_any:
            break
    return hum_base, hum_peaks


def _hiss_reading(samples, quietest_idx):
    """The hiss: high-band energy (6-7.5 kHz) against the speech band
    (0.3-3 kHz) in the quiet windows, in dB. A quiet window can be EXACT
    digital silence (a gated TTS take, a paused recording): a zero
    spectrum is a truthy list of zeros, so a plain emptiness test never
    skips it, and a zero speech band turns the ratio into a
    ZeroDivisionError that killed the repair stage it fed. Silence
    carries no hiss evidence - skipped here, and never divided by
    zero."""
    highs, mids = [], []
    for i in quietest_idx[:4]:
        mags = _vq._spectrum(samples, i * _vq.HOP)
        if not mags or sum(mags) < 1e-9:
            continue
        highs.append(_vq._band_energy(mags, 6000, 7500))
        mids.append(_vq._band_energy(mags, 300, 3000))
    if highs and mids and sum(mids) > 0:
        return _vq._db(sum(highs) / sum(mids))
    return None


def _rumble_reading(samples, env):
    """The rumble: sub-60 Hz energy against the whole band in the
    LOUDEST windows, in dB - rumble is read where the take itself is
    loud, because a quiet window's sub-bass is the room breathing, not
    the recording's problem."""
    low, all_e = [], []
    for i in sorted(range(len(env)), key=lambda j: -env[j])[:4]:
        mags = _vq._spectrum(samples, i * _vq.HOP)
        if not mags or sum(mags) < 1e-9:
            continue
        low.append(_vq._band_energy(mags, 20, 60))
        all_e.append(_vq._band_energy(mags, 20, 7500))
    if low and all_e and sum(all_e) > 0:
        return _vq._db(sum(low) / sum(all_e))
    return None


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
    if (refusal := _needs_ffmpeg(spec)):
        return refusal
    ff, _ = _vs._ffmpeg(spec)
    samples, _ = _vq._pcm(spec, audio)
    if samples is None or not len(samples):
        return {"ok": False, "error": f"could not decode {audio}"}
    env = _vq._envelope(samples)

    # Every parameter measured from THIS take, at the same 50 ms
    # window the classifier reads its floor with - a coarser window
    # here put the gate threshold (floor + 8 dB) up at soft-speech
    # level on a gated take, and agate ate the quiet words the chain
    # was supposed to preserve.
    windows = _window_peaks(env)
    floor_db = _vq._db(windows[int(0.15 * len(windows))]) if windows else -60.0
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
    _, probe = _vs._ffmpeg(spec)
    args = (["-i", str(audio)] + _encode_args(out_path, chain)
            + _source_rate_args(probe, audio))
    good, why = _write_atomic(ff, args, out_path)
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
    if (refusal := _needs_ffmpeg(spec)):
        return refusal
    ff, _ = _vs._ffmpeg(spec)
    samples, _ = _vq._pcm(spec, audio)
    if samples is None or not len(samples):
        return {"ok": False, "error": f"could not decode {audio}"}
    env = _vq._envelope(samples)

    # Where the quiet lives: the windows to read the noise from, at
    # the same 50 ms granularity the classifier's floor uses.
    quietest_idx = _quietest_windows(env)

    # The three noise readings, each now the one shared instrument the
    # diagnosis tools read too: a repair chain and a diagnosis that
    # measured hum, hiss or rumble differently would be two opinions
    # wearing one name, and the caller could not tell which one lied.
    hum_base, hum_peaks = _hum_reading(samples, env)
    hiss_db = _hiss_reading(samples, quietest_idx)
    rumble_db = _rumble_reading(samples, env)

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
    _, probe = _vs._ffmpeg(spec)
    args = (["-i", str(audio)] + _encode_args(out_path, chain)
            + _source_rate_args(probe, audio))
    good, why = _write_atomic(ff, args, out_path)
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
# 3. diagnose_take
# ---------------------------------------------------------------------------

# The thresholds that turn a measurement into a repair. Each one is the
# point where the symptom starts costing more than the cure: below it a
# filter's own artefacts (notch ring, denoise swirl, a presence lift on
# a take that already has presence) cost more than what they remove.
# The selftest's justification gate reads the same constants, so a
# threshold that drifts fails the gate instead of quietly notching
# clean takes.
_DC_TRIGGER = 0.02           # of full scale: tilts every floor after it
_RUMBLE_TRIGGER_DB = -32.0   # dB of sub-60 Hz energy in the loudest windows
_HISS_TRIGGER_DB = -18.0     # dB, high band over speech band, in the quiet
_SIBILANCE_TRIGGER = 0.15    # of total energy in the 4-8 kHz band
_ROLLOFF_TRIGGER_HZ = 5000.0  # Hz: under it (with low flatness) = muffled
_FLATNESS_TRIGGER = 0.25     # above it a denoise fits; below, a notch does
_CLIPPING_TRIGGER = 0.001    # of samples at the rail


def _averaged_spectrum(samples, windows=8):
    """One magnitude spectrum averaged over windows spread through the
    file: the diagnosis's single view of the take's shape. Spread, not
    stacked at the start - a take's first phrase is not the whole take,
    and a diagnosis that only heard the intro would name the intro's
    problems as the file's."""
    if len(samples) < _vq.WIN:
        return None
    acc = None
    used = 0
    for k in range(windows):
        centre = int((k + 0.5) * len(samples) / windows)
        mags = _vq._spectrum(samples, centre - _vq.WIN // 2)
        if not mags:
            continue
        if acc is None:
            acc = [0.0] * len(mags)
        if len(mags) != len(acc):
            continue
        used += 1
        for j, m in enumerate(mags):
            acc[j] += m
    if not used:
        return None
    return [m / used for m in acc]


def _take_diagnosis(spec, ff, audio):
    """Every forensic number the repair decision spends, measured from
    THIS take in one pass: (diagnosis, error). The values are rounded
    before the plan reads them, so the reply a caller inspects is
    exactly what the repairs were built from - a threshold shown as
    passed never sits beside a repair list that disagrees with it."""
    samples, _ = _vq._pcm(spec, audio)
    if samples is None or not len(samples):
        return None, f"could not decode {audio}"
    env = _vq._envelope(samples)

    # The floor: the 15th-percentile 50 ms window, the one instrument
    # the gate threshold and the SNR table already read.
    peaks = _window_peaks(env)
    floor_db = _vq._db(peaks[int(0.15 * len(peaks))]) if peaks else -60.0

    # The tone family and the two broadband noises, each read by the
    # same helper the generic clean trusts - the diagnosis and the
    # generic chain must agree on what hum, hiss and rumble ARE.
    hum_base, hum_peaks = _hum_reading(samples, env)
    hiss_db = _hiss_reading(samples, _quietest_windows(env))
    rumble_db = _rumble_reading(samples, env)

    # DC and clipping in one walk over the samples. The DC blocker sits
    # FIRST in the repair order precisely because a bias tilts the floor
    # every other reading here stands on.
    clip_level = 0.999 * 32768.0
    total = 0
    clipped = 0
    for s in samples:
        total += s
        if s >= clip_level or s <= -clip_level:
            clipped += 1
    n = max(1, len(samples))
    dc_offset = abs(total) / n / 32768.0
    clipping_ratio = clipped / n

    # Flatness, rolloff and sibilance all read ONE averaged spectrum:
    # three views of a single instrument, not three instruments that
    # can disagree about the same take.
    flatness = rolloff_hz = sibilance = None
    avg = _averaged_spectrum(samples)
    if avg:
        # Flatness, 0..1: geometric mean over arithmetic mean. A pure
        # tone reads ~0 (all its energy in a few bins), white noise
        # approaches 1 - the number that says denoise (broadband) or
        # notch (tonal). eps keeps one silent bin's log(0) from killing
        # the reading.
        eps = 1e-12
        geo = math.exp(sum(math.log(m + eps) for m in avg) / len(avg))
        flatness = geo / (sum(avg) / len(avg) + eps)
        energy = [m * m for m in avg]
        total_e = sum(energy)
        if total_e > 0:
            per_bin = RATE / _vq.WIN
            # Rolloff at 85%, the same fraction the Rust forensic ear
            # uses: 95% chases the noise floor past 10 kHz on every
            # take and never separates a muffled voice from a bright
            # one.
            acc = 0.0
            rolloff_hz = (len(energy) - 1) * per_bin
            for k, e in enumerate(energy):
                acc += e
                if acc >= 0.85 * total_e:
                    rolloff_hz = k * per_bin
                    break
            lo = int(4000.0 / per_bin)
            hi = min(len(energy) - 1, int(8000.0 / per_bin))
            sibilance = sum(energy[lo:hi + 1]) / total_e if hi > lo else 0.0

    snr = _measure_snr(spec, audio)
    integrated, _lra, tp = _vs._integrated(ff, audio)

    return {
        "noiseFloorDbfs": round(floor_db, 1),
        "snrDb": snr,
        "humHz": hum_base,
        "humPeaks": hum_peaks,
        "hissDb": round(hiss_db, 1) if hiss_db is not None else None,
        "rumbleDb": round(rumble_db, 1) if rumble_db is not None else None,
        "dcOffset": round(dc_offset, 5),
        "clippingRatio": round(clipping_ratio, 6),
        "spectralFlatness": (round(flatness, 4)
                             if flatness is not None else None),
        "spectralRolloffHz": (round(rolloff_hz, 1)
                              if rolloff_hz is not None else None),
        "sibilanceRatio": (round(sibilance, 4)
                           if sibilance is not None else None),
        "integratedLufs": integrated,
        "truePeakDbfs": tp,
    }, None


def _repair_plan(diag):
    """The ordered shopping list: every repair the measurements justify,
    in the order the chain must apply them. The order is not cosmetic -
    the DC blocker comes first because a biased take measures every
    other arm against a tilted floor, the notches come before any
    denoise because a spectral denoise smears the very tones a notch
    takes cleanly, and the limiter comes last because it polices the
    gain the stages before it added."""
    plan = []
    dc = diag.get("dcOffset")
    if dc is not None and dc >= _DC_TRIGGER:
        plan.append({
            "found": "DC offset",
            "fix": "highpass at 10 Hz (the DC blocker)",
            "why": "a highpass has zero response at DC, and the bias "
                   "tilts the floor every later stage measures against",
            "measure": f"{dc:.4f} of full scale",
            "filters": ["highpass=f=10"],
        })
    hum_hz = diag.get("humHz")
    if hum_hz:
        peaks = (diag.get("humPeaks") or [])[:4]
        plan.append({
            "found": f"hum at {hum_hz:.0f} Hz + harmonics",
            "fix": "14 dB notches, Q 8",
            "why": "mains tones standing 6 dB over their neighbourhood: "
                   "a narrow notch takes the tone and leaves the voice "
                   "around it",
            "measure": f"{peaks} Hz standing 6 dB over their neighbourhood",
            "filters": [f"equalizer=frequency={hz}:t=q:w=8:g=-14"
                        for hz in peaks],
        })
    rumble = diag.get("rumbleDb")
    if rumble is not None and rumble > _RUMBLE_TRIGGER_DB:
        plan.append({
            "found": "rumble",
            "fix": "highpass at 70 Hz",
            "why": "sub-60 Hz energy the voice does not live in; the "
                   "door at 70 Hz keeps the chest out of the mud",
            "measure": f"{rumble:.1f} dB of sub-60 Hz energy",
            "filters": ["highpass=f=70"],
        })
    sib = diag.get("sibilanceRatio")
    if sib is not None and sib > _SIBILANCE_TRIGGER:
        plan.append({
            "found": "sibilance",
            "fix": "deesser",
            "why": "a 4-8 kHz share this large rides over the "
                   "consonants; taming it before any gain keeps the "
                   "chain from pumping on it",
            "measure": f"{sib:.2f} of the energy in 4-8 kHz",
            "filters": ["deesser=i=0.5:m=0.5:f=0.55"],
        })
    rolloff = diag.get("spectralRolloffHz")
    flat = diag.get("spectralFlatness")
    muffled = (rolloff is not None and flat is not None
               and rolloff < _ROLLOFF_TRIGGER_HZ and flat < _FLATNESS_TRIGGER)
    if muffled:
        plan.append({
            "found": "a muffled take",
            "fix": "presence lift at 3 kHz",
            "why": "the rolloff never reached the presence band and the "
                   "spectrum is tonal, not flat: this take is muffled, "
                   "not noisy, and the clarity a presence lift restores "
                   "is what a denoise would only deepen",
            "measure": f"rolloff {rolloff:.0f} Hz, flatness {flat:.2f}",
            "filters": ["equalizer=frequency=3000:t=q:w=2:g=3"],
        })
    hiss = diag.get("hissDb")
    # The flatness gate IS the muffled skip: a muffled take is flatness
    # under 0.25 by definition, so this arm cannot fire on one. It also
    # refuses a tonal hiss (a narrow whistle) the way the Rust forensic
    # ear does - afftdn would smear it, and the notch arm above is the
    # tool for tones.
    if (hiss is not None and hiss > _HISS_TRIGGER_DB
            and flat is not None and flat >= _FLATNESS_TRIGGER):
        strength = min(24, max(12, int(-hiss)))
        plan.append({
            "found": "hiss",
            "fix": f"afftdn nr={strength}",
            "why": "broadband high-band noise over the speech band, on "
                   "a spectrum flat enough that a spectral denoise "
                   "fits it",
            "measure": f"high band {hiss:.1f} dB over the speech band in quiet",
            "filters": [f"afftdn=nr={strength}:nf=-{max(30, int(-hiss) + 6)}"],
        })
    clip = diag.get("clippingRatio")
    if clip is not None and clip > _CLIPPING_TRIGGER:
        plan.append({
            "found": "clipping",
            "fix": "limiter at 0.989",
            "why": "samples at the rail: the ceiling stops the gain the "
                   "stages above added from re-clipping what was "
                   "already clipped",
            "measure": f"{clip:.4f} of samples at full scale",
            "filters": ["alimiter=limit=0.989:level=false"],
        })
    return plan


def _diagnose_take(spec):
    audio = spec.get("audio") or spec.get("path")
    if not audio or not Path(str(audio)).is_file():
        return {"ok": False, "error": f"no such audio file: {audio}"}
    ff, _ = _vs._ffmpeg(spec)
    diag, err = _take_diagnosis(spec, ff, audio)
    if err:
        return {"ok": False, "error": err}
    plan = _repair_plan(diag)
    return _ok(
        "diagnose_take",
        audio=audio,
        diagnosis=diag,
        repairs=[{"fix": e["fix"], "why": e["why"], "measure": e["measure"]}
                 for e in plan],
        verdict="repair" if plan else "clean",
        note=(None if diag.get("integratedLufs") is not None else
              "loudness figures absent: no ffmpeg to run ebur128 - every "
              "other number above is the decode's own"),
        source="python (50 ms windows, Goertzel, FFT) + ffmpeg ebur128",
    )


# ---------------------------------------------------------------------------
# 4. repair_by_diagnosis
# ---------------------------------------------------------------------------


def _repair_by_diagnosis(spec):
    audio = spec.get("audio") or spec.get("path")
    if not audio or not Path(str(audio)).is_file():
        return {"ok": False, "error": f"no such audio file: {audio}"}
    if (refusal := _needs_ffmpeg(spec)):
        return refusal
    ff, _ = _vs._ffmpeg(spec)
    # Re-measured, never trusted: a diagnosis carried across a re-encode
    # or a retry names the last take's problems, and the chain would
    # then repair what is no longer there. The numbers below are THIS
    # decode's, measured in the same call that spends them.
    diag, err = _take_diagnosis(spec, ff, audio)
    if err:
        return {"ok": False, "error": err}
    plan = _repair_plan(diag)
    if not plan:
        # Nothing fired: no file, the same convention the generic clean
        # keeps - a filter the take did not ask for costs artefacts it
        # did not owe. force still writes the conservative door, because
        # a caller who insists gets the one filter no voice misses.
        if not spec.get("force"):
            return _ok(
                "repair_by_diagnosis",
                file=None,
                findings=_negative_findings(diag),
                diagnosis=diag,
                note="nothing worth repairing: every arm measured under "
                     "its threshold, and a cure without a symptom costs "
                     "artefacts. Pass force: true to clean anyway (the "
                     "conservative 60 Hz highpass).",
                forced=False,
                source="python (50 ms windows, Goertzel, FFT) + ffmpeg (declined)",
            )
        filters = ["highpass=f=60"]
        findings = _negative_findings(diag)
    else:
        # The chain in the order the shopping list says: DC blocker,
        # notches, rumble door, de-esser, presence-or-denoise, limiter.
        filters = [f for entry in plan for f in entry["filters"]]
        findings = [{"found": e["found"], "measure": e["measure"],
                     "fix": e["fix"]} for e in plan]
    out_path, overwrite = _output_path(spec, audio, "_repaired")
    if Path(out_path).exists() and not overwrite:
        return {"ok": False, "error": f"{out_path} exists; pass overwrite: true or a new out"}
    _, probe = _vs._ffmpeg(spec)
    args = (["-i", str(audio)] + _encode_args(out_path, ",".join(filters))
            + _source_rate_args(probe, audio))
    good, why = _write_atomic(ff, args, out_path)
    if not good:
        return {"ok": False, "error": f"the repair chain failed: {why}"}
    snr_before = _measure_snr(spec, audio)
    snr_after = _measure_snr(spec, out_path)
    return _ok(
        "repair_by_diagnosis",
        file=out_path,
        findings=findings,
        filterChain=filters,
        before={"snrDb": snr_before},
        after={"snrDb": snr_after},
        diagnosis=diag,
        note="the input file was not touched",
        source="python (the re-measured diagnosis) + ffmpeg (the chain it ordered)",
    )


def _negative_findings(diag):
    """The declined reply's evidence: one row per arm, every one under
    its threshold, so the refusal is a measured fact and not a shrug -
    the same convention the generic clean's declined reply keeps. The
    rumble row names the conservative door force would open, which is
    the only filter a clean take can be owed."""
    rumble = diag.get("rumbleDb")
    hiss = diag.get("hissDb")
    rolloff = diag.get("spectralRolloffHz")
    return [
        {"found": "no DC offset",
         "measure": f"{diag.get('dcOffset')} of full scale",
         "fix": "nothing"},
        {"found": "no hum",
         "measure": "no mains family with two harmonics over its guards",
         "fix": "nothing"},
        {"found": "no significant rumble",
         "measure": (f"{rumble} dB of sub-60 Hz energy"
                     if rumble is not None else "n/a"),
         "fix": "conservative 60 Hz highpass"},
        {"found": "no sibilance problem",
         "measure": f"{diag.get('sibilanceRatio')} of the energy in 4-8 kHz",
         "fix": "nothing"},
        {"found": "not muffled",
         "measure": (f"rolloff {rolloff} Hz, flatness "
                     f"{diag.get('spectralFlatness')}"
                     if rolloff is not None else "n/a"),
         "fix": "nothing"},
        {"found": "no significant hiss",
         "measure": f"{hiss} dB" if hiss is not None else "n/a",
         "fix": "nothing"},
        {"found": "no clipping",
         "measure": f"{diag.get('clippingRatio')} of samples at the rail",
         "fix": "nothing"},
    ]


# ---------------------------------------------------------------------------
# 5. normalize_loudness
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
    if (refusal := _needs_ffmpeg(spec)):
        return refusal
    ff, probe = _vs._ffmpeg(spec)

    platform = str(spec.get("platform") or "").lower()
    if platform and platform in _PLATFORMS:
        target_i, target_tp = _PLATFORMS[platform]
    else:
        # targetLufs 0 is a legitimate (if loud) request - `or` would
        # silently swap it for the default.
        target_i = (float(spec["targetLufs"]) if spec.get("targetLufs") is not None
                    else -14.0)
        target_tp = (float(spec["targetTruePeakDbtp"])
                     if spec.get("targetTruePeakDbtp") is not None else -1.0)

    # Pass one: what the take is now.
    integrated, lra, tp = _vs._integrated(ff, audio)
    if integrated is None:
        return {"ok": False, "error": "no audio stream to measure"}

    # Linear when the take is close enough for it; dynamic when it is not
    # (loudnorm's own rule: linear only when within range, and LRA 1-50).
    linear = abs(target_i - integrated) <= 12 and (lra is None or lra <= 20)
    lra_for_filter = max(1.0, min(lra or 7, 20)) if linear else 7
    # linear=true with NO measured_* values is a lie loudnorm silently
    # ignores: it falls back to dynamic mode while the reply claims "one
    # gain, no dynamics touched". Hand it the take's own measurements so
    # the claim is the behaviour.
    measured = (f":measured_I={integrated:.2f}:measured_LRA={lra_for_filter:.2f}"
                f":measured_TP={tp if tp is not None else -1.0:.2f}"
                f":measured_thresh={integrated - lra_for_filter:.2f}") if linear else ""
    chain = (f"loudnorm=I={target_i}:TP={target_tp}:LRA={lra_for_filter}"
             f":linear={'true' if linear else 'false'}{measured}")

    out_path, overwrite = _output_path(spec, audio, f"_{int(target_i)}lufs")
    if Path(out_path).exists() and not overwrite:
        return {"ok": False, "error": f"{out_path} exists; pass overwrite: true or a new out"}
    # The codec the output's own container accepts (_encode_args), the
    # source's own sample rate pinned (-ar, so a loudnorm pass cannot
    # renegotiate to 192 kHz), written so the target is never a half
    # file (_write_atomic) - the same triple every writing path here
    # shares.
    args = (["-i", str(audio)] + _encode_args(out_path, chain)
            + _source_rate_args(probe, audio))
    good, why = _write_atomic(ff, args, out_path)
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
# 6. detect_music_segments
# ---------------------------------------------------------------------------


def _detect_music_segments(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    # The path this read was asked about, echoed back so the reader that
    # pairs `musicPresent` replies with takes keys the pairing on the
    # answer itself: a call that failed leaves no reply behind, and a
    # pairing by call order would then name the wrong take as the one
    # that already carries music - and the bed decision for the real
    # take with it.
    audio = spec.get("audio") or spec.get("path")
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
        # j walks env-frame indices (100 fps), 25 frames - 250 ms -
        # apart: i, i+25, i+50, i+75, four spectra inside the span.
        # The old stop (`i + step - _vq.HOP`) subtracted a SAMPLE count
        # from a FRAME count and landed under the start, so the range
        # was always empty, every span's centroidCv was None, and the
        # classifier's stability arms never measured anything.
        for j in range(i, min(i + step, len(env)), 25):
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
            # A beat the 1-second window can see AND a locked timbre:
            # the per-second ear's own clear music reading.
            cls = "music"
        elif duty > 0.85:
            # Continuous energy the per-second ear cannot settle: a beat
            # slower than ~125 BPM is invisible to a 1-second span's
            # autocorrelation, and a gated-TTS take is just as
            # continuous - the whole take's tempo (below) is the
            # authority this span honestly defers to. The old arm also
            # demanded a wandering centroid, but the number had never
            # been measured (the loop above was dead) and its 0.6
            # threshold sits far above anything real audio reads
            # (0.04-0.2 on the fixtures) - the arm never fired on
            # anything, and every dense take classified as plain speech.
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
    # The gate's word against the tempo's. A "music_or_dense_speech" span
    # is the classifier saying it cannot tell - and a gated TTS take lands
    # there every time: continuous energy, a wandering centroid, no beat.
    # The whole take's onset autocorrelation settles it, the same number
    # identify_music reports as tempo.strength: music holds a pulse the
    # autocorrelation can find (>= 0.1); dense speech does not. The reply
    # records which evidence decided, so a caller can tell a measured
    # false positive from a clean negative.
    dense = any(s["class"] == "music_or_dense_speech" for s in spans)
    clear_music = any(s["class"] == "music" for s in spans)
    whole_bpm, whole_strength = _tempo_autocorr(curve) if len(curve) >= 100 else (None, 0.0)
    if clear_music:
        has_music = True
        evidence = "gate"
    elif dense and whole_strength >= 0.1:
        has_music = True
        evidence = "gate+tempo"
    elif dense:
        has_music = False
        evidence = "gate_false_positive"
    else:
        has_music = False
        evidence = "gate"
    return _ok(
        "detect_music_segments",
        audio=audio,
        duration=round(duration, 2),
        segments=merged,
        coverageSeconds={k: round(v * step / 100.0, 1) for k, v in coverage.items()},
        musicPresent=has_music,
        evidence=evidence,
        tempoStrength=whole_strength,
        editingAnswer=(
            "this take already carries music: do NOT double the bed - duck or replace, never stack"
            if has_music else
            (f"the dense spans are dense speech, not music - the take's beat strength "
             f"({whole_strength}) is under the 0.1 a pulse needs: a BGM bed is safe"
             if evidence == "gate_false_positive" else
             "no music in this take: a BGM bed is safe - start it under the first speech, "
             "duck 10-14 dB while words run")),
        directorHints=[
            "silence spans longer than 2 s are either breathing room or a cut candidate - check the pause classes",
            "a music_or_dense_speech span needs the emotion read to settle which it is",
        ] + (["the dense spans' pulse was measured too weak for music - the bed "
              "decision stands on that measurement"]
             if evidence == "gate_false_positive" else []),
        source="python (duty cycle, onset autocorrelation beat strength, centroid stability)",
    )


# ---------------------------------------------------------------------------
# 7. identify_music
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
# 8. analyze_music_structure
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
    "diagnose_take": _diagnose_take,
    "repair_by_diagnosis": _repair_by_diagnosis,
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


def _synth_dense_speech(path):
    """Dense speech: short phrases butted together with no gaps, pitch
    leaping between them and gliding inside them - the gated-TTS pattern
    whose continuous energy used to read as music_or_dense_speech and
    gate the BGM decision as music. Lengths, pitches, glides and syllable
    rates come from a deterministic scramble, so nothing the tempo
    autocorrelation can search is a period of the whole."""
    import wave as _wave
    rate = RATE
    total = []
    state = 987654321

    def rnd():
        nonlocal state
        state = (state * 1103515245 + 12345) % (1 << 31)
        return state / (1 << 31)

    for _ in range(110):
        # Short phrases: a one-second window spans two or three, each at
        # a very different pitch - that is what drives the centroid
        # variation past the music classifier's stability floor. No
        # syllabic amplitude ride: a gated TTS take is continuous energy,
        # and a periodic ride is exactly what an autocorrelation reads
        # as a beat. What moves is the pitch - leaping between phrases,
        # gliding inside them - under a deterministic noise floor.
        n = int((0.28 + 0.32 * rnd()) * rate)
        f0 = 110.0 + 300.0 * rnd()
        glide = (rnd() - 0.5) * 90.0
        gain = 0.7 + 0.3 * rnd()
        for i in range(n):
            t = i / rate
            carrier = f0 + glide * t
            v = gain * (0.4 * math.sin(2 * math.pi * carrier * t) +
                        0.25 * math.sin(6 * math.pi * carrier * t) +
                        0.1 * math.sin(10 * math.pi * carrier * t))
            noise = (((i * 2654435761) % 2000) / 1000.0 - 1.0) * 0.09
            total.append(0.5 * v + noise)
    with _wave.open(path, "wb") as h:
        h.setnchannels(1)
        h.setsampwidth(2)
        h.setframerate(rate)
        h.writeframes(b"".join(int(max(-1, min(1, s)) * 32000).to_bytes(2, "little", signed=True)
                               for s in total))
    return len(total) / rate


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


def _filter_justified(f, diag):
    """The selftest's own reading of the repair contract: every filter
    in a repair_by_diagnosis chain must trace back to the measurement
    that fired it. Deliberately written apart from _repair_plan (which
    it must agree with), so a plan that drifts from the thresholds
    fails here instead of agreeing with itself. The conservative 60 Hz
    door is the one exception: by convention it rides only on a forced
    clean, and no measurement fires it."""
    flat = diag.get("spectralFlatness")
    rolloff = diag.get("spectralRolloffHz")
    muffled = (rolloff is not None and flat is not None
               and rolloff < _ROLLOFF_TRIGGER_HZ and flat < _FLATNESS_TRIGGER)
    if f == "highpass=f=10":
        dc = diag.get("dcOffset")
        return dc is not None and dc >= _DC_TRIGGER
    if f.startswith("equalizer=frequency=") and f.endswith(":t=q:w=8:g=-14"):
        hz = int(f.split("frequency=")[1].split(":")[0])
        return bool(diag.get("humHz")) and hz in (diag.get("humPeaks") or [])
    if f == "highpass=f=70":
        rumble = diag.get("rumbleDb")
        return rumble is not None and rumble > _RUMBLE_TRIGGER_DB
    if f == "highpass=f=60":
        # the conservative door, convention not measurement
        return True
    if f == "deesser=i=0.5:m=0.5:f=0.55":
        sib = diag.get("sibilanceRatio")
        return sib is not None and sib > _SIBILANCE_TRIGGER
    if f == "equalizer=frequency=3000:t=q:w=2:g=3":
        return muffled
    if f.startswith("afftdn="):
        hiss = diag.get("hissDb")
        return (hiss is not None and hiss > _HISS_TRIGGER_DB
                and flat is not None and flat >= _FLATNESS_TRIGGER)
    if f == "alimiter=limit=0.989:level=false":
        clip = diag.get("clippingRatio")
        return clip is not None and clip > _CLIPPING_TRIGGER
    return False


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
            "diagnose_take": {"audio": speech},
            "repair_by_diagnosis": {"audio": speech,
                                    "out": str(Path(tmp) / "repaired.wav")},
            "normalize_loudness": {"audio": speech, "out": str(Path(tmp) / "norm.wav"),
                                   "targetLufs": -18},
            "detect_music_segments": {"audio": music},
            "identify_music": {"audio": music},
            "analyze_music_structure": {"audio": music},
        }
        # The four writers filter through ffmpeg; without a decoder they
        # must refuse cleanly, naming the way to one - that refusal is
        # what the selftest proves on a machine with no ffmpeg, beside
        # the music reads, which decode WAV natively and prove fully.
        writers = ("enhance_speech", "remove_hiss_hum_rumble",
                   "repair_by_diagnosis", "normalize_loudness")
        no_ffmpeg = _needs_ffmpeg({}) is not None
        for name, handler in TOOLS.items():
            if only and name != only:
                continue
            spec = plans.get(name) or {"audio": speech}
            try:
                out = handler(spec)
                extra = None
                if name in writers:
                    if no_ffmpeg:
                        refused_cleanly = (not out.get("ok")
                                           and "FUNCUT_FFMPEG" in str(out.get("error") or ""))
                        extra = "clean refusal (no ffmpeg)" if refused_cleanly else "no file!"
                        checks.append({"tool": name, "ok": refused_cleanly,
                                       "error": out.get("error"), "extra": extra})
                        continue
                    made = out.get("file")
                    extra = "file written" if (made and Path(str(made)).is_file()) else "no file!"
                checks.append({"tool": name, "ok": bool(out.get("ok")) and extra != "no file!",
                               "error": out.get("error"), "extra": extra})
            except Exception as exc:  # noqa: BLE001
                checks.append({"tool": name, "ok": False,
                               "error": f"{type(exc).__name__}: {exc}"})
        # The hum the synthetic take baked in must be FOUND, not only
        # survived: the detection feeds the notch chain, and a detector
        # that never fires writes a clean bill of health over a humming
        # take (the bin-truncated probe did exactly that). Provable only
        # where ffmpeg could run the chain.
        if not no_ffmpeg:
            hum_read = _remove_hiss_hum_rumble(
                {"audio": humming, "out": str(Path(tmp) / "clean2.wav"),
                 "overwrite": True})
            found_hum = any("hum at 50 Hz" in str(finding.get("found") or "")
                            for finding in (hum_read.get("findings") or []))
            checks.append({
                "tool": "remove_hiss_hum_rumble (hum found)",
                "ok": bool(hum_read.get("ok")) and found_hum,
                "error": hum_read.get("error"),
                "extra": "the baked 50 Hz hum is detected and notched",
            })
            # A non-WAV audio hand-in must come back in its own
            # container: the old chain wrote pcm_s16le into whatever
            # suffix the input had, and the mp3 muxer refused it - the
            # writing tools only ever worked on .wav and video. An
            # encoder the build lacks is a skip, not a failure.
            mp3_read = _enhance_speech(
                {"audio": speech, "out": str(Path(tmp) / "enhanced.mp3"),
                 "overwrite": True})
            mp3_made = bool(mp3_read.get("ok")) and Path(str(mp3_read.get("file") or "")).is_file()
            encoder_absent = "encoder" in str(mp3_read.get("error") or "").lower()
            checks.append({
                "tool": "enhance_speech (mp3 out)",
                "ok": mp3_made or encoder_absent,
                "error": None if (mp3_made or encoder_absent) else mp3_read.get("error"),
                "extra": ("the mp3 container keeps an mp3 payload" if mp3_made
                          else ("libmp3lame absent here, skipped" if encoder_absent
                                else "no file")),
            })
        # The signal-selected repair's own proof, beside the loop above.
        # The diagnosis is the half the auto edit used to skip: it must
        # read every key the decision spends, and its verdict must agree
        # with its own shopping list - a "clean" that names repairs, or
        # a "repair" that names none, is a decision the caller cannot
        # trust. The humming fixture is the whole point of the exercise:
        # the baked 50 Hz hum must be NAMED in the plan, instead of the
        # generic chain running whether or not anything hummed. Both
        # reads decode WAV natively, so they prove with or without a
        # decoder - only the loudness keys ride ebur128.
        diag_read = TOOLS["diagnose_take"]({"audio": speech})
        diag = diag_read.get("diagnosis") or {}
        repairs = diag_read.get("repairs") or []
        verdict = diag_read.get("verdict")
        keys = ("noiseFloorDbfs", "snrDb", "humHz", "humPeaks", "hissDb",
                "rumbleDb", "dcOffset", "clippingRatio", "spectralFlatness",
                "spectralRolloffHz", "sibilanceRatio", "integratedLufs",
                "truePeakDbfs")
        checks.append({
            "tool": "diagnose_take (speech)",
            "ok": (bool(diag_read.get("ok"))
                   and all(k in diag for k in keys)
                   and verdict in ("clean", "repair")
                   and (verdict == "repair") == bool(repairs)),
            "error": diag_read.get("error"),
            "extra": (f"verdict={verdict}, "
                      f"repairs={[r.get('fix') for r in repairs]}"),
        })
        hum_diag = TOOLS["diagnose_take"]({"audio": humming})
        hd = hum_diag.get("diagnosis") or {}
        hum_repairs = hum_diag.get("repairs") or []
        checks.append({
            "tool": "diagnose_take (hum fixture)",
            "ok": (bool(hum_diag.get("ok"))
                   and hd.get("humHz") == 50
                   and any("notch" in str(r.get("fix") or "").lower()
                           for r in hum_repairs)),
            "error": hum_diag.get("error"),
            "extra": (f"humHz={hd.get('humHz')}, peaks={hd.get('humPeaks')}, "
                      f"repairs={[r.get('fix') for r in hum_repairs]}"),
        })
        # The writing half needs the decoder: the chain it writes must
        # carry only stages the measurements fired (the whole contract
        # of a signal-selected repair), the muffled lift and the denoise
        # must never share a chain (a denoise deepens a muffled take),
        # and the humming take's chain must carry the 50 Hz notch its
        # diagnosis named.
        if not no_ffmpeg:
            rep_read = TOOLS["repair_by_diagnosis"](
                {"audio": speech, "out": str(Path(tmp) / "repaired2.wav"),
                 "overwrite": True})
            chain = rep_read.get("filterChain") or []
            if rep_read.get("ok") and rep_read.get("file"):
                made = (Path(str(rep_read["file"])).is_file()
                        and Path(str(rep_read["file"])).stat().st_size > 0)
                justified = all(_filter_justified(
                    f, rep_read.get("diagnosis") or {}) for f in chain)
                exclusive = not ("equalizer=frequency=3000:t=q:w=2:g=3" in chain
                                 and any(f.startswith("afftdn=") for f in chain))
            elif rep_read.get("ok"):
                # A clean take declines with no file: the honest reply.
                made = justified = exclusive = True
            else:
                made = justified = exclusive = False
            checks.append({
                "tool": "repair_by_diagnosis (speech)",
                "ok": (bool(rep_read.get("ok")) and made and justified
                       and exclusive),
                "error": rep_read.get("error"),
                "extra": f"chain={chain}",
            })
            hum_rep = TOOLS["repair_by_diagnosis"](
                {"audio": humming, "out": str(Path(tmp) / "hum-repaired.wav"),
                 "overwrite": True})
            hr_chain = hum_rep.get("filterChain") or []
            hr_file = hum_rep.get("file")
            checks.append({
                "tool": "repair_by_diagnosis (hum fixture)",
                "ok": (bool(hum_rep.get("ok"))
                       and bool(hr_file) and Path(str(hr_file)).is_file()
                       and any(f.startswith("equalizer=frequency=50:")
                               for f in hr_chain)
                       and all(_filter_justified(
                           f, hum_rep.get("diagnosis") or {})
                           for f in hr_chain)),
                "error": hum_rep.get("error"),
                "extra": f"chain={hr_chain}",
            })
        # The gate's two directions, proven beyond the loop above: a
        # dense-speech take (the gated-TTS pattern) must NOT gate as
        # music - the tempo veto names it a false positive - and a take
        # with a real beat grid must.
        dense = str(Path(tmp) / "dense.wav")
        _synth_dense_speech(dense)
        dense_gate = _detect_music_segments({"audio": dense})
        checks.append({
            "tool": "detect_music_segments (dense speech)",
            "ok": bool(dense_gate.get("ok")) and dense_gate.get("musicPresent") is False
                  and dense_gate.get("evidence") == "gate_false_positive"
                  # The path asked about, echoed back: the AUTO EDIT pairs
                  # the reply with its take by this key, so a reply that
                  # stops carrying it fails here instead of pairing the
                  # takes by call order again.
                  and dense_gate.get("audio") == dense,
            "error": dense_gate.get("error"),
            "extra": f"evidence={dense_gate.get('evidence')} "
                     f"tempoStrength={dense_gate.get('tempoStrength')}",
        })
        music_gate = _detect_music_segments({"audio": music})
        checks.append({
            "tool": "detect_music_segments (real music)",
            "ok": bool(music_gate.get("ok")) and music_gate.get("musicPresent") is True,
            "error": music_gate.get("error"),
            "extra": f"evidence={music_gate.get('evidence')} "
                     f"tempoStrength={music_gate.get('tempoStrength')}",
        })
        # The AUTO EDIT's own chain, end to end, on a take with EXACT-zero
        # gaps (the gated-TTS pattern the fixture above carries): enhance
        # to a copy, then clean ONTO the take in place. A quiet window of
        # digital silence used to divide by zero in the hiss scan and the
        # whole chain died before any write - this check is the proof it
        # lands, stays the same duration, and keeps the source rate
        # (no silent 192 kHz renegotiation).
        if not no_ffmpeg:
            import wave as _wave_check
            take = str(Path(tmp) / "take.wav")
            before_seconds = _synth_speech(take)
            enhanced = str(Path(tmp) / "take-enhanced.wav")
            step1 = TOOLS["enhance_speech"]({"audio": take, "out": enhanced,
                                             "overwrite": True})
            step2 = TOOLS["remove_hiss_hum_rumble"](
                {"audio": enhanced, "out": take, "overwrite": True, "force": True})
            landed = (bool(step1.get("ok")) and bool(step2.get("ok"))
                      and Path(take).is_file())
            duration_ok = rate_ok = False
            if landed:
                with _wave_check.open(take, "rb") as h:
                    frames, rate = h.getnframes(), h.getframerate()
                    duration_ok = abs(frames / rate - before_seconds) < 0.05
                    # the fixture is 16 kHz; 192 kHz is the renegotiation bug
                    rate_ok = rate == RATE
            checks.append({
                "tool": "the AUTO EDIT chain (enhance -> clean in place)",
                "ok": landed and duration_ok and rate_ok,
                "error": (step1.get("error") or step2.get("error"))
                         if not landed else None,
                "extra": (f"duration kept: {duration_ok}, rate {RATE} Hz kept: {rate_ok}"),
            })
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
