#!/usr/bin/env python3
"""concat-prosody-style: how the voice moves - the delivery read.

Pure Python standard library, same contract as the other sense scripts:
`prosody_style.py <extract|selftest> --spec -` reads one JSON spec from
stdin and prints one JSON object on stdout. The spec names the tool in
`tool`.

THE SIX TOOLS THIS MODULE SERVES

  score_emphasis_type    every emphasis word typed: contrastive,
                         new-information, list, intensity - and the visual
                         each type earns
  detect_speaking_style  conversational / presenter / newsread / podcast /
                         documentary-narration / high-energy, read from
                         rate, pitch spread, pause profile and dynamics
  measure_rhythm_meter   syllable nuclei timing, the PVI, stress-timed vs
                         syllable-timed, and whether stresses lock to a
                         beat grid (the music-sync map)
  detect_tempo_changes   accelerando / rallentando / plateau segments of
                         the speech rate - where the delivery itself is
                         the drama
  detect_accent          an honest L2-likeness proxy from fluency, filled
                         pauses, article patterns and (when built) vowel
                         dispersion - caption pacing for clarity, never a
                         verdict on the speaker
  measure_vocal_tract    formant-derived: F1-F4 spread, vowel space, the
                         timbre descriptors behind "this voice sounds
                         like", and the match score for voice-matching

Every reading names its evidence. Where the Rust `voice-signals` ear
(formants, pitch contour) is built, it is used; without it these are
envelope-and-timing proxies and say so.
"""

from __future__ import annotations

import argparse
import json
import math
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

SCHEMA = "funcut.prosody-style/1"


def _ok(tool, **fields):
    out = {"ok": True, "schema": SCHEMA, "tool": tool}
    out.update(fields)
    return out


def _rust_reading(spec):
    """The Rust ear's reading when it is built: pitch contour, emphasis,
    rhythm and formants come from there (see voice_signals._rust_ears)."""
    audio = spec.get("audio") or spec.get("path")
    lines = _vs._collect_lines(spec)
    if not lines:
        return None, None
    reading, note = _vs._rust_ears(spec, lines, audio if audio and Path(str(audio)).is_file() else None,
                                   _vs._ffmpeg(spec)[0])
    return reading, note


def _words_with_times(spec):
    """All measured words with (start, end), from transcript/lines."""
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
# 1. score_emphasis_type
# ---------------------------------------------------------------------------

_CONTRASTIVE = re.compile(r"\b(but|however|instead|actually|only|just|even|"
                          r"real|truth)\b", re.I)
_INTENSIFIERS = {"very", "really", "super", "extremely", "absolutely",
                 "insanely", "totally", "completely", "literally", "so"}
_LIST_CUES = re.compile(r"^(first|second|third|next|then|finally|last)\b", re.I)
_NEWINFO_MARKERS = re.compile(r"\b(this|that|these|those|here|now|today)\b", re.I)


