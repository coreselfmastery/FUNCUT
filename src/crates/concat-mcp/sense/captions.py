#!/usr/bin/env python3
"""concat-captions: the words and the take, made to agree.

Pure Python standard library, same contract as the other sense scripts:
`captions.py <extract|selftest> --spec -` reads one JSON spec from stdin
and prints one JSON object on stdout. The spec names the tool in `tool`.

THE FIVE TOOLS THIS MODULE SERVES

  evaluate_tts_quality   is this TTS take alive: pitch variance, timing
                         regularity, pause placement, emphasis spread -
                         the naturalness score and the regeneration
                         settings that fix what it measured
  detect_ai_voice        the honest synthetic-media heuristic: too-clean
                         floor, metronomic timing, missing breaths, absent
                         jitter - a disclosure score, never a verdict
  seal_voice             the provenance and permission record: hash the
                         take, freeze the voiceprint, write the usage
                         policy beside it, verify it later - tamper-evident
  align_captions         re-time caption cues to the audio: drift
                         correction against the energy onsets, reading-
                         speed budgets, boundaries snapped to silence
  clean_subtitles        subtitle hygiene: punctuation, casing, filler
                         stripping, merge/split by characters-per-line and
                         reading speed, overlap and gap repairs

Every tool says what it measured and what it changed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import sys
from datetime import datetime, timezone
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

SCHEMA = "funcut.captions/1"


def _ok(tool, **fields):
    out = {"ok": True, "schema": SCHEMA, "tool": tool}
    out.update(fields)
    return out


def _words_all(spec):
    lines = _vs._collect_lines(spec)
    words = []
    for idx, line in enumerate(lines):
        for w in _vs._words_of(line):
            if w.get("start") is not None and w.get("end") is not None:
                w = dict(w)
                w["line"] = idx
                words.append(w)
    return lines, words


# ---------------------------------------------------------------------------
# 1. evaluate_tts_quality
# ---------------------------------------------------------------------------


def _evaluate_tts_quality(spec):
    audio = spec.get("audio") or spec.get("path")
    lines, words = _words_all(spec)
    if not audio or not Path(str(audio)).is_file():
        return {"ok": False, "error": "pass the TTS take in `audio` (with its transcript for timings)"}
    if len(words) < 10:
        return {"ok": False, "error": "word timings are needed (10+ words): pass the transcript the TTS gave"}
    samples, _ = _vq._pcm(spec, audio)
    if samples is None:
        return {"ok": False, "error": f"could not decode {audio}"}
    env = _vq._envelope(samples)
    track = _vq._f0_track(samples, env)

    # 1. Pitch variance: semitone range and CV over voiced frames.
    f0s = [t[1] for t in track]
    if len(f0s) >= 5:
        f0_mean = sum(f0s) / len(f0s)
        semis = 12 * math.log2(max(f0s) / min(min(f0s), f0_mean))
        semis_down = 12 * math.log2(f0_mean / max(min(f0s), 1e-3))
        range_semis = 12 * math.log2(max(f0s) / max(min(f0s), 1e-3))
        f0_cv = (math.sqrt(sum((f - f0_mean) ** 2 for f in f0s) / len(f0s)) / f0_mean)
    else:
        f0_mean, range_semis, f0_cv = None, None, None

    # 2. Timing regularity: CV of inter-word gaps inside lines.
    gaps = []
    for i in range(1, len(words)):
        if words[i].get("line") == words[i - 1].get("line"):
            g = words[i]["start"] - words[i - 1]["end"]
            if g >= 0:
                gaps.append(g)
    gap_cv = None
    if len(gaps) >= 5:
        gm = sum(gaps) / len(gaps)
        gap_cv = math.sqrt(sum((g - gm) ** 2 for g in gaps) / len(gaps)) / (gm or 1e-6)

    # 3. Pause placement: commas and periods should carry gaps.
    placed = total_punct = 0
    for i in range(1, len(words)):
        if words[i].get("line") != words[i - 1].get("line"):
            continue
        prev_text = str(words[i - 1].get("text") or "")
        if prev_text.rstrip().endswith((",", ";", ":", ".", "!", "?")):
            total_punct += 1
            if words[i]["start"] - words[i - 1]["end"] >= 0.06:
                placed += 1
    pause_placement = round(placed / total_punct, 2) if total_punct >= 3 else None

    # 4. Emphasis spread: energy variance across words (dB range of the
    # per-word mean envelope).
    word_amps = []
    for w in words:
        lo = max(0, int(w["start"] * 100))
        hi = min(len(env), int(w["end"] * 100))
        seg = env[lo:hi]
        if seg:
            word_amps.append(sum(seg) / len(seg))
    emph_db = round(_vq._db(max(word_amps)) - _vq._db(min(word_amps)), 1) if len(word_amps) >= 5 else None

    cues = []
    score = 50.0
    if range_semis is not None:
        if range_semis < 3.5:
            cues.append(f"pitch range only {range_semis:.1f} semitones - monotone")
        else:
            score += 15
    if f0_cv is not None:
        if f0_cv < 0.04:
            cues.append(f"pitch variability {f0_cv:.3f} - too even to be alive")
        else:
            score += 10
    if gap_cv is not None:
        if gap_cv < 0.25:
            cues.append(f"word-gap variability {gap_cv:.2f} - metronomic")
        else:
            score += 15
    if pause_placement is not None:
        if pause_placement < 0.5:
            cues.append(f"only {int(pause_placement * 100)}% of punctuation carried a pause")
        else:
            score += 10
    if emph_db is not None:
        if emph_db < 6:
            cues.append(f"word-level dynamics only {emph_db} dB - no emphasis shape")
        else:
            score += 10

    fixes = []
    if range_semis is not None and range_semis < 3.5:
        fixes.append("regenerate with a voice or setting that varies pitch, or record a human for the hook")
    if gap_cv is not None and gap_cv < 0.25:
        fixes.append("edit the script's punctuation: commas force the engine to breathe")
    if emph_db is not None and emph_db < 6:
        fixes.append("emphasis needs SSML prosody marks or post EQ + level automation")
    verdict = ("alive" if score >= 80 and not cues else
               "workable" if score >= 60 else "robotic")
    return _ok(
        "evaluate_tts_quality",
        naturalness={"score": int(min(100, score)), "verdict": verdict},
        measures={
            "f0MeanHz": round(f0_mean, 1) if f0_mean else None,
            "pitchRangeSemitones": round(range_semis, 1) if range_semis is not None else None,
            "pitchCv": round(f0_cv, 3) if f0_cv is not None else None,
            "wordGapCv": round(gap_cv, 3) if gap_cv is not None else None,
            "pausePlacement": pause_placement,
            "wordDynamicsDb": emph_db,
        },
        roboticCues=cues,
        fixes=fixes or ["the take reads naturally: keep it"],
        directorHints=[
            "a robotic take undercuts emotion-led cuts: at minimum human-record the hook",
            "TTS takes often want the pause classes faked in the edit - hold frames where a human would breathe",
        ] if verdict != "alive" else ["natural enough: treat it like a human take"],
        source="python (sparse F0 track, word-gap stats, punctuation-pause alignment)",
    )


# ---------------------------------------------------------------------------
# 2. detect_ai_voice
# ---------------------------------------------------------------------------


def _detect_ai_voice(spec):
    audio = spec.get("audio") or spec.get("path")
    if not audio or not Path(str(audio)).is_file():
        return {"ok": False, "error": f"no such audio file: {audio}"}
    samples, _ = _vq._pcm(spec, audio)
    if samples is None:
        return {"ok": False, "error": f"could not decode {audio}"}
    env = _vq._envelope(samples)
    lines, words = _words_all(spec)

    # 1. The floor: too clean is itself a cue.
    win_frames = 25
    windows = []
    for i in range(0, len(env) - win_frames, win_frames):
        windows.append(max(env[i:i + win_frames]))
    windows.sort()
    floor_db = _vq._db(windows[int(0.15 * len(windows))]) if windows else None

    # 2. Breaths: short low-energy spans between phrase runs.
    breaths = 0
    in_gap = False
    for i in range(1, len(env)):
        quiet = env[i] < 0.015
        if quiet and not in_gap and env[i - 1] >= 0.015:
            in_gap = True
        elif not quiet and in_gap:
            in_gap = False
            breaths += 1
    span_min = max(1, len(env) // 100)
    breaths_per_min = 60.0 * breaths / span_min

    # 3. Jitter: natural voices carry period variability.
    track = _vq._f0_track(samples, env)
    periods = [1.0 / t[1] for t in track]
    jitters = []
    for i in range(1, len(periods)):
        if abs(track[i][0] - track[i - 1][0]) < 0.06:
            jitters.append(abs(periods[i] - periods[i - 1]) / max(periods[i - 1], 1e-6))
    jitter = (sum(jitters) / len(jitters)) if jitters else None

    # 4. Timing regularity (when words are given).
    gap_cv = None
    if len(words) >= 10:
        gaps = [words[i]["start"] - words[i - 1]["end"]
                for i in range(1, len(words))
                if words[i].get("line") == words[i - 1].get("line")
                and words[i]["start"] >= words[i - 1]["end"]]
        if len(gaps) >= 5:
            gm = sum(gaps) / len(gaps)
            gap_cv = math.sqrt(sum((g - gm) ** 2 for g in gaps) / len(gaps)) / (gm or 1e-6)

    score = 0.0
    cues = []
    if floor_db is not None and floor_db < -65:
        score += 25
        cues.append(f"noise floor {floor_db:.0f} dBFS: no room, no mic self-noise")
    if breaths_per_min < 2:
        score += 25
        cues.append(f"{breaths_per_min:.1f} breaths/min: nobody is breathing")
    if jitter is not None and jitter < 0.008:
        score += 25
        cues.append(f"period jitter {jitter:.4f}: natural voices carry more")
    if gap_cv is not None and gap_cv < 0.2:
        score += 25
        cues.append(f"word-gap variability {gap_cv:.2f}: machine-regular")
    if jitter is not None and jitter > 0.02 and breaths_per_min > 4:
        cues.append(f"jitter {jitter:.3f} and {breaths_per_min:.0f} breaths/min: human markers present")
        score -= 15
    score = max(0.0, min(100.0, score + 10))  # a prior, not an accusation
    band = ("likely synthetic" if score >= 70 else
            "uncertain" if score >= 40 else "likely human")
    return _ok(
        "detect_ai_voice",
        aiLikeness={"score": int(score), "band": band},
        cues=cues,
        measures={
            "noiseFloorDbfs": round(floor_db, 1) if floor_db is not None else None,
            "breathsPerMinute": round(breaths_per_min, 1),
            "jitterProxy": round(jitter, 4) if jitter is not None else None,
            "wordGapCv": round(gap_cv, 3) if gap_cv is not None else None,
        },
        purpose="disclosure: label synthetic voices honestly. This is a heuristic "
                "that modern TTS can pass and noisy humans can fail - it informs "
                "disclosure, it never proves anything about a person",
        directorHints=[
            "if this take is synthetic, disclose it - platforms and audiences both ask now",
            "pair a synthetic narration with human-recorded hooks to keep the feel alive",
        ],
        source="python (floor, breath scan, jitter proxy, gap regularity)",
    )


# ---------------------------------------------------------------------------
# 3. seal_voice
# ---------------------------------------------------------------------------

SEAL_NAME = "voice-seal.json"


def _voiceprint(spec, audio):
    """The short acoustic fingerprint that rides in the seal: enough to
    recognise the same voice again, never enough to synthesise it."""
    samples, _ = _vq._pcm(spec, audio)
    if samples is None:
        return None
    env = _vq._envelope(samples)
    track = _vq._f0_track(samples, env)
    f0s = [t[1] for t in track]
    return {
        "f0MeanHz": round(sum(f0s) / len(f0s), 1) if f0s else None,
        "f0SpreadHz": round(max(f0s) - min(f0s), 1) if f0s else None,
        "voicedFraction": round(len(track) / max(1, len(env)), 3),
        "durationSeconds": round(len(samples) / _vq.RATE, 2),
    }


def _seal_voice(spec):
    action = str(spec.get("action") or "seal")
    audio = spec.get("audio") or spec.get("path")

    if action == "status":
        if not audio:
            return {"ok": False, "error": "pass the sealed audio in `audio`"}
        seal_file = Path(str(audio)).with_name(SEAL_NAME)
        if not seal_file.is_file():
            return _ok("seal_voice", sealed=False,
                       sealFile=str(seal_file),
                       note="no seal beside this audio")
        seal = json.loads(seal_file.read_text())
        return _ok("seal_voice", sealed=True, sealFile=str(seal_file), seal=seal)

    if action in ("seal", "verify"):
        if not audio or not Path(str(audio)).is_file():
            return {"ok": False, "error": f"no such audio file: {audio}"}
        digest = hashlib.sha256()
        size = 0
        with open(audio, "rb") as handle:
            for block in iter(lambda: handle.read(1 << 20), b""):
                digest.update(block)
                size += len(block)
        sha = digest.hexdigest()

        seal_file = Path(str(audio)).with_name(SEAL_NAME)
        if action == "verify":
            if not seal_file.is_file():
                return _ok("seal_voice", verified=False, intact=None,
                           error="no seal to verify against - seal first",
                           note="run action 'seal' once, then verify")
            try:
                seal = json.loads(seal_file.read_text())
            except ValueError:
                return {"ok": False, "error": f"{seal_file} is not valid JSON"}
            intact = (seal.get("sha256") == sha and seal.get("bytes") == size)
            expired = False
            if seal.get("policy", {}).get("expiry"):
                try:
                    expired = seal["policy"]["expiry"] < datetime.now(timezone.utc).date().isoformat()
                except ValueError:
                    expired = None
            return _ok(
                "seal_voice", verified=True, intact=intact,
                sealFile=str(seal_file),
                policy=seal.get("policy"),
                policyExpired=expired,
                note=("the bytes match the seal" if intact else
                      "THE AUDIO NO LONGER MATCHES THE SEAL: it was edited, "
                      "re-encoded or replaced since sealing"),
            )

        # action == seal
        policy_in = spec.get("policy") or {}
        allowed = policy_in.get("allowedUses")
        policy = {
            "allowedUses": allowed or ["editing in this project"],
            "expiry": policy_in.get("expiry") or None,
            "attribution": policy_in.get("attribution") or None,
            "license": policy_in.get("license") or "unspecified",
            "syntheticVoiceDisclosure": policy_in.get("syntheticVoiceDisclosure", None),
        }
        seal = {
            "schema": SCHEMA,
            "sealedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "file": Path(str(audio)).name,
            "bytes": size,
            "sha256": sha,
            "voiceprint": _voiceprint(spec, audio),
            "policy": policy,
        }
        out_path = spec.get("out") or str(seal_file)
        if Path(out_path).exists() and not spec.get("overwrite"):
            return {"ok": False, "error": f"{out_path} exists; pass overwrite: true or a new out"}
        Path(out_path).write_text(json.dumps(seal, indent=1))
        return _ok(
            "seal_voice", sealed=True, sealFile=str(out_path),
            sha256=sha, bytes=size, policy=policy,
            verifyCommand={"tool": "seal_voice", "action": "verify", "audio": audio},
            note="the seal records what this take IS and what it may be used for; "
                 "any edit to the audio breaks it, by design",
        )
    return {"ok": False, "error": "action is seal, verify or status"}


# ---------------------------------------------------------------------------
# 4. align_captions
# ---------------------------------------------------------------------------


def _align_captions(spec):
    audio = spec.get("audio") or spec.get("path")
    lines, words = _words_all(spec)
    if not words:
        return {"ok": False, "error": "word timings needed: pass the transcript (with words) or lines with words"}
    samples, _ = (_vq._pcm(spec, audio) if audio else (None, None))
    env = _vq._envelope(samples) if samples is not None else None

    # 1. Drift: median offset from each word start to the nearest envelope
    # onset. A consistent offset is clock drift; snap it out.
    drift = None
    snapped = 0
    if env:
        onsets = []
        above = False
        for i in range(1, len(env)):
            now = env[i] > 0.02
            if now and not above and env[i] > env[i - 1] * 1.4:
                onsets.append(i / 100.0)
            above = now
        offsets = []
        for w in words[:: max(1, len(words) // 60)]:
            if not onsets:
                break
            nearest = min(onsets, key=lambda t: abs(t - w["start"]))
            delta = w["start"] - nearest
            if abs(delta) < 0.25:
                offsets.append(delta)
        if len(offsets) >= 8:
            offsets.sort()
            median = offsets[len(offsets) // 2]
            spread = offsets[int(0.9 * len(offsets))] - offsets[int(0.1 * len(offsets))]
            if abs(median) > 0.05 and spread < 0.18:
                drift = round(median, 3)
    if drift:
        for w in words:
            w["start"] = max(0.0, w["start"] - drift)
            w["end"] = max(0.0, w["end"] - drift)

    # 2. Cue building: group words under reading-speed budgets.
    max_cps = float(spec.get("maxCharsPerSecond") or 17)
    max_dur = float(spec.get("maxDurationSeconds") or 5.0)
    min_dur = float(spec.get("minDurationSeconds") or 0.9)
    max_chars = int(spec.get("maxCharsPerLine") or 42)
    cues = []
    cue_words = []
    cue_text = 0

    def flush():
        nonlocal cue_words, cue_text
        if not cue_words:
            return
        text = " ".join(str(w.get("text") or "").strip() for w in cue_words)
        start = cue_words[0]["start"]
        end = cue_words[-1]["end"]
        dur = max(min_dur, end - start + 0.15)
        cps = len(text) / dur
        if cps > max_cps:
            dur = len(text) / max_cps
            end = start + dur
        cues.append({"text": text, "start": round(start, 3), "end": round(end, 3),
                     "cps": round(cps, 1)})
        cue_words, cue_text = [], 0

    for w in words:
        token = str(w.get("text") or "").strip()
        ends_sentence = token.rstrip().endswith((".", "!", "?"))
        big_gap = (cue_words and w["start"] - cue_words[-1]["end"] > 0.7)
        if cue_words and (cue_text + len(token) + 1 > max_chars
                          or w["end"] - cue_words[0]["start"] > max_dur
                          or ends_sentence or big_gap):
            flush()
        cue_words.append(w)
        cue_text += len(token) + 1
    flush()

    # 3. Snap cue boundaries to silence when a gap is close by.
    if env:
        silences = []
        quiet = False
        for i in range(1, len(env)):
            now = env[i] < 0.015
            if now and not quiet:
                quiet = True
            elif not quiet is False and quiet and not now:
                quiet = False
            if now and not quiet:
                quiet = True
        # simpler: collect quiet spans directly
        silences = []
        q_start = None
        for i, v in enumerate(env):
            if v < 0.015:
                if q_start is None:
                    q_start = i / 100.0
            else:
                if q_start is not None:
                    if i / 100.0 - q_start >= 0.12:
                        silences.append((q_start, i / 100.0))
                    q_start = None
        if q_start is not None and len(env) / 100.0 - q_start >= 0.12:
            silences.append((q_start, len(env) / 100.0))
        for cue in cues:
            for lo, hi in silences:
                if 0 < cue["start"] - hi < 0.3:
                    cue["start"] = round(hi, 3)
                    snapped += 1
                    break
            for lo, hi in silences:
                if 0 < lo - cue["end"] < 0.3:
                    cue["end"] = round(lo, 3)
                    snapped += 1
                    break
    # no overlaps, no negative spans
    for i in range(1, len(cues)):
        if cues[i]["start"] < cues[i - 1]["end"]:
            cues[i]["start"] = cues[i - 1]["end"]
    return _ok(
        "align_captions",
        cues=cues,
        driftSeconds=drift,
        snappedBoundaries=snapped,
        budgets={"maxCharsPerSecond": max_cps, "maxDurationSeconds": max_dur,
                 "minDurationSeconds": min_dur, "maxCharsPerLine": max_chars},
        captionReady={"note": "the cues fit captions_add: [{text, start, end}]",
                      "count": len(cues)},
        source="python (word-onset drift fit, reading-speed cue grouping, silence snapping)",
    )


# ---------------------------------------------------------------------------
# 5. clean_subtitles
# ---------------------------------------------------------------------------

_SRT_BLOCK = re.compile(
    r"(\d+)\s*\n(\d{2}:\d{2}:\d{2}[,.]\d{3})\s*-->\s*(\d{2}:\d{2}:\d{2}[,.]\d{3})\s*\n(.*?)(?=\n\s*\n|\Z)",
    re.S)


def _srt_time(text):
    h, m, rest = text.replace(",", ".").split(":")
    return int(h) * 3600 + int(m) * 60 + float(rest)


def _to_srt_time(seconds):
    seconds = max(0.0, seconds)
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = seconds % 60
    return f"{h:02d}:{m:02d}:{s:06.3f}".replace(".", ",")


def _clean_subtitles(spec):
    # Input: an SRT string, or transcript/lines.
    srt = spec.get("srt")
    cues = []
    if srt:
        for num, start, end, body in _SRT_BLOCK.findall(str(srt)):
            cues.append({"start": _srt_time(start), "end": _srt_time(end),
                         "text": body.replace("\n", " ").strip()})
    if not cues:
        lines = _vs._collect_lines(spec)
        if not lines and not srt:
            return {"ok": False, "error": "no subtitles: pass srt (raw SRT text), transcript, lines or script"}
        for line in lines:
            if line.get("start") is not None:
                cues.append({"start": float(line["start"]),
                             "end": float(line.get("end") or line["start"] + 2.0),
                             "text": str(line.get("text") or "").strip()})
            else:
                cues.append({"start": None, "end": None,
                             "text": str(line.get("text") or "").strip()})
    strip_fillers = bool(spec.get("stripFillers", True))
    max_chars = int(spec.get("maxCharsPerLine") or 42)
    max_cps = float(spec.get("maxCharsPerSecond") or 17)

    changes = {"punctuationFixed": 0, "casingFixed": 0, "fillersStripped": 0,
               "duplicatesMerged": 0, "split": 0, "merged": 0,
               "overlapsFixed": 0, "gapsFixed": 0}
    cleaned = []
    for cue in cues:
        text = re.sub(r"\s+", " ", cue["text"]).strip()
        # fillers out
        if strip_fillers:
            before = text
            text = re.sub(r"\b(um+|uh+|erm+|uhm+)\b[,]?\s*", "", text, flags=re.I)
            changes["fillersStripped"] += len(re.findall(r"\b(um+|uh+|erm+|uhm+)\b", before, re.I))
        # casing
        fixed = text[0].upper() + text[1:] if text and not text[0].isupper() else text
        if fixed != text:
            changes["casingFixed"] += 1
            text = fixed
        # punctuation: end a sentence
        if text and text[-1].isalnum():
            text += "."
            changes["punctuationFixed"] += 1
        cleaned.append({"start": cue["start"], "end": cue["end"], "text": text})

    # duplicates
    deduped = []
    for cue in cleaned:
        if deduped and cue["text"].lower() == deduped[-1]["text"].lower():
            deduped[-1]["end"] = cue["end"]
            changes["duplicatesMerged"] += 1
        else:
            deduped.append(cue)
    cleaned = deduped

    # split long, merge short
    final = []
    for cue in cleaned:
        if len(cue["text"]) <= max_chars:
            final.append(cue)
            continue
        words = cue["text"].split(" ")
        parts = []
        current = ""
        for w in words:
            if len(current) + len(w) + 1 > max_chars and current:
                parts.append(current.strip())
                current = w
            else:
                current = (current + " " + w).strip()
        if current:
            parts.append(current)
        if cue["start"] is None or len(parts) < 2:
            if len(parts) > 1:
                # no times: keep one cue, let align_captions time it
                cue["text"] = " ".join(parts)
                final.append(cue)
                changes["split"] += 1
            else:
                final.append(cue)
            continue
        span = max(0.3, (cue["end"] - cue["start"]) / len(parts))
        t = cue["start"]
        for part in parts:
            final.append({"start": round(t, 3), "end": round(t + span, 3), "text": part})
            t += span
        changes["split"] += 1
    cleaned = final

    merged_final = []
    for cue in cleaned:
        if (merged_final and cue["start"] is not None
                and merged_final[-1]["start"] is not None
                and len(merged_final[-1]["text"]) + len(cue["text"]) + 1 <= max_chars
                and cue["start"] - merged_final[-1]["end"] < 0.4
                and not merged_final[-1]["text"].rstrip().endswith((".", "!", "?"))):
            merged_final[-1]["text"] += " " + cue["text"]
            merged_final[-1]["end"] = cue["end"]
            changes["merged"] += 1
        else:
            merged_final.append(cue)
    cleaned = merged_final

    # timing repairs
    timed = [c for c in cleaned if c["start"] is not None]
    for i in range(1, len(timed)):
        if timed[i]["start"] < timed[i - 1]["end"] - 0.001:
            timed[i - 1]["end"] = timed[i]["start"]
            changes["overlapsFixed"] += 1
        gap = timed[i]["start"] - timed[i - 1]["end"]
        if 0 < gap < 0.04:
            timed[i - 1]["end"] = timed[i]["start"]
            changes["gapsFixed"] += 1
    # CPS enforcement: stretch end when text is dense
    for cue in timed:
        dur = cue["end"] - cue["start"]
        if dur > 0 and len(cue["text"]) / dur > max_cps:
            cue["end"] = round(cue["start"] + len(cue["text"]) / max_cps, 3)

    output_mode = str(spec.get("format") or "cues")
    payload = cleaned
    if output_mode == "srt":
        lines_out = []
        for n, cue in enumerate(cleaned, 1):
            if cue["start"] is None:
                cue["start"], cue["end"] = (n - 1) * 3.0, (n - 1) * 3.0 + 2.5
            lines_out.append(f"{n}\n{_to_srt_time(cue['start'])} --> {_to_srt_time(cue['end'])}\n{cue['text']}\n")
        payload = "\n".join(lines_out)
    return _ok(
        "clean_subtitles",
        cues=cleaned if output_mode != "srt" else None,
        srt=payload if output_mode == "srt" else None,
        changeLog=changes,
        counts={"in": len(cues) if 'cues' in dir() else len(cleaned), "out": len(cleaned)},
        budgets={"maxCharsPerLine": max_chars, "maxCharsPerSecond": max_cps,
                 "fillersStripped": strip_fillers},
        captionReady={"note": "the cues fit captions_add: [{text, start, end}]" if output_mode != "srt"
                      else "the SRT is ready to burn or import"},
        source="python (punctuation/casing/filler rules, merge-split budgets, overlap repair)",
    )


# ---------------------------------------------------------------------------
# Dispatch, selftest, entry
# ---------------------------------------------------------------------------

TOOLS = {
    "evaluate_tts_quality": _evaluate_tts_quality,
    "detect_ai_voice": _detect_ai_voice,
    "seal_voice": _seal_voice,
    "align_captions": _align_captions,
    "clean_subtitles": _clean_subtitles,
}


def _synth_tts(path, robotic=False):
    """A voiced take: robotic = metronomic timing, zero jitter, no breaths;
    alive = wandering pitch, breathing gaps, jitter."""
    import wave as _wave
    rate = _vq.RATE
    n = int(6 * rate)
    out = []
    phase = 0.0
    for i in range(n):
        t = i / rate
        f0 = 170.0 if robotic else 170.0 + 22.0 * math.sin(2 * math.pi * 0.7 * t) \
            + (6.0 * math.sin(2 * math.pi * 2.3 * t))
        if not robotic:
            f0 += 3.0 * math.sin(2 * math.pi * 13.7 * i / rate)
        phase += 2 * math.pi * f0 / rate
        env = (0.5 + 0.5 * math.sin(2 * math.pi * (3.0 if robotic else 2.1) * t)) * 0.45
        # breathing gaps for the alive take
        if not robotic and (t % 2.0) > 1.85:
            env = 0.0
        out.append(env * (0.6 * math.sin(phase) + 0.3 * math.sin(2 * phase)))
    with _wave.open(path, "wb") as h:
        h.setnchannels(1)
        h.setsampwidth(2)
        h.setframerate(rate)
        h.writeframes(b"".join(int(max(-1, min(1, s)) * 32000).to_bytes(2, "little", signed=True)
                               for s in out))
    return len(out) / rate


def _tts_transcript(robotic=False):
    words = []
    t = 0.15
    text = ["this", "is", "the", "voice", "that", "reads,", "and", "reads,",
            "and", "reads", "the", "numbers", "we", "need", "today",
            "so", "listen", "close", "and", "watch", "the", "story", "unfold"]
    for w in text:
        d = 0.16
        gap = 0.0 if robotic else (0.14 if w.endswith(",") else 0.05)
        words.append({"text": w, "start": round(t, 3), "end": round(t + d, 3)})
        t += d + (0.0 if robotic else gap)
    return {"segments": [{"text": " ".join(text), "start": 0.15,
                          "end": round(t, 3), "words": words}]}


def selftest(only=None):
    import tempfile
    checks = []
    with tempfile.TemporaryDirectory(prefix="captions-") as tmp:
        robotic = str(Path(tmp) / "robotic.wav")
        alive = str(Path(tmp) / "alive.wav")
        _synth_tts(robotic, robotic=True)
        _synth_tts(alive, robotic=False)
        plans = {
            "evaluate_tts_quality": {"audio": robotic, "transcript": _tts_transcript(True)},
            "detect_ai_voice": {"audio": robotic},
            "seal_voice": {"audio": robotic, "action": "seal",
                           "policy": {"allowedUses": ["youtube"], "expiry": "2027-01-01"}},
            "align_captions": {"audio": alive, "transcript": _tts_transcript(False)},
            "clean_subtitles": {"transcript": _tts_transcript(False)},
        }
        for name, handler in TOOLS.items():
            if only and name != only:
                continue
            try:
                out = handler(dict(plans.get(name) or {"audio": alive}))
                extra = None
                if name == "detect_ai_voice":
                    extra = f"score={out['aiLikeness']['score']} ({out['aiLikeness']['band']})"
                if name == "evaluate_tts_quality":
                    extra = f"score={out['naturalness']['score']} ({out['naturalness']['verdict']})"
                if name == "align_captions":
                    extra = f"{len(out['cues'])} cues"
                if name == "seal_voice":
                    extra = "sha256 sealed" if out.get("sealed") else str(out.get("error"))
                checks.append({"tool": name, "ok": bool(out.get("ok")),
                               "error": out.get("error"), "extra": extra})
            except Exception as exc:  # noqa: BLE001
                checks.append({"tool": name, "ok": False,
                               "error": f"{type(exc).__name__}: {exc}"})
        # The seal round trip and the SRT path run with the family (or the
        # seal's own single-tool selftest): a verify without a seal in the
        # same process would read tampering that never happened.
        if not only or only == "seal_voice":
            try:
                verify = _seal_voice({"audio": robotic, "action": "verify"})
                checks.append({"tool": "seal_voice (verify)", "ok": bool(verify.get("verified") and verify.get("intact")),
                               "extra": "intact" if verify.get("intact") else "TAMPERED?!"})
            except Exception as exc:  # noqa: BLE001
                checks.append({"tool": "seal_voice (verify)", "ok": False,
                               "error": f"{type(exc).__name__}: {exc}"})
        if not only or only == "clean_subtitles":
            try:
                # the SRT path: duplicate lines merged, punctuation and casing fixed
                srt_clean = _clean_subtitles({"srt": "1\n00:00:00,000 --> 00:00:02,000\num, this is the FIRST line\n\n2\n00:00:01,500 --> 00:00:04,000\nand this is line two with no stop\n\n3\n00:00:04,100 --> 00:00:06,000\nand this is line two with no stop\n\n4\n00:00:06,500 --> 00:00:09,000\na very long subtitle line that goes on and on and on and definitely exceeds any forty-two character budget by far\n"})
                checks.append({"tool": "clean_subtitles (srt)", "ok": bool(srt_clean.get("ok")) and
                               srt_clean["changeLog"]["duplicatesMerged"] >= 1 and
                               srt_clean["changeLog"]["fillersStripped"] >= 1,
                               "extra": str(srt_clean["changeLog"])})
            except Exception as exc:  # noqa: BLE001
                checks.append({"tool": "clean_subtitles (srt)", "ok": False,
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
    parser = argparse.ArgumentParser(description="The words and the take, made to agree.")
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