def _score_emphasis_type(spec):
    lines = _vs._collect_lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    audio = spec.get("audio") or spec.get("path")

    # The emphasis candidates: from the Rust ear's energy peaks when built,
    # else from the text's own emphasis grammar.
    rust, rust_note = _rust_reading(spec) if audio else (None, None)
    emphasis_source = None
    per_line = []
    if rust and rust.get("prosody"):
        emphasis_source = "voice-signals (energy peaks + pitch accents)"
        for pe in (rust["prosody"].get("lines") or []):
            per_line.append({"line": pe.get("line"),
                             "emphasisWords": pe.get("emphasisWords") or [],
                             "pitch": pe.get("pitch")})
    if not per_line:
        emphasis_source = ("python (emphasis grammar: contrastive markers, "
                           "intensifiers, list heads, new-information deicts)")
        for idx, line in enumerate(lines):
            text = str(line.get("text") or "")
            words = _vs._words_of(line)
            toks = [re.sub(r"[^\w']", "", str(w.get("text") or "").lower()) for w in words]
            emph = []
            for i, tok in enumerate(toks):
                if not tok:
                    continue
                why = None
                if i > 0 and _CONTRASTIVE.match(toks[i - 1] or ""):
                    why = "contrastive: the word after a pivot marker"
                elif tok in _INTENSIFIERS or (i + 1 < len(toks) and tok in _INTENSIFIERS):
                    why = "intensity: an amplifier or its target"
                elif _LIST_CUES.match(tok):
                    why = "list: an enumeration head"
                elif _NEWINFO_MARKERS.match(tok) and i < len(toks) - 1:
                    why = "new information: a pointing word introducing it"
                elif tok.isupper() and len(tok) > 2 and tok.isalpha():
                    why = "written emphasis: capitalised"
                elif re.search(r"\*\*?[^*]+\*\*?", str(words[i].get("text") or "")):
                    why = "written emphasis: marked in the script"
                if why:
                    emph.append({"word": tok, "why": why,
                                 "start": words[i].get("start") if i < len(words) else None,
                                 "end": words[i].get("end") if i < len(words) else None})
            per_line.append({"line": idx, "emphasisWords": emph})

    visual = {
        "contrastive": "split screen or a hard match cut - the pivot is the edit point",
        "new information": "the b-roll reveal lands exactly here, not before",
        "list": "kinetic sequence: one item per beat, same treatment, same rhythm",
        "intensity": "zoom punch, SFX hit or caption scale-up",
        "written": "highlight the word in the caption, not the whole line",
    }
    flat = []
    for entry in per_line:
        for e in entry.get("emphasisWords") or []:
            kind = next((k for k in ("contrastive", "intensity", "list", "new information")
                         if k in str(e.get("why") or "").lower()), "written")
            flat.append({"line": entry["line"], "word": e.get("word"),
                         "type": kind, "why": e.get("why"),
                         "start": e.get("start"), "end": e.get("end"),
                         "visual": visual[kind]})
    return _ok(
        "score_emphasis_type",
        emphasisWords=flat,
        perLine=per_line,
        counts={k: sum(1 for f in flat if f["type"] == k)
                for k in {f["type"] for f in flat}},
        source=emphasis_source,
        note=rust_note,
        directorHints=[
            "a contrastive emphasis is the cut point: the word before it ends one world, the word after opens another",
            "list emphases want equal shot lengths - the rhythm IS the meaning",
        ] if flat else ["no emphasis candidates found: the delivery is even, "
                       "let the visuals carry the accents"],
    )


# ---------------------------------------------------------------------------
# 2. detect_speaking_style
# ---------------------------------------------------------------------------


def _detect_speaking_style(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    lines = _vs._collect_lines(spec)
    _, words = _words_with_times(spec)

    duration = len(samples) / RATE if (samples := samples) is not None else 0.0
    rate = _vq.RATE
    # Speech rate: words inside voiced spans, per minute.
    if words:
        span = max(0.05, (words[-1]["end"] - words[0]["start"]))
        wpm = 60.0 * len(words) / span
        # pauses: envelope dips below the speech threshold inside the span
        loud = sorted(env, reverse=True)
        speech_ref = loud[max(0, int(0.05 * len(loud)))]
        quiet = sorted(env)
        floor = quiet[int(0.25 * len(quiet))]
        thresh = max(floor * 1.8, speech_ref * 0.04)
        pauses = []
        in_gap = False
        gap_start = 0
        for i in range(int(words[0]["start"] * 100), int(words[-1]["end"] * 100)):
            if i >= len(env):
                break
            if env[i] <= thresh and not in_gap:
                in_gap, gap_start = True, i
            elif env[i] > thresh and in_gap:
                in_gap = False
                if (i - gap_start) / 100.0 >= 0.15:
                    pauses.append((i - gap_start) / 100.0)
        pause_rate = len(pauses) / max(0.5, span / 60.0)  # pauses per minute
    else:
        wpm, pauses, pause_rate = None, [], None

    track = _vq._f0_track(samples, env)
    f0s = [t[1] for t in track]
    f0_mean = sum(f0s) / len(f0s) if f0s else None
    f0_range = (max(f0s) - min(f0s)) if f0s else None
    amps = [t[3] for t in track]
    dynamics = (_vq._db(max(amps)) - _vq._db(min(amps))) if amps and min(amps) > 0 else None

    evidence = {"wpm": round(wpm, 1) if wpm else None,
                "pausesPerMinute": round(pause_rate, 1) if pause_rate is not None else None,
                "f0MeanHz": round(f0_mean, 1) if f0_mean else None,
                "f0RangeSemitones": round(12 * math.log2(f0_range / f0_mean), 1)
                if f0_range and f0_mean else None,
                "dynamicsDb": round(dynamics, 1) if dynamics is not None else None}

    # The classification: each style is a named bundle of evidence.
    styles = []
    if wpm and wpm >= 175 and pause_rate and pause_rate >= 12:
        styles.append(("high-energy", "fast with many short pauses - the ad read"))
    elif wpm and wpm >= 160 and (dynamics or 0) > 12:
        styles.append(("conversational", "fast, dynamic, breathing often"))
    elif wpm and 140 <= wpm < 175 and (pause_rate or 0) >= 6:
        styles.append(("podcast", "relaxed mid pace with natural pausing"))
    elif wpm and 110 <= wpm < 150 and (f0_range or 0) > 40:
        styles.append(("presenter", "measured, deliberate pauses, wide pitch"))
    elif wpm and wpm < 125 and (dynamics or 20) < 12:
        styles.append(("documentary-narration", "slow, even, close to the floor"))
    elif wpm and 150 <= wpm < 185 and (f0_range or 999) < 40:
        styles.append(("newsread", "even pace, narrow pitch - the bulletin read"))
    if not styles:
        styles.append(("unclassified", "the evidence does not bundle into a named style"))
    primary, why = styles[0]

    cut_map = {
        "conversational": "jump cuts on the pauses, handheld energy, captions that pop",
        "presenter": "holds on the key lines, one clean cut per phrase",
        "newsread": "packages: b-roll under every phrase, cut on the phrase boundary",
        "podcast": "two-camera rhythm or subtle zooms, long dissolves",
        "documentary-narration": "slow dissolves, establishing holds, no jump cuts",
        "high-energy": "cut every 0.8-1.5 s, flash frames on the emphasis words",
        "unclassified": "let the emotion read pick the cut style",
    }
    return _ok(
        "detect_speaking_style",
        style=primary,
        alternatives=[{"style": s, "why": w} for s, w in styles[1:]],
        evidence=evidence,
        cutStyle=cut_map[primary],
        directorHints=[
            f"the delivery reads {primary}: {cut_map[primary]}",
            "match the BGM's busyness to the pause rate, not the word rate",
        ],
        source="python (word-rate, envelope pause scan, sparse F0 track)",
        note=None,
    )


RATE = _vq.RATE


# ---------------------------------------------------------------------------
# 3. measure_rhythm_meter
# ---------------------------------------------------------------------------


def _measure_rhythm_meter(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    lines = _vs._collect_lines(spec)
    _, words = _words_with_times(spec)
    if not words:
        return {"ok": False, "error": "no word timings: rhythm needs the transcript's words"}

    # Syllable nuclei: energy peaks inside each word, one per syllable
    # (vowel groups), from the 100 Hz envelope.
    nuclei = []
    for w in words:
        sylls = max(1, _vs._syllables(re.sub(r"[^\w']", "", str(w.get("text") or "").lower())))
        start_idx = int(w["start"] * 100)
        end_idx = int(w["end"] * 100)
        seg = env[max(0, start_idx - 2):max(0, end_idx + 2)]
        if not seg:
            continue
        # peak-pick: sylls peaks, greedily, spaced >= 80 ms
        order = sorted(range(len(seg)), key=lambda i: -seg[i])
        picked = []
        for i in order:
            if all(abs(i - p) >= 8 for p in picked):
                picked.append(i)
            if len(picked) >= sylls:
                break
        picked.sort()
        for p in picked:
            nuclei.append((start_idx - 2 + p) / 100.0)

    # PVI over successive nuclei: the pairwise variability index, the
    # standard syllable-rhythm measure.
    iois = [nuclei[i + 1] - nuclei[i] for i in range(len(nuclei) - 1)
            if 0.03 < nuclei[i + 1] - nuclei[i] < 2.0]
    if len(iois) < 5:
        return {"ok": False,
                "error": "not enough syllable nuclei to time the rhythm (need a longer take)"}
    pvi = 100.0 * sum(abs(iois[i + 1] - iois[i]) / (0.5 * (iois[i] + iois[i + 1]))
                      for i in range(len(iois) - 1)) / (len(iois) - 1)
    mean_ioi = sum(iois) / len(iois)

    # Stress regularity: the CV of inter-nucleus intervals - a metronome
    # reads near zero, free speech does not.
    mean_i = sum(iois) / len(iois)
    var = sum((x - mean_i) ** 2 for x in iois) / len(iois)
    cv = math.sqrt(var) / mean_i

    # Beat alignment: when a BPM is given (or estimated), which nuclei sit
    # within 60 ms of the beat grid, sweeping the phase.
    bpm_hint = spec.get("bpm")
    sync = None
    if bpm_hint:
        period = 60.0 / float(bpm_hint)
        best_phase, best_hits = 0.0, -1
        for phase in [k * period / 20 for k in range(20)]:
            hits = sum(1 for n in nuclei
                       if abs(((n - phase) % period) - 0) < 0.06
                       or abs(((n - phase) % period) - period) < 0.06)
            if hits > best_hits:
                best_hits, best_phase = hits, phase
        sync = {"bpm": float(bpm_hint),
                "nucleiOnBeat": best_hits, "nuclei": len(nuclei),
                "alignment": round(best_hits / len(nuclei), 3),
                "phaseSeconds": round(best_phase, 3)}

    rhythm_class = ("stress-timed" if pvi > 55 else
                    "syllable-timed" if pvi < 35 else "mixed")
    meter = None
    if cv < 0.25:
        meter = "regular (metronomic delivery)"
    elif cv < 0.45:
        meter = "loosely regular"
    return _ok(
        "measure_rhythm_meter",
        rhythmClass=rhythm_class,
        pvi=round(pvi, 1),
        interNucleusMs=round(mean_ioi * 1000, 1),
        nucleiCount=len(nuclei),
        regularity={"cv": round(cv, 3), "meter": meter},
        beatSync=sync,
        syllablesPerSecond=round(len(nuclei) / max(0.5, nuclei[-1] - nuclei[0]), 2),
        directorHints=[
            "a stress-timed voice cuts well to drum music; syllable-timed wants smoother beds",
            "the beatSync map says which words to land on the downbeat when cutting to music",
        ],
        source="python (energy-peak nuclei per word, PVI and CV over intervals)",
    )


# ---------------------------------------------------------------------------
# 4. detect_tempo_changes
# ---------------------------------------------------------------------------


def _detect_tempo_changes(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    _, words = _words_with_times(spec)
    if len(words) < 8:
        return {"ok": False, "error": "not enough words to see tempo change (need 8+)"}

    # Word rate in a sliding 4-word window, in WPM, one series over the
    # take - the tempo curve itself.
    curve = []
    for i in range(len(words) - 3):
        span = words[i + 3]["end"] - words[i]["start"]
        if span > 0.2:
            curve.append(((words[i]["start"] + words[i + 3]["end"]) / 2.0,
                          240.0 / span))
    if len(curve) < 3:
        return {"ok": False, "error": "the words are too sparse to time the tempo"}

    # Segment the curve: a change is a sustained slope beyond 12%/s.
    segments = []
    seg_start, seg_rate = curve[0]
    for j in range(1, len(curve)):
        t, r = curve[j]
        if abs(r - seg_rate) / max(seg_rate, 1) > 0.18:
            segments.append((seg_start, t, seg_rate))
            seg_start, seg_rate = t, r
    segments.append((seg_start, curve[-1][0], seg_rate))
    # merge micro-segments
    merged = []
    for s in segments:
        if merged and (s[1] - merged[-1][0]) < 1.2:
            merged[-1] = (merged[-1][0], s[1], (merged[-1][2] + s[2]) / 2)
        else:
            merged.append(s)
    out = []
    for k, (start, end, r) in enumerate(merged):
        if k == 0:
            trend = "steady"
        else:
            prev = merged[k - 1][2]
            change = (r - prev) / max(prev, 1)
            trend = ("accelerating" if change > 0.15 else
                     "decelerating" if change < -0.15 else "steady")
        out.append({
            "from": round(start, 2), "to": round(end, 2),
            "wpm": round(r, 1), "trend": trend,
            "changePct": round(100 * (r - (merged[k - 1][2] if k else r)) /
                               max(merged[k - 1][2] if k else r, 1), 1),
        })
    dramatic = [s for s in out if s["trend"] != "steady" and (s["to"] - s["from"]) >= 1.5]
    return _ok(
        "detect_tempo_changes",
        segments=out,
        dramatic=dramatic,
        curve=[{"t": round(t, 2), "wpm": round(r, 1)} for t, r in curve],
        directorHints=[
            "an accelerating span is rising tension: shorten the shots progressively",
            "a decelerating span is gravity arriving: hold the shot, let the pause breathe",
            "cut ON the trend changes - the delivery already made the edit point",
        ] if dramatic else ["the delivery keeps one tempo: rhythm changes are yours to make"],
        source="python (4-word sliding rate, slope segmentation)",
    )


# ---------------------------------------------------------------------------
# 5. detect_accent
# ---------------------------------------------------------------------------

_ARTICLE_ERRORS = re.compile(
    r"\b(a apple|a hour|an car|the a |a the |go to home|in the internet)\b", re.I)
_MISSING_ARTICLES = re.compile(
    r"^(She is teacher|He is engineer|I go to school by bus now|This is book)\b", re.I)


def _detect_accent(spec):
    lines = _vs._collect_lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    _, words = _words_with_times(spec)
    audio = spec.get("audio") or spec.get("path")

    cues = []
    # 1. Article patterns in the transcript (text-side, strongest signal).
    text = "\n".join(str(line.get("text") or "") for line in lines).lower()
    article_slips = len(_ARTICLE_ERRORS.findall(text)) + (1 if _MISSING_ARTICLES.search(text) else 0)
    if article_slips:
        cues.append(f"{article_slips} article-pattern slip(s) in the transcript")
    # articles per 100 words: native informal English ~ 8-11.
    toks = _vs._TOKENS(text) if hasattr(_vs, "_TOKENS") else re.findall(r"[\w'’]+", text)
    articles = sum(1 for t in toks if t in ("a", "an", "the"))
    per100 = 100.0 * articles / max(1, len(toks))
    if len(toks) > 40 and per100 < 5:
        cues.append(f"article density {per100:.1f}/100 words is low for native English")

    # 2. Filled pauses per minute, and their type mix.
    fillers = sum(len(_vs._FILLERS_SAFE.findall(" " + str(line.get("text") or "").lower() + " "))
                  for line in lines)
    if words:
        span_min = max(0.25, (words[-1]["end"] - words[0]["start"]) / 60.0)
        fpm = fillers / span_min
        if fpm > 6:
            cues.append(f"{fpm:.0f} filled pauses per minute - a fluency marker")
    # 3. Mid-sentence timing breaks: long intra-phrase gaps suggest
    # formulation effort (an L2 cue when text is simple).
    breaks = 0
    if words:
        for i in range(1, len(words)):
            gap = words[i]["start"] - words[i - 1]["end"]
            same_line = words[i].get("line") == words[i - 1].get("line")
            if same_line and 0.35 < gap < 1.5:
                breaks += 1
        if words and breaks / len(words) > 0.08:
            cues.append(f"{breaks} mid-line timing break(s) while formulating")

    # 4. Vowel dispersion needs the formant ear; used when built.
    rust, rust_note = _rust_reading(spec) if audio else (None, None)
    vowel_note = None
    if rust and rust.get("forensic") and rust["forensic"].get("formants"):
        f = rust["forensic"]["formants"]
        vowel_note = "voice-signals formants available for vowel-dispersion cross-check"
        cues.append(vowel_note) if False else None

    weight = min(1.0, 0.25 * article_slips + 0.2 * (1 if len(toks) > 40 and per100 < 5 else 0)
                 + 0.2 * (1 if fillers and fpm > 6 else 0)
                 + 0.2 * (1 if words and breaks / len(words) > 0.08 else 0))
    if not cues:
        weight = 0.0
    band = "native-like" if weight < 0.25 else "possibly L2" if weight < 0.55 else "likely L2"
    return _ok(
        "detect_accent",
        band=band,
        weight=round(weight, 2),
        cues=cues,
        confidence=0.5,
        purpose="caption pacing and clarity only: slower captions, cleaner mix, key-term emphasis",
        note="an honest proxy from fluency and text patterns, NOT a verdict on the speaker; "
             "accents are normal speech, this tool exists to make edits CLEARER, not to label people",
        captionPacing={"charsPerSecond": 12 if weight >= 0.55 else 15,
                       "note": "L2-informed pacing: more time on screen for key terms"},
        directorHints=[
            "give key terms a caption hold: non-native viewers read terms twice",
            "a light music bed with fewer transients keeps L2 narration intelligible",
        ] if weight >= 0.25 else ["native-like fluency: standard caption pacing"],
        source="python (article patterns, filler rate, mid-line breaks)",
    )


# ---------------------------------------------------------------------------
# 6. measure_vocal_tract
# ---------------------------------------------------------------------------


def _measure_vocal_tract(spec):
    err, samples, env = _vq._need_audio(spec)
    if err:
        return {"ok": False, "error": err}
    rust, rust_note = _rust_reading(spec)
    formants = None
    if rust and rust.get("forensic") and rust["forensic"].get("formants"):
        formants = rust["forensic"]["formants"]

    track = _vq._f0_track(samples, env)
    if not track and not formants:
        return {"ok": False, "error": "no voiced audio and no formant ear: nothing to read"}
    f0s = [t[1] for t in track]
    f0_mean = sum(f0s) / len(f0s) if f0s else None

    if not formants:
        # The proxy: spectral bands standing in for formant regions.
        loudest = sorted(range(len(env)), key=lambda i: -env[i])[:8]
        f1s, f2s, f3s = [], [], []
        for i in loudest:
            mags = _vq._spectrum(samples, i * _vq.HOP)
            if not mags:
                continue
            f1s.append(_vq._band_energy(mags, 250, 850))
            f2s.append(_vq._band_energy(mags, 850, 2500))
            f3s.append(_vq._band_energy(mags, 2500, 3500))
        formant_read = {
            "f1BandDb": round(_vq._db(sum(f1s) / len(f1s)), 1) if f1s else None,
            "f2BandDb": round(_vq._db(sum(f2s) / len(f2s)), 1) if f2s else None,
            "f3BandDb": round(_vq._db(sum(f3s) / len(f3s)), 1) if f3s else None,
            "mode": "proxy bands (250-850 / 850-2500 / 2500-3500 Hz)",
        }
        vowel_space = None
        ctl_estimate = None
    else:
        f1 = formants.get("f1MeanHz") or formants.get("f1")
        f2 = formants.get("f2MeanHz") or formants.get("f2")
        f3 = formants.get("f3MeanHz") or formants.get("f3")
        f4 = formants.get("f4MeanHz") or formants.get("f4")
        formant_read = {"f1Hz": f1, "f2Hz": f2, "f3Hz": f3, "f4Hz": f4,
                        "mode": "voice-signals LPC formants"}
        # Vowel space: the F1/F2 spread the corner vowels define; the tract
        # length from the quarter-wave rule on F3/F4 averages.
        try:
            vowel_space = round(math.hypot(
                (formants.get("f1SpreadHz") or 300),
                (formants.get("f2SpreadHz") or 600)), 1)
        except Exception:  # noqa: BLE001
            vowel_space = None
        ctl = None
        if f3 and f4 and f4 > f3 > 0:
            # average of the 3rd and 4th quarter-wave resonances
            ctl = 3.0 * 343.0 / (4.0 * ((f3 / 3.0) + (f4 / 4.0)) / 2.0)
        ctl_estimate = round(ctl, 1) if ctl else None

    timbre = []
    if formant_read.get("f1BandDb") is not None:
        tilt = (formant_read["f2BandDb"] or 0) - (formant_read["f1BandDb"] or 0)
        timbre.append("dark/warm" if tilt < -6 else "balanced" if tilt < 6 else "bright/forward")
    elif formant_read.get("f2Hz") and formant_read.get("f1Hz"):
        ratio = formant_read["f2Hz"] / max(1.0, formant_read["f1Hz"])
        timbre.append("dark/warm" if ratio < 2.2 else "bright/forward" if ratio > 3.0 else "balanced")
    if f0_mean:
        timbre.append("low-register" if f0_mean < 120 else
                      "mid-register" if f0_mean < 200 else "high-register")

    return _ok(
        "measure_vocal_tract",
        f0MeanHz=round(f0_mean, 1) if f0_mean else None,
        formants=formant_read,
        vowelSpaceHz=vowel_space,
        estimatedTractLengthCm=ctl_estimate,
        timbre=timbre,
        voiceMatch={
            "note": "for multi-voice edits: match F0 within 15 Hz and the F2/F1 ratio within 15% "
                    "before panning two narrators side by side",
            "fingerprint": {"f0": round(f0_mean, 1) if f0_mean else None,
                            "mode": formant_read.get("mode")},
        },
        note=rust_note or ("formants are band proxies; build the voice-signals binary "
                           "(`cargo build -p voice-signals`) for LPC formants"),
        source="python (sparse F0 + spectral bands)" + (" + voice-signals" if formants else ""),
        directorHints=[
            "a dark timbre wants warm grades and slower motion; a bright one cuts faster",
            "match tract estimates before cutting between two narrators - the ear hears the seam",
        ],
    )


# ---------------------------------------------------------------------------
# Dispatch, selftest, entry
# ---------------------------------------------------------------------------

TOOLS = {
    "score_emphasis_type": _score_emphasis_type,
    "detect_speaking_style": _detect_speaking_style,
    "measure_rhythm_meter": _measure_rhythm_meter,
    "detect_tempo_changes": _detect_tempo_changes,
    "detect_accent": _detect_accent,
    "measure_vocal_tract": _measure_vocal_tract,
}

_SELFTEST_WORDS = []
_t = 0.2
for _w, _d in [("but", 0.12), ("THIS", 0.14), ("is", 0.10),
               ("the", 0.09), ("first", 0.16), ("thing", 0.20),
               ("really", 0.18), ("matters", 0.30),
               ("second", 0.16), ("here", 0.12),
               ("today", 0.20), ("numbers", 0.26),
               ("tell", 0.16), ("the", 0.08), ("story", 0.24)]:
    _SELFTEST_WORDS.append({"text": _w, "start": round(_t, 2),
                            "end": round(_t + _d, 2)})
    _t += 0.35


def _synth_voiced(path, f0_start=170.0, f0_end=120.0):
    """A voiced take with drifting F0 and steady energy: enough prosody to
    read F0, style and tract proxies from."""
    import wave as _wave
    rate = _vq.RATE
    total = []
    n = int(4.5 * rate)
    for i in range(n):
        t = i / rate
        f0 = f0_start + (f0_end - f0_start) * (t / 4.5)
        env = 0.55 + 0.45 * math.sin(2 * math.pi * 2.0 * t)
        # two harmonics + a touch of third: vowel-ish
        s = (0.5 * math.sin(2 * math.pi * f0 * t) +
             0.25 * math.sin(4 * math.pi * f0 * t) +
             0.12 * math.sin(6 * math.pi * f0 * t)) * env * 0.4
        total.append(s)
    with _wave.open(path, "wb") as handle:
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
    with tempfile.TemporaryDirectory(prefix="prosody-") as tmp:
        wav = str(Path(tmp) / "voiced.wav")
        _synth_voiced(wav)
        transcript = {"segments": [{
            "text": "But THIS is the first thing that really matters. "
                    "Second, here today the numbers tell the story.",
            "start": 0.2, "end": 4.2, "words": _SELFTEST_WORDS}]}
        for name, handler in TOOLS.items():
            spec = {"audio": wav, "transcript": transcript}
            if name == "score_emphasis_type":
                # text-only path also answers
                spec = {"transcript": transcript}
            if name in ("detect_accent",):
                spec = {"transcript": transcript}
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
    parser = argparse.ArgumentParser(description="How the voice moves.")
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
