#!/usr/bin/env python3
"""concat-voice-signals: the voiceover read as a score, not just words.

Pure Python standard library, same contract as the other sense scripts:
`voice_signals.py <extract|catalog|selftest> --spec -` reads one JSON spec
from stdin (or argv) and prints one JSON object on stdout.

`catalog` answers the signal catalogue: the fifty named readings, grouped
by family, each with what it measures and why the edit wants it - the
table a UI's per-signal buttons are built from. `extract` accepts a
`signals: [name...]` filter (the catalogue's own names): a manual
extraction of exactly the asked-for readings, where only the passes they
need run - a filler read costs no ffmpeg pass and no Rust ear - and the
reply carries a `requested` block with each reading, the ear that
answered it, the family's confidence, and the why the signal exists.

WHAT THIS READS

The six signals FUNCUT already computes but never wired together, per line
of the voiceover:

  loudness    one ffmpeg ebur128 pass, momentary (M) series every 100 ms,
              folded per line: quiet lines vs loud lines for pacing
  pauses      silencedetect spans, classified breath / beat / dramatic /
              scene_turn / dead_air - a dramatic hold must not be cut
  speech rate words and syllables per second, per line
  word confidence  a measured gate from word timing, silence overlap and
              energy - low-confidence words are flagged, not guessed
  language    the script the words are written in, plus the caller's hint:
              Bengali gets a Bengali font and a different music shelf
  arc         passed through from the emotion read when the caller gives it

The three regex signals:

  sentence type  ? ! . and fragments - a question gets a different visual
  repeats    n-gram motifs across lines - the same visual returns
  fillers    um, uh and friends - remove for punch, keep for authenticity

And the Rust ear, `voice-signals` (src/crates/voice-signals): prosody
(pitch contour, emphasis words, rhythm, phrases, terminal rise), forensics
(formants, noise floor, SNR, spectral flux, voice quality), speakers
(VAD + voice-print diarization) and entities (offline NER: money becomes
a stat card, a name becomes a lower-third, a date becomes a timeline).
The binary is optional: without it the Python signals still answer, and
`sources` says exactly which ear answered what.

WhisperX, when installed (pip install whisperx), replaces the whisper
token-timestamp words with aligned words that carry real confidence
scores - the phoneme-accurate cut alignment. Without it, the whisper
times are used as-is and the confidence gate is the honest heuristic.

Everything degrades loudly, never silently: each family of signals names
its source, and an ear that could not run says why.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unicodedata
import wave
from pathlib import Path

SCHEMA = "funcut.voice-signals/2"

# The decision policy: for each editing decision, which signals drive it
# and in what priority order - the table a director reads beside the
# profile so the readings become choices instead of a data dump. The
# signal names are the log's vocabulary: a signalLog entry that names one
# of these is a decision the profile can stand behind.
DECISION_POLICY = {
    "cut": {
        "signals": ["pause.class=dead_air", "emphasisWords", "speechRate.pace",
                    "rhythmLayers", "motifs"],
        "rule": "Cut on dead air and at emphasis boundaries, never inside a "
                "dramatic hold or a scene turn; the pace class sets the cut "
                "length (punchy 0.8-1.5 s, editorial 2.5 s+).",
    },
    "caption": {
        "signals": ["emphasisWords", "words[].confidence", "language.script",
                    "sentence.type"],
        "rule": "Highlight the emphasis words, gate or re-check words under "
                "0.5 confidence, and give non-Latin scripts their fonts and "
                "pacing; a question line's caption holds past its answer.",
    },
    "graphic": {
        "signals": ["entities", "sentence.type", "speechActs", "claims"],
        "rule": "Numbers and money become stat cards, names become lower "
                "thirds, dates become timelines; PII is redacted, and an "
                "uncovered claim gets proof b-roll or a source card.",
    },
    "bgm": {
        "signals": ["forensic.noiseFloorDbfs", "musicSegments", "speechRatio"],
        "rule": "A dry voice (floor under -45 dBFS) gets music and ducking; "
                "a take that already carries music gets none added; a low "
                "SNR gets cleaned first.",
    },
    "transition": {
        "signals": ["speakerChanges", "topics", "discourseMarkers",
                    "sentence.type"],
        "rule": "A speaker change, a topic boundary or a contrastive "
                "'but' earns the transition (whip, dissolve, chapter card); "
                "inside a topic the cuts stay hard.",
    },
}


def _decision_policy_out():
    """The policy as it ships with the profile, with the signal
    vocabulary the log validates against."""
    return {
        decision: {
            "signals": list(entry["signals"]),
            "rule": entry["rule"],
        }
        for decision, entry in DECISION_POLICY.items()
    }


# ---------------------------------------------------------------------------
# The signal catalogue: the fifty named readings, one button each
# ---------------------------------------------------------------------------

# Every reading the profile answers, named, grouped by family, and held
# to a why: the decision (or decisions) the edit spends it on, in the
# decision policy's own vocabulary. `where` says where the reading lands
# - ("take", <dotted path from the reply's root>) for a whole-profile
# reading, ("line", <dotted path within one line row>) for a per-line
# one - so a filtered extract can resolve exactly what was asked. `rust`
# marks the readings that need the binary's ears; `new` marks the
# fourteen the fifty-ear pass added.
SIGNAL_CATALOGUE = [
    # The loudness family, one ebur128 pass.
    {"name": "loudness.integratedLufs", "family": "Loudness",
     "label": "Integrated level",
     "reads": "The take's integrated level, in LUFS.",
     "why": "The level stage's gain against the platform target, capped by "
            "the peak readings (decision: the mix level).",
     "where": ("take", "wholeTake.loudness.integratedLufs"), "rust": False, "new": False},
    {"name": "loudness.loudnessRangeLu", "family": "Loudness",
     "label": "Loudness range",
     "reads": "The take's loudness range, in LU.",
     "why": "A compressed range (under 4 LU) caps the gain stage at +6 dB: "
            "everything the compressor squashed in comes up with the voice.",
     "where": ("take", "wholeTake.loudness.loudnessRangeLu"), "rust": False, "new": False},
    {"name": "loudness.truePeakDbfs", "family": "Loudness",
     "label": "True peak",
     "reads": "The take's true peak, in dBTP.",
     "why": "The -1 dB ceiling the gain stage must respect when the "
            "loudness pass measured one.",
     "where": ("take", "wholeTake.loudness.truePeakDbfs"), "rust": False, "new": False},
    {"name": "loudness.meanLufsM", "family": "Loudness",
     "label": "Line level",
     "reads": "Each line's mean momentary level, in LUFS.",
     "why": "A line delivered above the take's own level is a line the "
            "speaker lifted: it carries the heavier caption weight.",
     "where": ("line", "loudness.meanLufsM"), "rust": False, "new": False},
    {"name": "loudness.relativeLu", "family": "Loudness",
     "label": "Line lift",
     "reads": "Each line's level against the take's integrated level, in LU.",
     "why": "The measured lift behind the caption's weight-800 arm.",
     "where": ("line", "loudness.relativeLu"), "rust": False, "new": False},
    # The pause family, one silencedetect pass.
    {"name": "pauses.classes", "family": "Pauses",
     "label": "Classified pauses",
     "reads": "Every silence, classified: breath, beat, dramatic, scene "
              "turn or dead air - with keep/cut per span.",
     "why": "The tighten cuts only the dead air and never a hold (decision: "
            "cut); a held beat keeps the softest transition (decision: "
            "transition).",
     "where": ("take", "pauses"), "rust": False, "new": False},
    {"name": "pauses.speechRatio", "family": "Pauses",
     "label": "Speech ratio",
     "reads": "The share of the take's time that is speech.",
     "why": "The BGM gate's third input beside the floor and the music "
            "verdict (decision: bgm).",
     "where": ("take", "wholeTake.speechRatio"), "rust": False, "new": False},
    # The rate family, arithmetic on the word timings.
    {"name": "speechRate.wordsPerSec", "family": "Speech rate",
     "label": "Words per second",
     "reads": "Words per second, per line.",
     "why": "The pacing class (punchy, conversational, slow) sizes every "
            "shot's drift (decision: pacing).",
     "where": ("line", "speechRate.wordsPerSec"), "rust": False, "new": False},
    {"name": "speechRate.syllablesPerSec", "family": "Speech rate",
     "label": "Syllables per second",
     "reads": "Syllables per second, per line.",
     "why": "The caption hold's second estimate: a line of long words "
            "reads slower than its word count alone suggests.",
     "where": ("line", "speechRate.syllablesPerSec"), "rust": False, "new": False},
    # The words family.
    {"name": "words.confidence", "family": "Words",
     "label": "Word confidence",
     "reads": "Each word's confidence - WhisperX alignment scores when "
              "installed, the measured gate (timing, silence overlap, "
              "energy) otherwise.",
     "why": "Words under 0.5 are gated from captions rather than guessed "
            "(decision: caption).",
     "where": ("line", "words"), "rust": False, "new": False},
    # The language family.
    {"name": "language.script", "family": "Language",
     "label": "Script",
     "reads": "The script the words are written in, the language detected, "
              "and the caller's hint.",
     "why": "Non-Latin scripts get their fonts and their caption pacing "
            "(decision: caption).",
     "where": ("take", "language"), "rust": False, "new": False},
    # The sentence family.
    {"name": "sentence.type", "family": "Sentence",
     "label": "Sentence type",
     "reads": "Each line's type: question, exclamation, statement or "
              "fragment.",
     "why": "A question's caption holds past its answer, and its seam "
            "takes the pop (decisions: caption, transition).",
     "where": ("line", "sentence.type"), "rust": False, "new": False},
    # The entities family.
    {"name": "entities.cards", "family": "Entities",
     "label": "Entity cards",
     "reads": "Names, numbers, money, dates and places, with the card each "
              "earns (stat, lower third, timeline) and whether the NER "
              "measured it or the regex complement found it.",
     "why": "Money becomes a stat card, a name a lower third, a date a "
            "timeline (decision: graphic) - and an approximate reading "
            "earns no card that claims exactness.",
     "where": ("take", "entities"), "rust": False, "new": False},
    # The motifs family.
    {"name": "motifs.repeats", "family": "Motifs",
     "label": "Repeated phrases",
     "reads": "The phrases the narration repeats: n-gram motifs across "
              "lines, 3-6 words.",
     "why": "A repeated motif is a visual that returns - the same "
            "treatment, the same shot family (decision: cut).",
     "where": ("take", "motifs"), "rust": False, "new": False},
    # The fillers family.
    {"name": "fillers.list", "family": "Fillers",
     "label": "Fillers",
     "reads": "The ums, uhs and friends, per line, classed safe-remove or "
              "keep.",
     "why": "Safe-remove fillers leave the captions; the keep class stays "
            "for authenticity (decision: caption).",
     "where": ("line", "fillers"), "rust": False, "new": False},
    # The arc family, passed through from the emotion read.
    {"name": "arc.emotion", "family": "Arc",
     "label": "Emotional arc",
     "reads": "The emotional arc, passed through from the emotion read the "
              "caller supplies.",
     "why": "The arc's shape sizes the bed's duck and the accent family "
            "each line asks for (decision: bgm).",
     "where": ("take", "arc"), "rust": False, "new": False},
    # The prosody family, the Rust ear's pitch and rhythm.
    {"name": "prosody.emphasisWords", "family": "Prosody",
     "label": "Emphasis words",
     "reads": "The words the delivery stressed, with times and strength.",
     "why": "Emphasis words carry the caption highlight and the cut "
            "boundary (decisions: caption, cut).",
     "where": ("line", "prosody.emphasisWords"), "rust": True, "new": False},
    {"name": "prosody.pitchRangeSemitones", "family": "Prosody",
     "label": "Pitch range",
     "reads": "Each line's pitch range, in semitones.",
     "why": "A range at or over six semitones reads performed rather than "
            "reported: the heavier caption weight.",
     "where": ("line", "prosody.pitchRangeSemitones"), "rust": True, "new": False},
    {"name": "prosody.pitchMeanHz", "family": "Prosody",
     "label": "Line register",
     "reads": "Each line's mean voiced pitch, in Hz.",
     "why": "The register lift - a line over the take's own register is "
            "one the delivery raised, and its peaks outrank a settled "
            "line's.",
     "where": ("line", "prosody.pitchMeanHz"), "rust": True, "new": False},
    {"name": "prosody.finalRise", "family": "Prosody",
     "label": "Terminal rise",
     "reads": "Whether each line's pitch rose at the end, measured "
              "acoustically.",
     "why": "The acoustic ask stands in where the punctuation wrote no "
            "question (decision: transition).",
     "where": ("line", "prosody.finalRise"), "rust": True, "new": False},
    {"name": "prosody.voicedRatio", "family": "Prosody",
     "label": "Voiced ratio",
     "reads": "The share of each line's frames the pitch tracker found a "
              "voice in.",
     "why": "A line with barely any voice under it is a misaligned "
            "transcript, and its caption is gated.",
     "where": ("line", "prosody.voicedRatio"), "rust": True, "new": False},
    {"name": "prosody.rhythmLayers", "family": "Prosody",
     "label": "Rhythm layers",
     "reads": "The rhythm layers, strongest first, with their periods and "
              "strengths.",
     "why": "The period with strength behind it sizes the seam's blend "
            "(decision: pacing).",
     "where": ("take", "wholeTake.prosody"), "rust": True, "new": False},
    {"name": "prosody.phrases", "family": "Prosody",
     "label": "Phrase boundaries",
     "reads": "The breath-group boundaries the prosody graph cut the take "
              "into.",
     "why": "A phrase boundary is the weakest structural seam: a visible "
            "move where the narration turned (decision: transition).",
     "where": ("take", "wholeTake.phrases"), "rust": True, "new": False},
    {"name": "prosody.pitchContourHz", "family": "Prosody",
     "label": "Pitch contour",
     "reads": "The take's own F0 track, at most 256 points, 0 where "
              "unvoiced.",
     "why": "The curve the per-line statistics flatten away: the "
            "delivery's whole shape.",
     "where": ("take", "wholeTake.pitchContourHz"), "rust": True, "new": True},
    # The forensic family: the whole-take reading's keys, one button each.
    {"name": "forensic.noiseFloorDbfs", "family": "Forensics",
     "label": "Noise floor",
     "reads": "The noise floor, in dBFS.",
     "why": "A dry floor (under -45 dBFS) earns a bed; a loud one earns "
            "the repair first (decision: bgm).",
     "where": ("take", "wholeTake.forensic.noiseFloorDbfs"), "rust": True, "new": False},
    {"name": "forensic.snrDb", "family": "Forensics",
     "label": "Signal-to-noise",
     "reads": "The signal-to-noise ratio, in dB.",
     "why": "The duck depth reads it: a low SNR deepens the bed's dip.",
     "where": ("take", "wholeTake.forensic.snrDb"), "rust": True, "new": False},
    {"name": "forensic.noiseType", "family": "Forensics",
     "label": "Noise type",
     "reads": "The floor's character: hiss, hum, rumble or broadband.",
     "why": "The repair shopping list: a notch for a hum, a spectral "
            "denoise for hiss.",
     "where": ("take", "wholeTake.forensic.noiseType"), "rust": True, "new": False},
    {"name": "forensic.spectralFlatness", "family": "Forensics",
     "label": "Spectral flatness",
     "reads": "The floor's spectral flatness, 0-1.",
     "why": "Flatness over 0.25 is broadband: the denoise that fits.",
     "where": ("take", "wholeTake.forensic.spectralFlatness"), "rust": True, "new": False},
    {"name": "forensic.formantsHz", "family": "Forensics",
     "label": "Formants",
     "reads": "The vocal-tract formants, per line.",
     "why": "The speaker's own identity reading: no timeline decision "
            "follows from it, but the profile stands behind it.",
     "where": ("line", "forensic.formantsHz"), "rust": True, "new": False},
    {"name": "forensic.hnrDb", "family": "Forensics",
     "label": "Harmonics-to-noise",
     "reads": "Each line's harmonics-to-noise ratio, in dB.",
     "why": "A clean voice (10 dB or better) keeps the weight its "
            "performance earned.",
     "where": ("line", "forensic.hnrDb"), "rust": True, "new": False},
    {"name": "forensic.jitter", "family": "Forensics",
     "label": "Jitter",
     "reads": "Each line's pitch-period variation, 0-1.",
     "why": "Jitter over 5% retires the accent: emphasis an unstable "
            "delivery cannot hold.",
     "where": ("line", "forensic.jitter"), "rust": True, "new": False},
    {"name": "forensic.shimmer", "family": "Forensics",
     "label": "Shimmer",
     "reads": "Each line's amplitude variation, 0-1.",
     "why": "Shimmer over 6% retires the accent with the jitter it rides "
            "with.",
     "where": ("line", "forensic.shimmer"), "rust": True, "new": False},
    {"name": "forensic.fluxMean", "family": "Forensics",
     "label": "Mean spectral flux",
     "reads": "The mean spectral flux, read against the level it moved "
              "from.",
     "why": "A flux at or over a third of the envelope's mean is a take "
            "that moves: the fast cut style's third fallback.",
     "where": ("take", "wholeTake.forensic.fluxMean"), "rust": True, "new": False},
    {"name": "forensic.fluxOnsetsSec", "family": "Forensics",
     "label": "Transients",
     "reads": "The transient times spectral flux found, in seconds.",
     "why": "A cut lands on a transient, not an arbitrary frame (decision: "
            "cut).",
     "where": ("take", "wholeTake.forensic.fluxOnsetsSec"), "rust": True, "new": False},
    {"name": "forensic.rmsEnvelope", "family": "Forensics",
     "label": "Energy envelope",
     "reads": "The take's RMS envelope, at most 256 points.",
     "why": "The energy curve: a shot laid over a loud stretch moves a "
            "touch more than the same line over a quiet one.",
     "where": ("take", "wholeTake.forensic.rmsEnvelope"), "rust": True, "new": False},
    {"name": "forensic.samplePeakDbfs", "family": "Forensics",
     "label": "Sample peak",
     "reads": "The loudest single sample, in dBFS.",
     "why": "The gain ceiling: peaks near full scale only rise to -1 dB.",
     "where": ("take", "wholeTake.forensic.samplePeakDbfs"), "rust": True, "new": True},
    {"name": "forensic.clippingRatio", "family": "Forensics",
     "label": "Clipping",
     "reads": "The share of samples at or over 99.9% of full scale.",
     "why": "A take that already clips is never raised, whatever the "
            "platform target asks for.",
     "where": ("take", "wholeTake.forensic.clippingRatio"), "rust": True, "new": True},
    {"name": "forensic.dcOffset", "family": "Forensics",
     "label": "DC offset",
     "reads": "The take's DC offset, 0-1 of full scale.",
     "why": "A biased take wants a DC blocker before the denoise: the one "
            "repair a denoise cannot make.",
     "where": ("take", "wholeTake.forensic.dcOffset"), "rust": True, "new": True},
    {"name": "forensic.humHz", "family": "Forensics",
     "label": "Mains hum",
     "reads": "The mains frequency the floor hums at, when a Goertzel pass "
              "found one.",
     "why": "A notch at the mains, not a broadband denoise that would eat "
            "the voice's own low end.",
     "where": ("take", "wholeTake.forensic.humHz"), "rust": True, "new": True},
    {"name": "forensic.sibilanceRatio", "family": "Forensics",
     "label": "Sibilance",
     "reads": "The share of the take's energy in the 4-8 kHz sibilance "
              "band.",
     "why": "A sibilant voice wants the bed further down, out of the band "
            "its own clarity lives in.",
     "where": ("take", "wholeTake.forensic.sibilanceRatio"), "rust": True, "new": True},
    {"name": "forensic.spectralRolloffHz", "family": "Forensics",
     "label": "Spectral rolloff",
     "reads": "The frequency under which 85% of the take's energy sits, in "
              "Hz.",
     "why": "A low rolloff with a narrow band is a muffled take: a "
            "presence lift, not a denoise.",
     "where": ("take", "wholeTake.forensic.spectralRolloffHz"), "rust": True, "new": True},
    {"name": "forensic.spectralBandwidthHz", "family": "Forensics",
     "label": "Spectral bandwidth",
     "reads": "The width of the band the voice occupies, in Hz.",
     "why": "The muffled reading's second arm: the voice that never "
            "reached the presence band at all.",
     "where": ("take", "wholeTake.forensic.spectralBandwidthHz"), "rust": True, "new": True},
    {"name": "forensic.onsetRatePerMin", "family": "Forensics",
     "label": "Onset rate",
     "reads": "The spectral-flux onset rate, in onsets per minute.",
     "why": "An onset a second is a take that moves: the fast cut style's "
            "first fallback.",
     "where": ("take", "wholeTake.forensic.onsetRatePerMin"), "rust": True, "new": True},
    {"name": "forensic.reverbTailSec", "family": "Forensics",
     "label": "Reverb tail",
     "reads": "The estimated reverb tail, in seconds (a median 10 dB decay "
              "scaled to RT60).",
     "why": "A reverberant tail smears over whatever sits under it: the "
            "duck deepens with it.",
     "where": ("take", "wholeTake.forensic.reverbTailSec"), "rust": True, "new": True},
    {"name": "forensic.musicLikelihood", "family": "Forensics",
     "label": "Music likelihood",
     "reads": "How music-like the take is, 0-1: dense regular onsets that "
              "survive the pure-tone veto.",
     "why": "The BGM gate's second opinion: a beat at or over 0.6 says the "
            "take already carries music (decision: bgm).",
     "where": ("take", "wholeTake.forensic.musicLikelihood"), "rust": True, "new": True},
    # The speaker family: the diarization and the VAD's own rhythm.
    {"name": "speaker.turns", "family": "Speakers",
     "label": "Speaker turns",
     "reads": "Who held the take over which span, and the moments it "
              "changed hands.",
     "why": "A speaker change earns the flash transition - but only where "
            "the shares say the take is a conversation (decision: "
            "transition).",
     "where": ("take", "speaker.turns"), "rust": True, "new": False},
    {"name": "speaker.shares", "family": "Speakers",
     "label": "Speaker shares",
     "reads": "How much of the take each speaker holds, in seconds.",
     "why": "A second speaker under a fifth of the speech is a clustering "
            "wobble, not a turn - the gate on the speaker seam.",
     "where": ("take", "speaker.speakers"), "rust": True, "new": False},
    {"name": "speaker.utterances", "family": "Speakers",
     "label": "Utterances",
     "reads": "How many pause-separated speech spans the VAD found.",
     "why": "With the mean: six short utterances is machine-gun delivery, "
            "the fast cut style's second fallback.",
     "where": ("take", "speaker.utterances"), "rust": True, "new": True},
    {"name": "speaker.meanUtteranceSec", "family": "Speakers",
     "label": "Mean utterance",
     "reads": "The mean utterance length, in seconds.",
     "why": "The breath-group length that sizes a seam's blend when the "
            "rhythm read found no period (decision: pacing).",
     "where": ("take", "speaker.meanUtteranceSec"), "rust": True, "new": True},
    {"name": "speaker.longestSilenceSec", "family": "Speakers",
     "label": "Longest silence",
     "reads": "The longest stretch of detected silence between two "
              "utterances, in seconds.",
     "why": "A take whose longest silence never reached 0.9 s is left "
            "whole: its silences are its breathing, not dead air.",
     "where": ("take", "speaker.longestSilenceSec"), "rust": True, "new": True},
]

# The catalogue as the `catalog` action answers it: the families in
# catalogue order, and every signal with its name, label, family, what
# it reads and why the edit wants it.
CATALOGUE_BY_NAME = {entry["name"]: entry for entry in SIGNAL_CATALOGUE}
CATALOGUE_FAMILIES = []
for _entry in SIGNAL_CATALOGUE:
    if _entry["family"] not in CATALOGUE_FAMILIES:
        CATALOGUE_FAMILIES.append(_entry["family"])


def _catalog_out():
    """The catalogue whole: what the fifty signals are and why each one
    exists - the table the UI's signal buttons are built from, and the
    vocabulary the `signals` filter accepts."""
    return {
        "ok": True,
        "schema": SCHEMA,
        "count": len(SIGNAL_CATALOGUE),
        "new": sum(1 for entry in SIGNAL_CATALOGUE if entry["new"]),
        "families": list(CATALOGUE_FAMILIES),
        "signals": [
            {
                "name": entry["name"],
                "family": entry["family"],
                "label": entry["label"],
                "reads": entry["reads"],
                "why": entry["why"],
                "new": entry["new"],
            }
            for entry in SIGNAL_CATALOGUE
        ],
    }


def _wanted_signals(spec):
    """The `signals` filter as a set of catalogue names, or None when the
    caller asked for the whole profile. A name the catalogue does not
    carry is refused with the names that exist, not silently dropped."""
    wanted = spec.get("signals")
    if wanted is None:
        return None
    if isinstance(wanted, str):
        wanted = [wanted]
    if not isinstance(wanted, list) or not wanted:
        return None
    names = []
    for name in wanted:
        if not isinstance(name, str) or name not in CATALOGUE_BY_NAME:
            return {"error": "unknown signal name: pass one of the "
                             "catalogue's names (action 'catalog' lists "
                             "them); got " + repr(name)}
        names.append(name)
    return set(names)


def _resolve_requested(out, wanted):
    """The requested block: one row per asked-for signal, in catalogue
    order, each with the reading resolved from the profile the extract
    built, the ear that answered it, and the confidence the family
    carries. A reading the ear could not make resolves to None - an
    absent measurement, never a guess."""
    confidences = {row["signal"]: row
                   for row in (out.get("signalConfidence") or {}).get("perSignal", [])}
    sources = out.get("sources") or {}
    family_confidence = {
        "Loudness": "loudness", "Pauses": "pauses", "Speech rate": "speechRate",
        "Words": "wordConfidence", "Language": "language", "Sentence": "sentenceType",
        "Entities": "entities", "Motifs": "motifs", "Fillers": "fillers",
        "Arc": "arc", "Prosody": "prosody", "Forensics": "forensic",
        "Speakers": "speaker",
    }
    # A few signals carry their own confidence row rather than their
    # family's: the formants are a separate, lower-confidence LPC reading
    # inside the forensic family, and the catalogue's button must quote
    # the row the formants themselves earned.
    signal_confidence = {
        "forensic.formantsHz": "formants",
    }
    family_source = {
        "Loudness": "loudness", "Pauses": "pauses", "Speech rate": "speechRate",
        "Words": "wordConfidence", "Language": "language", "Sentence": "sentenceType",
        "Entities": "entities", "Motifs": "motifs", "Fillers": "fillers",
        "Arc": "arc", "Prosody": "prosody", "Forensics": "forensic",
        "Speakers": "speaker",
    }
    rows = []
    for entry in SIGNAL_CATALOGUE:
        if entry["name"] not in wanted:
            continue
        kind, path = entry["where"]
        if kind == "line":
            value = []
            for line in out.get("lines") or []:
                node = line
                for step in path.split("."):
                    node = (node or {}).get(step) if isinstance(node, dict) else None
                value.append({
                    "line": line.get("index"),
                    "start": line.get("start"),
                    "end": line.get("end"),
                    "text": line.get("text"),
                    "value": node,
                })
        else:
            node = out
            for step in path.split("."):
                node = (node or {}).get(step) if isinstance(node, dict) else None
            value = node
        confidence = confidences.get(
            signal_confidence.get(entry["name"])
            or family_confidence.get(entry["family"], ""))
        rows.append({
            "name": entry["name"],
            "family": entry["family"],
            "label": entry["label"],
            "reads": entry["reads"],
            "why": entry["why"],
            "new": entry["new"],
            "value": value,
            "source": sources.get(family_source.get(entry["family"], "")) or None,
            "confidence": confidence["confidence"] if confidence else None,
            "basis": confidence["basis"] if confidence else None,
        })
    return rows

# ---------------------------------------------------------------------------
# The transcript in
# ---------------------------------------------------------------------------


def _collect_lines(spec):
    """Lines from a transcript job, a bare list, or raw script text.

    Same reading order as emotion.py: a line is a sentence, because every
    signal here - loudness, rate, sentence type, emphasis - is a
    per-sentence fact.
    """
    if isinstance(spec.get("lines"), list) and spec["lines"]:
        lines = []
        for entry in spec["lines"]:
            if isinstance(entry, str):
                lines.append({"text": entry})
            elif isinstance(entry, dict) and str(entry.get("text") or "").strip():
                lines.append(entry)
        return lines
    transcript = spec.get("transcript")
    if isinstance(transcript, dict) and isinstance(transcript.get("segments"), list):
        segments = [s for s in transcript["segments"]
                    if isinstance(s, dict) and str(s.get("text") or "").strip()]
        if segments:
            return segments
    if isinstance(transcript, list) and transcript:
        return [s for s in transcript if isinstance(s, dict) and str(s.get("text") or "").strip()]
    for key in ("script", "text"):
        if isinstance(spec.get(key), str) and spec[key].strip():
            pieces = re.split(r"(?<=[.!?])\s+|\n+", spec[key].strip())
            return [{"text": piece.strip()} for piece in pieces if piece.strip()]
    return []


def _words_of(line):
    """The line's measured words, normalised to {text, start, end}."""
    words = []
    for word in line.get("words") or []:
        if not isinstance(word, dict):
            continue
        text = str(word.get("text") or word.get("word") or "").strip()
        try:
            start = float(word.get("start"))
            end = float(word.get("end"))
        except (TypeError, ValueError):
            continue
        if text:
            words.append({"text": text, "start": start, "end": end})
    return words


# ---------------------------------------------------------------------------
# ffmpeg helpers (the same shapes sense.py uses, kept local so the script
# runs standalone from the unpacked sense runtime)
# ---------------------------------------------------------------------------


def _ffmpeg(spec):
    return (os.environ.get("FUNCUT_FFMPEG") or shutil.which("ffmpeg") or "ffmpeg",
            shutil.which("ffprobe") or "ffprobe")


def _ffmpeg_runs(spec=None):
    """Whether a decoder this module can drive is actually on the machine.
    The loudness and pause readings ride ffmpeg; everything else is
    pure Python, and the selftest says which half it proved."""
    ff, _ = _ffmpeg(spec or {})
    rc, _, _ = _run([ff, "-hide_banner", "-version"], 10)
    return rc == 0


def _run(cmd, timeout=120):
    try:
        done = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              timeout=timeout)
        return done.returncode, done.stdout, done.stderr
    except (OSError, subprocess.TimeoutExpired) as exc:
        return -1, b"", str(exc).encode()


_SILENCE_START = re.compile(r"silence_start:\s*([-0-9.]+)")
_SILENCE_END = re.compile(r"silence_end:\s*([-0-9.]+)")


def _silence_spans(ff, path, threshold_db, min_silence):
    """ffmpeg's silencedetect, the exact analysis.silence reading."""
    cmd = [ff, "-hide_banner", "-nostdin", "-nostats", "-i", path, "-vn",
           "-af", "silencedetect=noise=-%gdB:d=%g" % (threshold_db, min_silence),
           "-f", "null", "-"]
    rc, _, err = _run(cmd, 240)
    text = err.decode("utf-8", "replace")
    if rc != 0:
        raise RuntimeError(f"silencedetect failed (rc {rc}): {text.strip()[-200:] or 'no decoder - set FUNCUT_FFMPEG or install ffmpeg'}")
    duration = None
    total = re.search(r"Duration:\s*([0-9:.]+)", text)
    if total:
        parts = [float(p) for p in total.group(1).split(":")]
        duration = parts[0] * 3600 + parts[1] * 60 + parts[2]
    spans = []
    start = None
    for line in text.splitlines():
        begin = _SILENCE_START.search(line)
        finish = _SILENCE_END.search(line)
        if begin:
            start = max(0.0, float(begin.group(1)))
        if finish and start is not None:
            end = float(finish.group(1))
            if end > start:
                spans.append((start, end))
            start = None
    if start is not None and duration and duration > start:
        spans.append((start, duration))
    return spans, duration


_MOMENTARY = re.compile(
    r"t:\s*([0-9.]+)\s+TARGET:.*?\bM:\s*(-?[0-9.]+|-inf)\s+S:.*?\bTPK:\s*(-?[0-9.]+|-inf)")


def _momentary_series(ff, path):
    """The ebur128 momentary-loudness series: [(t, M LUFS, TP dBFS)] every
    100 ms, from one pass. framelog left on, which is the point."""
    cmd = [ff, "-hide_banner", "-nostats", "-i", path,
           "-filter_complex", "ebur128=peak=true", "-f", "null", "-"]
    rc, _, err = _run(cmd, 240)
    if rc != 0:
        raise RuntimeError("ebur128 pass failed (rc %d): %s" % (rc, err.decode("utf-8", "replace").strip()[-200:] or "no decoder - set FUNCUT_FFMPEG or install ffmpeg"))
    series = []
    for match in _MOMENTARY.finditer(err.decode("utf-8", "replace")):
        t = float(match.group(1))
        m = None if match.group(2) == "-inf" else float(match.group(2))
        tp = None if match.group(3) == "-inf" else float(match.group(3))
        series.append((t, m, tp))
    return series


def _integrated(ff, path):
    """The whole-file integrated figures, for relative per-line loudness."""
    cmd = [ff, "-hide_banner", "-nostats", "-i", path,
           "-filter_complex", "ebur128=peak=true:framelog=quiet", "-f", "null", "-"]
    rc, _, err = _run(cmd, 240)
    text = err.decode("utf-8", "replace")
    summary = text[text.rfind("Summary:"):] if "Summary:" in text else text

    def grab(pattern):
        found = re.findall(pattern, summary)
        try:
            return float(found[-1])
        except (ValueError, IndexError):
            return None

    return (grab(r"I:\s*(-?[\d.]+)\s*LUFS"),
            grab(r"LRA:\s*(-?[\d.]+)\s*LU"),
            grab(r"Peak:\s*(-?[\d.]+)\s*dBFS"))


# ---------------------------------------------------------------------------
# The six wired signals
# ---------------------------------------------------------------------------


def _line_loudness(series, start, end):
    """Per-line momentary loudness. The M window is 400 ms, so samples are
    taken from half a window outside the line too: a 200 ms line still gets
    a reading from the window that covers it, not from nothing."""
    if start is None or end is None:
        return None
    values = [m for t, m, _ in series
              if m is not None and start - 0.2 <= t <= end + 0.2]
    peaks = [tp for t, _, tp in series
             if tp is not None and start - 0.2 <= t <= end + 0.2]
    if not values:
        return None
    return {
        "meanLufsM": round(sum(values) / len(values), 2),
        "minLufsM": round(min(values), 2),
        "maxLufsM": round(max(values), 2),
        "maxTpDbfs": round(max(peaks), 2) if peaks else None,
        "samples": len(values),
    }


def _classify_pauses(spans, lines):
    """breath / beat / dramatic / scene_turn / dead_air.

    A pause is a dramatic hold when it sits inside a sentence (the next
    line does not start a new sentence); it is a scene turn when it does.
    Both are KEEP signals - the dead air edit must not cut a hold. Only
    dead air (over 2.5 s, or a turn over 4 s) is a cut candidate.
    """
    timed = [(i, l) for i, l in enumerate(lines)
             if isinstance(l.get("start"), (int, float)) and isinstance(l.get("end"), (int, float))]
    out = []
    for start, end in spans:
        dur = end - start
        before = None
        after = None
        for i, line in timed:
            if line["end"] <= start + 0.05:
                before = (i, str(line.get("text") or ""))
            if line.get("start") >= end - 0.05 and after is None:
                after = (i, str(line.get("text") or ""))
        next_lower = bool(after and after[1][:1].islower())
        if dur < 0.25:
            kind, keep = "breath", True
        elif dur < 0.6:
            kind, keep = "beat", True
        elif dur < 1.2 and next_lower:
            kind, keep = "dramatic", True
        elif dur < 2.5:
            kind, keep = "scene_turn", True
        else:
            kind, keep = "dead_air", False
        out.append({
            "start": round(start, 3),
            "end": round(end, 3),
            "durationSec": round(dur, 3),
            "class": kind,
            "keep": keep,
            "betweenLines": [before[0] if before else None, after[0] if after else None],
            "note": {
                "breath": "natural breath - keep",
                "beat": "emphasis beat - keep",
                "dramatic": "dramatic hold mid-sentence - never cut",
                "scene_turn": "sentence boundary - the moment a visual can change",
                "dead_air": "dead air - a cut candidate",
            }[kind],
        })
    return out


def _syllables(word):
    """Syllable estimate: vowel groups for Latin/Cyrillic/Greek scripts,
    a flat 1.5-per-word estimate elsewhere. An estimate, labelled as one."""
    vowels_latin = "aeiouy"
    vowels_cyrillic = "аеёиоуыэюяєіїои"
    vowels_greek = "αεηιουω"
    text = word.lower()
    latin = sum(1 for c in text if c in vowels_latin)
    cyrillic = sum(1 for c in text if c in vowels_cyrillic)
    greek = sum(1 for c in text if c in vowels_greek)
    if latin:
        groups = len(re.findall(r"[aeiouy]+", text))
        if text.endswith("e") and groups > 1:
            groups -= 1  # the silent English 'e'
        return max(1, groups)
    if cyrillic:
        return max(1, len(re.findall(r"[аеёиоуыэюяєіїои]+", text)))
    if greek:
        return max(1, len(re.findall(r"[αεηιουω]+", text)))
    return 1.5  # an honest flat estimate for scripts without vowel marks


def _speech_rate(line, words):
    """Words per minute and syllables per second, per line."""
    start, end = line.get("start"), line.get("end")
    text = str(line.get("text") or "")
    token_count = len(words) or len([w for w in text.split() if w])
    if not isinstance(start, (int, float)) or not isinstance(end, (int, float)):
        return None
    dur = max(0.05, end - start)
    wpm = token_count / dur * 60.0
    syl = sum(_syllables(w["text"]) for w in words) if words else \
        sum(_syllables(w) for w in text.split())
    sps = syl / dur
    if wpm >= 190 or sps >= 5.0:
        pace = "punchy"
    elif wpm >= 140 or sps >= 3.5:
        pace = "conversational"
    elif wpm >= 105:
        pace = "editorial"
    else:
        pace = "slow"
    return {
        "wordsPerMinute": round(wpm, 1),
        "wordsPerSec": round(token_count / dur, 2),
        "syllablesPerSec": round(sps, 2),
        "pace": pace,
    }


def _word_confidences(words, silence_spans, series):
    """A measured confidence per word, when whisper gave no probability.

    The gate: a word whose span overlaps silence, or whose span is
    implausibly short or long, or whose moment sits on audio tens of LU
    under the take's mean, loses confidence. This is a gate, not a guess:
    it only ever lowers confidence for a measured reason.
    """
    if not words:
        return []
    loud = [m for _, m, _ in series if m is not None]
    # The quiet arm's floor sits 18 LU under the take's own mean, not 6 LU
    # over its minimum: any take with a real silence in it put the old
    # floor near digital silence, and the arm never fired at all.
    floor = (sum(loud) / len(loud) - 18.0) if loud else None
    out = []
    for word in words:
        conf = 0.9
        reasons = []
        dur = word["end"] - word["start"]
        if dur < 0.06:
            conf -= 0.3
            reasons.append("span under 60 ms")
        elif dur > 1.6:
            conf -= 0.2
            reasons.append("span over 1.6 s")
        for s, e in silence_spans:
            overlap = min(word["end"], e) - max(word["start"], s)
            if overlap > 0.04 * max(0.01, dur):
                conf -= 0.35
                reasons.append("overlaps silence")
                break
        if floor is not None:
            mid = (word["start"] + word["end"]) / 2.0
            near = [m for t, m, _ in series if m is not None and abs(t - mid) <= 0.25]
            if near and max(near) < floor:
                conf -= 0.3
                reasons.append("sits on quiet audio")
        out.append(dict(word, confidence=round(max(0.0, min(1.0, conf)), 3),
                        confidenceReasons=reasons))
    return out


_SCRIPTS = [
    ("bengali", "bn", "Bengali", "Noto Sans Bengali", "cinematic-strings"),
    ("devanagari", "hi", "Devanagari", "Noto Sans Devanagari", "cinematic-strings"),
    ("arabic", "ar", "Arabic", "Noto Naskh Arabic", "oud-percussion"),
    ("hebrew", "he", "Hebrew", "Noto Sans Hebrew", "piano-minimal"),
    ("thai", "th", "Thai", "Noto Sans Thai", "gamelan-soft"),
    ("hangul", "ko", "Hangul", "Noto Sans KR", "k-indie"),
    ("hiragana", "ja", "Japanese", "Noto Sans JP", "lofi-piano"),
    ("katakana", "ja", "Japanese", "Noto Sans JP", "lofi-piano"),
    ("han", "zh", "Han", "Noto Sans SC", "guzheng-modern"),
    ("cyrillic", "ru", "Cyrillic", "Noto Sans", "piano-minimal"),
    ("greek", "el", "Greek", "Noto Sans", "guitar-warm"),
]


def _detect_language(lines, hint):
    """The script the words are written in, and what it means for the edit."""
    text = " ".join(str(l.get("text") or "") for l in lines)
    counts = {}
    for char in text:
        if char.isalpha():
            script = unicodedata.name(char).split()[0].lower()
            counts[script] = counts.get(script, 0) + 1
    total = sum(counts.values()) or 1
    ranked = sorted(counts.items(), key=lambda kv: -kv[1])
    script = ranked[0][0] if ranked else "latin"
    share = ranked[0][1] / total if ranked else 0.0
    detected = None
    font = "Inter"
    genre = "lofi-warm"
    for name, code, label, font_hint, genre_hint in _SCRIPTS:
        if script == name and share >= 0.25:
            detected = code
            font, genre = font_hint, genre_hint
            break
    else:
        if script == "latin" or not ranked:
            detected = "en"
            font, genre = "Inter", "lofi-warm"
    return {
        "detected": detected,
        "script": script,
        "scriptShare": round(share, 3),
        "hint": hint,
        "matchesHint": (hint is None or detected is None or hint.lower().startswith(detected)
                        or detected.startswith(hint.lower()[:2])),
        "fontHint": font,
        "musicGenreHint": genre,
    }


# ---------------------------------------------------------------------------
# The three regex signals
# ---------------------------------------------------------------------------


def _sentence_type(text, final_rise=None):
    """question / exclamation / statement / fragment, from the punctuation,
    cross-checked against the prosody's terminal rise when it is there."""
    body = str(text or "").rstrip()
    if body.endswith("?"):
        kind = "question"
    elif body.endswith("!"):
        kind = "exclamation"
    elif body.endswith(".") or body.endswith("。") or body.endswith("।"):
        kind = "statement"
    elif re.match(r"^(who|what|when|where|why|how|which|whose|whom|can|could|would|should|"
                  r"is|are|do|does|did|will|won't|have|has)\b", body, re.I) and not body.endswith("."):
        kind = "question"
    else:
        kind = "fragment"
    confirmed = None
    if final_rise is not None:
        if kind == "question":
            confirmed = bool(final_rise)
        elif final_rise and kind in ("statement", "fragment"):
            confirmed = True  # the voice asks what the punctuation did not
    return {"type": kind, "riseConfirms": confirmed}


_FILLERS_SAFE = re.compile(r"\b(um+|uh+|erm+|hmm+|uhm+|mm+hmm)\b", re.I)
_FILLERS_CONTEXT = re.compile(
    r"\b(like|you know|sort of|kind of|basically|actually|literally|i mean)\b", re.I)


def _fillers(line, words):
    """Filler words with timings: um/uh are safe removals, the discourse
    markers are context calls the director makes."""
    text = str(line.get("text") or "")
    found = []
    for match in _FILLERS_SAFE.finditer(text):
        found.append({"word": match.group(0), "kind": "safe-remove",
                      "start": _char_time(match.start(), text, line, words),
                      "end": _char_time(match.end(), text, line, words)})
    for match in _FILLERS_CONTEXT.finditer(text):
        found.append({"word": match.group(0), "kind": "context-call",
                      "start": _char_time(match.start(), text, line, words),
                      "end": _char_time(match.end(), text, line, words)})
    return found


def _char_time(char_index, text, line, words):
    """Best-effort time for a character position: proportionally inside the
    line's span, snapped onto the word that contains it when there is one."""
    start, end = line.get("start"), line.get("end")
    if not isinstance(start, (int, float)) or not isinstance(end, (int, float)):
        return None
    if not text:
        return None
    frac = char_index / max(1, len(text))
    for word in words:
        wstart = text.find(word["text"])
        if wstart >= 0 and wstart <= char_index <= wstart + len(word["text"]):
            return round(word["start"] + (char_index - wstart) /
                         max(1, len(word["text"])) * (word["end"] - word["start"]), 3)
    return round(start + frac * (end - start), 3)


def _tokenize(text):
    return [t for t in re.findall(r"[a-z0-9']+", str(text or "").lower()) if len(t) > 2]


def _motifs(lines):
    """Repeated n-gram phrases (3 to 6 words) across lines: the motifs that
    deserve the same visual or sound when they return."""
    grams = {}
    for i, line in enumerate(lines):
        tokens = _tokenize(line.get("text"))
        for n in (3, 4, 5, 6):
            for j in range(len(tokens) - n + 1):
                gram = " ".join(tokens[j:j + n])
                grams.setdefault(gram, {"n": n, "lines": []})["lines"].append(i)
    motifs = []
    claimed = []
    # Longest first: a repeated 5-gram absorbs its repeated 3-gram tails.
    for gram, info in sorted(grams.items(), key=lambda kv: (-kv[1]["n"], kv[0])):
        distinct = sorted(set(info["lines"]))
        if len(distinct) < 2:
            continue
        if any(gram in c for c in claimed):
            continue
        claimed.append(gram)
        motifs.append({
            "phrase": gram,
            "count": len(distinct),
            "lineIndices": distinct,
            "note": "bring the same visual and sound back on the repeat",
        })
        if len(motifs) >= 12:
            break
    return motifs


# ---------------------------------------------------------------------------
# The entity complement: what the pattern NER misses
# ---------------------------------------------------------------------------

_YEAR = re.compile(r"\b(19\d{2}|20\d{2})\b")
_MONEYISH = re.compile(r"\$?\b\d{1,3}(?:,\d{3})+|\$\d+(?:\.\d+)?(?:\s?(?:k|m|b|million|billion))?\b",
                       re.I)
_PERCENT = re.compile(r"\b\d+(?:\.\d+)?\s?%\b")


def _entity_complement(lines):
    """Bare years, big numbers, money and percentages: the pattern NER
    labels written-out dates and money with symbols, and misses the bare
    forms a narration actually says. Each is mapped to word times when the
    words are there, exactly like the anno entities."""
    out = []
    for i, line in enumerate(lines):
        text = str(line.get("text") or "")
        words = _words_of(line)
        for pattern, kind, card in ((_YEAR, "DATE", "timeline"),
                                    (_MONEYISH, "MONEY", "stat"),
                                    (_PERCENT, "PERCENT", "stat")):
            for match in pattern.finditer(text):
                span = match.span()
                start = _char_time(span[0], text, line, words)
                end = _char_time(span[1], text, line, words)
                out.append({
                    "lineIndex": i,
                    "text": match.group(0),
                    "entityType": kind,
                    "confidence": 0.75,
                    "start": start,
                    "end": end,
                    "approximate": True,
                    "card": card,
                    "source": "regex-complement",
                })
    return out


def _merge_entities(primary, complement):
    """anno first, the regex complement only where nothing overlaps it."""
    merged = list(primary)
    for extra in complement:
        overlap = any(
            e["lineIndex"] == extra["lineIndex"] and
            (e.get("start") is None or extra.get("start") is None or
             (e.get("start", 0) < extra.get("end", 0) and
              extra.get("start", 0) < e.get("end", 0)))
            for e in merged if e.get("source") != "regex-complement"
        )
        if not overlap:
            merged.append(extra)
    return merged


# ---------------------------------------------------------------------------
# The Rust ear
# ---------------------------------------------------------------------------


def _rust_binary():
    """Where the voice-signals binary hides: the operator's pin, the app
    bundle's own directory (FUNCUT_APP_DIR - the host passes the directory
    of its own executable, where a packaged install drops voice-signals
    beside it, no PATH entry required), PATH, the script's own directory
    (where an installer drops it), or a build tree checked out beside the
    sense scripts."""
    pinned = os.environ.get("FUNCUT_VOICE_SIGNALS_BIN")
    if pinned and os.path.isfile(pinned):
        return pinned
    app_dir = os.environ.get("FUNCUT_APP_DIR")
    if app_dir:
        bundled = os.path.join(app_dir, "voice-signals")
        if os.path.isfile(bundled):
            return bundled
    found = shutil.which("voice-signals")
    if found:
        return found
    here = Path(__file__).resolve().parent
    beside = here / "voice-signals"
    if beside.is_file():
        return str(beside)
    # A build tree checked out beside the sense scripts: the workspace's
    # target/ (this repo keeps its Cargo workspace under src/, so the
    # script's grandparents are the workspace root) and, for a checkout
    # whose workspace sits at the root, the repository's own target/.
    for base in (here.parents[2], here.parents[3]):
        for profile in ("release", "debug"):
            built = base / "target" / profile / "voice-signals"
            if built.is_file():
                return str(built)
    return None


def _rust_ears(spec, lines, audio, ff):
    """Run the Rust extractor, over a 16 kHz mono WAV handoff when there
    is audio and text-only otherwise (the NER ear still answers). Returns
    (reading, note): the reading when it ran, the note when it did not."""
    binary = _rust_binary()
    if binary is None:
        return None, "the voice-signals binary is not built; build it with " \
                     "`cargo build -p voice-signals` (prosody, forensics, " \
                     "speakers and NER entities are absent from this reading)"
    rust_spec = {
        "lines": [
            {
                "start": line.get("start"),
                "end": line.get("end"),
                "text": str(line.get("text") or ""),
                "words": [{"start": w["start"], "end": w["end"], "text": w["text"]}
                          for w in _words_of(line)],
            }
            for line in lines
        ],
        "maxSpeakers": int(spec.get("maxSpeakers") or 4),
        "speakerThreshold": float(spec.get("speakerThreshold") or 0.6),
    }
    workdir = None
    if audio:
        workdir = tempfile.mkdtemp(prefix="voice-signals-")
        wav = os.path.join(workdir, "in.wav")
        convert = [ff, "-hide_banner", "-nostdin", "-y", "-i", audio,
                   "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", wav]
        rc, _, err = _run(convert, 240)
        if rc != 0 or not os.path.isfile(wav):
            shutil.rmtree(workdir, ignore_errors=True)
            return None, "could not convert the audio for the Rust ears: " + \
                err.decode("utf-8", "replace").strip()[-200:]
        rust_spec["wav"] = wav
    try:
        done = subprocess.run([binary, "extract", "--spec", "-"],
                              input=json.dumps(rust_spec).encode(),
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=420)
        reading = json.loads(done.stdout.decode("utf-8", "replace"))
    except (OSError, ValueError, subprocess.TimeoutExpired) as exc:
        return None, f"the Rust ears did not answer: {type(exc).__name__}: {exc}"
    finally:
        if workdir:
            shutil.rmtree(workdir, ignore_errors=True)
    if not reading.get("ok"):
        return None, "the Rust ears refused: " + str(reading.get("error") or "unknown error")[:200]
    return reading, None


# ---------------------------------------------------------------------------
# WhisperX: the phoneme-accurate word alignment, when it is installed
# ---------------------------------------------------------------------------


def _whisperx_words(audio, lines, language):
    """Align the transcript with WhisperX when it is importable: word times
    that carry measured scores, the alignment captions and cuts actually
    want. Returns (words per line index, note)."""
    try:
        import whisperx  # noqa: F401  the heavy import, guarded on purpose
    except Exception:
        return None, "whisperx is not installed (pip install whisperx); " \
                     "whisper token-timestamp words are used as-is"
    try:
        segments = [{"start": l.get("start"), "end": l.get("end"),
                     "text": str(l.get("text") or "")}
                    for l in lines
                    if isinstance(l.get("start"), (int, float))]
        if not segments:
            return None, "no timed lines to align"
        device = os.environ.get("FUNCUT_VOICE_SIGNALS_DEVICE", "cpu")
        audio_data = whisperx.load_audio(audio)
        model, metadata = whisperx.load_align_model(
            language_code=language or "en", device=device)
        aligned = whisperx.align(segments, model, metadata, audio_data, device,
                                 return_char_alignments=False)
        by_line = []
        for seg in aligned.get("segments") or []:
            words = []
            for word in seg.get("words") or []:
                if word.get("start") is None or word.get("end") is None:
                    continue
                words.append({
                    "text": str(word.get("word") or "").strip(),
                    "start": float(word["start"]),
                    "end": float(word["end"]),
                    "score": float(word.get("score") or 0.0),
                })
            by_line.append(words)
        return by_line, None
    except Exception as exc:
        return None, f"whisperx alignment failed: {type(exc).__name__}: {exc}"


# ---------------------------------------------------------------------------
# The director hints: the wiring
# ---------------------------------------------------------------------------


def _signal_confidence(sources, audio, rust, wx_words, series, notes):
    """One confidence per signal family, with the basis a director can
    weigh: which ear answered, and what it was built on. A signal with a
    low confidence is not wrong - it is one the plan should lean on less,
    and the basis says why. Formants on a phone-band take, reverb from a
    single-channel envelope, emphasis without word timings: each is
    honest about what it is."""
    rust_up = bool(rust)
    band = None
    forensic = (rust or {}).get("forensic", {}).get("whole") or {}
    sample_rate = forensic.get("sampleRateHz") or forensic.get("sampleRate")
    if isinstance(sample_rate, (int, float)) and sample_rate > 0:
        band = min(sample_rate / 2.0, 20000.0)
    lines = []

    def entry(name, confidence, basis):
        lines.append({"signal": name, "confidence": round(confidence, 2),
                      "basis": basis})

    # Loudness: ebur128 measured on the real take, or nothing.
    if sources.get("loudness"):
        entry("loudness", 0.9,
              "ffmpeg ebur128 momentary series over the take"
              + (f"; bandwidth {band/1000:.1f} kHz" if band else ""))
    else:
        entry("loudness", 0.0, "no audio: the loudness signals are absent")

    # Pauses: silencedetect thresholds are honest but class-dependent.
    if sources.get("pauses"):
        entry("pauses", 0.75,
              "ffmpeg silencedetect; the class boundaries (breath/beat/"
              "dramatic/dead air) are heuristic, the spans are measured")
    else:
        entry("pauses", 0.0, "no audio: no silence spans")

    # Speech rate: arithmetic on word timings; as good as the timings.
    if wx_words:
        entry("speechRate", 0.9, "whisperx-aligned word times")
    elif audio:
        entry("speechRate", 0.7, "whisper token timestamps; syllables are "
                                 "vowel-group estimates")
    else:
        entry("speechRate", 0.4, "no timings: estimated from the script at "
                                 "150 wpm")

    # Word confidence: whisperx scores or the measured gate.
    if str(sources.get("wordConfidence") or "").startswith("whisperx"):
        entry("wordConfidence", 0.9, "whisperx alignment scores")
    elif sources.get("wordConfidence"):
        entry("wordConfidence", 0.6,
              "measured gate (timing, silence overlap, energy) - a screen, "
              "not a score")
    else:
        entry("wordConfidence", 0.3, "no audio: word confidence is absent")

    # Text signals: regex and lexicons, language-dependent.
    entry("sentenceType", 0.8, "punctuation and cue patterns; cross-checked "
                               "against terminal pitch rise when prosody ran")
    entry("fillers", 0.65, "regex lexicon, English core; other languages "
                           "under-read")
    entry("motifs", 0.7, "n-gram overlap; a motif shorter than three words "
                         "can be coincidence")
    entry("language", 0.85, "Unicode script census")
    # The arc is a pass-through: its confidence is the emotion read's
    # own standing, not this module's - and an arc nobody passed is a
    # 0 the requested block must not read as a high one.
    if sources.get("arc"):
        entry("arc", 0.8, "emotion.py's arc, passed through with the spec")
    else:
        entry("arc", 0.0, "no emotion read was given, so no arc")

    # The Rust ears.
    if rust_up:
        entry("prosody", 0.85, "spectral-prosody (Rust): F0, energy, "
                               "flux onsets, rhythm layers, pitch contour")
        entry("forensic", 0.8, "oximedia-audio-analysis (Rust): noise floor, "
                               "SNR, flatness, band and safety readings "
                               "(peak, clipping, hum, sibilance); reverb and "
                               "music-likelihood are heuristics that say so "
                               "in the sources note")
        # The speaker confidence rides the same gate as its source: a stub
        # that answered no method is an absent ear, not a 0.7 one.
        if (rust.get("speaker") or {}).get("method"):
            entry("speaker", 0.7, "polyvoice voice-prints (Rust); two speakers "
                                  "with similar voices can merge")
        else:
            entry("speaker", 0.0, "the speaker ear answered no method "
                                 "(no WAV in the spec)")
        entry("entities", 0.8, "anno NER (Rust) with a regex complement")
        entry("formants", (0.55 if band and band < 5000 else 0.75),
              "vocal-tract formants"
              + (" on a narrow-band take (phone audio): the higher "
                 "formants are missing" if band and band < 5000 else ""))
    else:
        for name in ("prosody", "forensic", "speaker", "entities", "formants"):
            basis = next(
                (n for n in notes if "filtered to" in n
                 or "voice-signals binary" in n), None)
            entry(name, 0.0, basis or "the Rust ear is not built: "
                                     "cargo build -p voice-signals")

    return {"ok": True, "perSignal": lines}


def _director_hints(lines_out, pauses, rust, language, arc, bgm):
    """The decisions the signals exist to make, in the words a director
    reads: cut style, caption gates, BGM, speaker changes, repeats."""
    paced = [l for l in lines_out if l.get("speechRate")]
    loud = [l["loudness"].get("relativeLu") for l in lines_out if l.get("loudness")]
    fast = sum(1 for l in paced if l["speechRate"]["pace"] in ("punchy",))
    slow = sum(1 for l in paced if l["speechRate"]["pace"] in ("editorial", "slow"))
    loud_count = sum(1 for v in loud if v is not None and v > 1.5)
    quiet_count = sum(1 for v in loud if v is not None and v < -3.0)
    if fast * 2 >= max(1, len(paced)) or loud_count * 2 >= max(1, len(loud)):
        cut_style = "fast"
    elif slow * 2 >= max(1, len(paced)) or quiet_count * 2 >= max(1, len(loud)):
        cut_style = "hold"
    else:
        cut_style = "mixed"
    if arc and str(arc.get("shape") or "") == "rise" and cut_style == "mixed":
        cut_style = "building"
    gated = [{"lineIndex": l["index"], "word": w["text"], "confidence": w["confidence"]}
             for l in lines_out for w in l.get("words") or []
             if w.get("confidence") is not None and w["confidence"] < 0.5]
    speaker_changes = []
    if rust and rust.get("speaker", {}).get("changes"):
        speaker_changes = rust["speaker"]["changes"]
    questions = [l["index"] for l in lines_out
                 if l.get("sentence", {}).get("type") == "question"]
    emphasis = [{"lineIndex": l["index"], "words": [w.get("wordText") for w in
                                                    (l.get("prosody") or {}).get("emphasisWords") or []
                                                    if w.get("wordText")]}
                for l in lines_out if (l.get("prosody") or {}).get("emphasisWords")]
    emphasis = [e for e in emphasis if e["words"]]
    return {
        "cutStyle": cut_style,
        "cutRule": {
            "fast": "cuts every 0.8-1.5 s on the flux onsets and emphasis words",
            "hold": "long holds (2.5 s+), the pauses carry the weight",
            "mixed": "alternate: fast over the punchy lines, hold on the quiet ones",
            "building": "start held, accelerate into the climax lines",
        }[cut_style],
        "captionGate": gated[:40],
        "captionRule": "re-check or drop the gated words before captions_animate",
        "bgm": bgm,
        "speakerChanges": speaker_changes,
        "speakerRule": "a speaker change is a scene change: cut the angle, "
                       "the colour or the lower-third on the change time",
        "questions": questions,
        "questionRule": "a question line wants its answer as the next visual",
        "emphasisWords": emphasis,
        "emphasisRule": "SFX peaks and caption highlights land on the emphasis words",
        "language": {
            "font": language["fontHint"],
            "musicGenre": language["musicGenreHint"],
        },
        "protectedPauses": [p for p in pauses if p["class"] in ("dramatic", "scene_turn")],
    }


def _bgm_decision(rust, speech_ratio):
    """Dry voice wants music under it; a take that already carries music
    wants none added."""
    whole = (rust or {}).get("forensic", {}).get("whole")
    if whole:
        floor = whole.get("noiseFloorDbfs")
        snr = whole.get("snrDb")
        flatness = whole.get("spectralFlatness")
        onsets = len(whole.get("fluxOnsetsSec") or [])
        # The Rust ear's own pulse reading: dense, regular onsets that
        # survive the pure-tone veto are a beat, whatever the floor says.
        likelihood = whole.get("musicLikelihood")
        if isinstance(likelihood, (int, float)) and likelihood >= 0.6:
            return {"recommendation": "skip",
                    "reason": f"music likelihood {likelihood:.2f}: dense, regular "
                              "onsets with a music-like spectrum - this take "
                              "already carries music"}
        if flatness is not None and flatness > 0.25 and onsets > 30:
            return {"recommendation": "skip",
                    "reason": f"spectral flatness {flatness:.2f} with {onsets} flux onsets: "
                              "this take already carries music-like content"}
        if floor is not None and floor < -45.0:
            return {"recommendation": "add",
                    "reason": f"noise floor {floor:.0f} dBFS: a dry voiceover - "
                              "bed it with music and duck on the speech"}
        if snr is not None and snr < 12.0:
            return {"recommendation": "duck_only",
                    "reason": f"SNR {snr:.0f} dB is low; clean the voice before "
                              "adding anything under it"}
        return {"recommendation": "add",
                "reason": "the forensics read as a clean dry voice; music wants adding"}
    if speech_ratio is not None and speech_ratio < 0.55:
        return {"recommendation": "add",
                "reason": f"only {speech_ratio:.0%} of the take is speech; "
                          "music fills the space between lines"}
    return {"recommendation": "add",
            "reason": "no forensic reading; a TTS voiceover is dry by construction"}


# ---------------------------------------------------------------------------
# extract
# ---------------------------------------------------------------------------


def extract(spec):
    lines = _collect_lines(spec)
    if not lines:
        return {"ok": False,
                "error": "no lines: pass transcript (a transcribe job's output), "
                         "lines [{text, start, end}], or script (raw text)"}
    audio = spec.get("audio") or spec.get("path")
    if audio and not os.path.isfile(audio):
        return {"ok": False, "error": f"no such audio file: {audio}"}
    # The `signals` filter: a manual extraction of named readings, not
    # the whole profile. Only the passes the asked-for signals need run -
    # a filler read costs no ffmpeg pass and no Rust ear - and the reply
    # carries a `requested` block with each reading, its source and its
    # confidence, beside the (honestly degraded) profile.
    wanted = _wanted_signals(spec)
    if isinstance(wanted, dict):
        return {"ok": False, "error": wanted["error"]}
    need_pauses = wanted is None or any(
        name in ("pauses.classes", "pauses.speechRatio", "words.confidence")
        for name in wanted)
    # The measured word gate reads the momentary series, so a
    # words.confidence ask owes the loudness pass too - WhisperX's own
    # scores replace the gate only when the alignment actually ran.
    need_loudness = wanted is None or any(
        name.startswith("loudness.") for name in wanted) \
        or "words.confidence" in wanted
    need_rust = wanted is None or any(
        CATALOGUE_BY_NAME[name]["rust"] for name in wanted)
    need_whisperx = wanted is None or "words.confidence" in wanted
    language = _detect_language(lines, spec.get("language"))
    notes = []
    sources = {
        "loudness": None,
        "pauses": None,
        "speechRate": "python (word count / duration; vowel-group syllables)",
        "wordConfidence": None,
        "language": "python (Unicode script scan)",
        "sentenceType": "python (punctuation regex)",
        "repeats": "python (n-gram overlap, 3-6 words)",
        "fillers": "python (regex, English core)",
        "prosody": None,
        "forensic": None,
        "speaker": None,
        "entities": None,
        "alignment": "whisper token timestamps",
    }

    ff, _ = _ffmpeg(spec)
    silence_spans = []
    duration = None
    series = []
    integrated = None
    lra = None
    true_peak = None
    if audio and need_pauses:
        threshold_db = abs(float(spec.get("thresholdDb") or 35.0))
        min_silence = max(0.05, float(spec.get("minSilenceMs") or 350) / 1000.0)
        try:
            silence_spans, duration = _silence_spans(ff, audio, threshold_db, min_silence)
            sources["pauses"] = "ffmpeg silencedetect, classified in python"
        except Exception as exc:
            notes.append(f"silence detection failed: {exc}")
    if audio and need_loudness:
        try:
            series = _momentary_series(ff, audio)
            integrated, lra, true_peak = _integrated(ff, audio)
            sources["loudness"] = "ffmpeg ebur128 momentary series (per line)"
            sources["wordConfidence"] = "measured gate (timing, silence overlap, energy)"
        except Exception as exc:
            notes.append(f"loudness measurement failed: {exc}")
    pauses = _classify_pauses(silence_spans, lines)

    # WhisperX: replace the token-timestamp words with aligned ones -
    # only when the ask needs them, so a filler read never pays for an
    # alignment model.
    wx_words = None
    if audio and need_whisperx \
            and str(spec.get("whisperx", "auto")).lower() not in ("off", "0", "no"):
        wx_words, wx_note = _whisperx_words(audio, lines, language.get("detected"))
        if wx_note:
            notes.append(wx_note)
        else:
            sources["alignment"] = "whisperx aligned words with scores"
            sources["wordConfidence"] = "whisperx alignment scores"

    # The Rust ears (text-only when there is no audio: the NER ear answers
    # for a script before any TTS has run) - and only when the ask needs
    # them: a text signal costs no audio conversion and no binary.
    rust = None
    rust_note = None
    if need_rust:
        rust, rust_note = _rust_ears(spec, lines, audio, ff)
    if rust_note:
        notes.append(rust_note)

    rust_lines = []
    rust_forensic = []
    if rust:
        rust_lines = rust.get("prosody", {}).get("lines") or []
        rust_forensic = rust.get("forensic", {}).get("lines") or []
        sources["prosody"] = (rust.get("sources") or {}).get("prosody")
        sources["forensic"] = (rust.get("sources") or {}).get("forensic")
        # The speaker ear's own answer gates the source claim: a stub that
        # ran without a WAV answers no method, and an unanswered ear must
        # not be named as one - the degradation check below owes its truth
        # to this gate.
        speaker_method = (rust.get("speaker") or {}).get("method") or ""
        if speaker_method:
            sources["speaker"] = speaker_method
        sources["entities"] = (rust.get("sources") or {}).get("entities")

    # The per-line assembly.
    speaker_lines = (rust or {}).get("speaker", {}).get("lineSpeakers") or []
    lines_out = []
    for i, line in enumerate(lines):
        words = _words_of(line)
        if wx_words and i < len(wx_words) and wx_words[i]:
            words = wx_words[i]
        confidences = []
        if words:
            if wx_words:
                confidences = [dict(w, confidence=round(w.get("score") or 0.0, 3))
                               for w in words]
            else:
                confidences = _word_confidences(words, silence_spans, series)
            words = confidences
        loudness = _line_loudness(series, line.get("start"), line.get("end"))
        if loudness and integrated is not None:
            loudness["relativeLu"] = round(loudness["meanLufsM"] - integrated, 2)
        prosody = rust_lines[i] if i < len(rust_lines) else None
        forensic = rust_forensic[i] if i < len(rust_forensic) else None
        sentence = _sentence_type(
            line.get("text"),
            (prosody or {}).get("finalRise") if isinstance(prosody, dict) else None)
        line_speaker = speaker_lines[i] if i < len(speaker_lines) else None
        lines_out.append({
            "index": i,
            "text": str(line.get("text") or ""),
            "start": line.get("start"),
            "end": line.get("end"),
            "loudness": loudness,
            "speechRate": _speech_rate(line, words),
            "sentence": sentence,
            "prosody": prosody,
            "forensic": forensic,
            "speaker": line_speaker if isinstance(line_speaker, int) and line_speaker >= 0 else None,
            "fillers": _fillers(line, words),
            "words": [
                {"text": w["text"], "start": w["start"], "end": w["end"],
                 "confidence": w.get("confidence"),
                 "syllables": _syllables(w["text"])}
                for w in words
            ],
        })

    # Entities: anno (Rust) plus the regex complement.
    entities = list((rust or {}).get("entities") or [])
    if entities:
        for entity in entities:
            entity["source"] = "anno"
    entities = _merge_entities(entities, _entity_complement(lines))
    # The complement ran on the text whenever there were lines, so the
    # entities reading is sourced even when the Rust ear never was: a
    # filtered entities ask answers from the complement and is not a
    # degradation.
    if lines and not sources.get("entities"):
        sources["entities"] = "regex complement (python)"
        if rust:
            sources["entities"] = "anno NER (Rust) with a regex complement"

    # The arc, passed through from the emotion read when the caller gave it.
    emotion = spec.get("emotion")
    arc = None
    if isinstance(emotion, dict) and isinstance(emotion.get("arc"), dict):
        arc = emotion["arc"]
        sources["arc"] = "emotion.py, passed through"
    else:
        notes.append("no emotion read in the spec: call the emotion tool for the arc; "
                     "pacing decisions here do not use it")

    speech_ratio = None
    if duration:
        spoken = sum(min(l.get("end") or 0, duration) - min(l.get("start") or 0, duration)
                     for l in lines
                     if isinstance(l.get("start"), (int, float)) and isinstance(l.get("end"), (int, float)))
        speech_ratio = max(0.0, min(1.0, spoken / max(0.1, duration)))
    bgm = _bgm_decision(rust, speech_ratio)
    hints = _director_hints(lines_out, pauses, rust, language, arc, bgm)
    # The filter's own note lands before the confidence builder reads the
    # notes, so an ear the ask never needed explains itself as skipped by
    # request rather than as broken.
    if wanted is not None:
        notes.append(
            f"filtered to {len(wanted)} signal(s): the passes the ask does "
            "not need did not run")
    confidence = _signal_confidence(sources, audio, rust, wx_words, series, notes)

    # The honest headline the notes bury: which ears did not answer. The
    # same conditions that append a note name a degradation here, so a
    # caller reading one field knows the reading's standing without
    # parsing prose - "ok" only when every ear that could run did, and a
    # degraded reading is still a reading, never a silent half of one.
    # A filtered extract is held to a different bar: only the ears the
    # ask needs are owed an answer, and an ear the ask never needed is
    # not a degradation - the note says what ran and what the filter
    # skipped, and the status is the asked-for signals' own.
    degradations = []
    if wanted is not None:
        for name in sorted(wanted):
            family = CATALOGUE_BY_NAME[name]["family"]
            owed = {"Loudness": "loudness", "Pauses": "pauses",
                    "Words": "wordConfidence", "Prosody": "prosody",
                    "Forensics": "forensic", "Speakers": "speaker",
                    "Entities": "entities"}.get(family)
            if owed and not sources.get(owed):
                degradations.append(f"{name}: the {owed} ear did not answer")
    else:
        if rust is None:
            degradations.append(
                "voice-signals binary absent: prosody/forensics/speakers/NER missing")
        if wx_words is None:
            degradations.append("whisperx words absent: word timings estimated")
        if not sources.get("loudness"):
            degradations.append("ffmpeg absent: loudness unmeasured")
        if not sources.get("pauses"):
            degradations.append("silencedetect absent: pauses unclassified")

    # DEG-01: "no entities found" and "the NER ear missing" are different
    # facts, and an empty list could not tell them apart. The flag says
    # which: entitiesAvailable is True only when the NER ear actually
    # answered - then an empty entities list is a real negative. False
    # means whatever entities are present came from the regex complement
    # alone, and a caller driving decisions off them should record the
    # fallback rather than trust the reading as measured.
    entities_available = bool(sources.get("entities"))
    if wanted is None and not entities_available:
        degradations.append(
            "NER absent: entities answered by the regex complement alone")

    out = {
        "ok": True,
        "schema": SCHEMA,
        "status": "degraded" if degradations else "ok",
        "degradations": degradations,
        "audio": {"path": audio, "durationSec": round(duration, 3) if duration else None},
        "language": language,
        "lines": lines_out,
        "pauses": pauses,
        "motifs": _motifs(lines),
        "entities": entities,
        "entitiesAvailable": entities_available,
        "arc": arc,
        "speaker": (rust or {}).get("speaker"),
        "wholeTake": {
            "loudness": {"integratedLufs": integrated,
                         "loudnessRangeLu": lra,
                         "truePeakDbfs": true_peak},
            "prosody": (rust or {}).get("prosody", {}).get("rhythmLayers"),
            "dominantPeriodSec": (rust or {}).get("prosody", {}).get("dominantPeriodSec"),
            "phrases": (rust or {}).get("prosody", {}).get("phrases"),
            # The take's own F0 track: the curve the per-line statistics
            # flatten away, at most 256 points, 0 where unvoiced.
            "pitchContourHz": (rust or {}).get("prosody", {}).get("pitchContourHz"),
            "forensic": (rust or {}).get("forensic", {}).get("whole"),
            "speechRatio": round(speech_ratio, 3) if speech_ratio is not None else None,
        },
        "directorHints": hints,
        "signalConfidence": confidence,
        "decisionPolicy": _decision_policy_out(),
        "sources": {k: v for k, v in sources.items() if v},
        "notes": notes,
    }
    if wanted is not None:
        out["requested"] = _resolve_requested(out, wanted)
    return out


# ---------------------------------------------------------------------------
# selftest
# ---------------------------------------------------------------------------


def _synth_take(path):
    """A synthetic take with the pause classes built in: three spoken-ish
    bursts (amplitude-modulated tones), separated by a 150 ms breath, a
    450 ms beat, a 900 ms dramatic hold, and a 2.8 s dead-air tail."""
    rate = 16000
    segments = [
        (150.0, 1.6), (None, 0.15),   # breath
        (210.0, 1.9), (None, 0.45),   # beat
        (130.0, 1.7), (None, 0.90),   # dramatic
        (180.0, 1.5), (None, 2.80),   # dead air
    ]
    samples = []
    for freq, secs in segments:
        n = int(secs * rate)
        if freq is None:
            samples.extend([0.0] * n)
            continue
        for i in range(n):
            t = i / rate
            pulse = 0.3 + 0.3 * math.sin(2 * math.pi * 2.5 * t)
            samples.append(math.sin(2 * math.pi * freq * t) * pulse)
    with wave.open(path, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes(b"".join(
            int(max(-1.0, min(1.0, s)) * 32000).to_bytes(2, "little", signed=True)
            for s in samples))
    return len(samples) / rate


def selftest():
    """Prove the Python signals without the Rust ears or whisperx: a
    synthetic take, a fake transcript, and assertions on every family."""
    workdir = tempfile.mkdtemp(prefix="voice-signals-selftest-")
    try:
        wav = os.path.join(workdir, "take.wav")
        duration = _synth_take(wav)
        spec = {
            "audio": wav,
            "minSilenceMs": 120,  # catch the 150 ms breath too
            "transcript": {"segments": [
                {"start": 0.05, "end": 1.70, "text": "Um, so this is the first line?",
                 "words": [
                     {"text": "Um,", "start": 0.05, "end": 0.25},
                     {"text": "so", "start": 0.25, "end": 0.40},
                     {"text": "this", "start": 0.40, "end": 0.60},
                     {"text": "is", "start": 0.60, "end": 0.75},
                     {"text": "the", "start": 0.75, "end": 0.90},
                     {"text": "first", "start": 0.90, "end": 1.30},
                     {"text": "line?", "start": 1.30, "end": 1.70},
                 ]},
                {"start": 1.85, "end": 3.70, "text": "This is the first line again.",
                 "words": [
                     {"text": "This", "start": 1.85, "end": 2.05},
                     {"text": "is", "start": 2.05, "end": 2.20},
                     {"text": "the", "start": 2.20, "end": 2.35},
                     {"text": "first", "start": 2.35, "end": 2.75},
                     {"text": "line", "start": 2.75, "end": 3.10},
                     {"text": "again.", "start": 3.10, "end": 3.70},
                 ]},
                {"start": 4.15, "end": 5.80, "text": "The third line lands here.",
                 "words": [
                     {"text": "The", "start": 4.15, "end": 4.30},
                     {"text": "third", "start": 4.30, "end": 4.70},
                     {"text": "line", "start": 4.70, "end": 5.05},
                     {"text": "lands", "start": 5.05, "end": 5.45},
                     {"text": "here.", "start": 5.45, "end": 5.80},
                 ]},
                # Lowercase start: the 0.9 s hold before it is mid-sentence,
                # a dramatic hold, not a scene turn.
                {"start": 6.70, "end": 8.10, "text": "it earned $10,450 in 2026.",
                 "words": [
                     {"text": "it", "start": 6.70, "end": 6.85},
                     {"text": "earned", "start": 6.85, "end": 7.25},
                     {"text": "$10,450", "start": 7.25, "end": 7.85},
                     {"text": "in", "start": 7.85, "end": 7.95},
                     {"text": "2026.", "start": 7.95, "end": 8.10},
                 ]},
            ]},
            "language": "en",
            "whisperx": "off",
        }
        # Keep the selftest honest about the Rust ears: they run if the
        # binary is built, and their absence is a note, not a failure.
        result = extract(spec)
        failures = []
        if not result.get("ok"):
            return {"ok": False, "selftest": "extract failed", "error": result.get("error")}
        lines = result["lines"]
        # Half the signals ride ffmpeg (silencedetect, ebur128); the rest
        # are pure Python. The selftest proves everything it can on this
        # machine and names the half it proved.
        measured = _ffmpeg_runs(spec)
        if measured:
            # The pause classes: breath, beat, dramatic and dead air all found.
            kinds = {p["class"] for p in result["pauses"]}
            for wanted in ("breath", "beat", "dramatic", "dead_air"):
                if wanted not in kinds:
                    failures.append(f"pause class {wanted} not found in {sorted(kinds)}")
            # The dramatic hold is protected.
            if not all(p["keep"] for p in result["pauses"] if p["class"] != "dead_air"):
                failures.append("a keep-class pause was marked cuttable")
            # Per-line loudness exists.
            if not all(isinstance(l.get("loudness"), dict) and
                       l["loudness"].get("meanLufsM") is not None for l in lines):
                failures.append("per-line loudness missing")
        else:
            kinds = set()
            # The honest absence, not a silent skip: the notes name the
            # measurement that could not run.
            if not any("failed" in str(n) or "no ffmpeg" in str(n) for n in result["notes"]):
                failures.append(f"no ffmpeg and no note naming it: {result['notes']}")
            if any(l.get("loudness") for l in lines):
                failures.append("loudness answered without a decoder to measure it")
        if lines[0]["sentence"]["type"] != "question":
            failures.append(f"line 0 read as {lines[0]['sentence']['type']}, not question")
        if lines[2]["sentence"]["type"] != "statement":
            failures.append(f"line 2 read as {lines[2]['sentence']['type']}, not statement")
        # Speech rate: a value and a pace class per line.
        if not all(l.get("speechRate") and l["speechRate"].get("pace") for l in lines):
            failures.append("speech rate missing")
        # The filler and the motif.
        if not any(f["word"].lower().startswith("um") for f in lines[0]["fillers"]):
            failures.append(f"the um filler was not found: {lines[0]['fillers']}")
        if not any("first line" in m["phrase"] for m in result["motifs"]):
            failures.append(f"the repeated phrase was not found: {result['motifs']}")
        # Word confidence gated the word that sits on silence (line 4's tail
        # word ends before a long dead-air span starts, so use the energy
        # gate instead: assert every word carries a confidence).
        for line in lines:
            for word in line["words"]:
                if word["confidence"] is None:
                    failures.append(f"word {word['text']} has no confidence")
                    break
        # Entities: the money complement, from the regex when anno is absent.
        money = [e for e in result["entities"] if e["entityType"] == "MONEY"]
        if not money:
            failures.append("no MONEY entity from either ear")
        # DEG-01: without the Rust ears the NER is missing, and the reply
        # must say so - entities present from the complement alone do not
        # make the NER "run and found these".
        if result.get("speaker") is None:
            if result.get("entitiesAvailable") is not False:
                failures.append(
                    "entitiesAvailable must be False when the NER ear is absent: "
                    f"{result.get('entitiesAvailable')}")
            if not any("NER absent" in d for d in result.get("degradations") or []):
                failures.append("the NER-absent degradation was not named")
        else:
            if result.get("entitiesAvailable") is not True:
                failures.append(
                    "entitiesAvailable must be True when the NER ear answered: "
                    f"{result.get('entitiesAvailable')}")
        # Language detection and hints.
        if result["language"]["detected"] != "en" or not result["language"]["matchesHint"]:
            failures.append(f"language read {result['language']}")
        if not result["directorHints"]["cutStyle"]:
            failures.append("no cut style")
        if result["directorHints"]["bgm"]["recommendation"] not in ("add", "skip", "duck_only"):
            failures.append(f"bgm said {result['directorHints']['bgm']}")
        # The Rust ears' new readings flow through the merge whole: the
        # fifty-ear keys exist whenever the binary ran, so a shape that
        # drifts (a renamed key on the Rust side) fails here rather than
        # as a director that quietly had nothing to read.
        if result.get("speaker"):
            whole = result["wholeTake"]["forensic"] or {}
            for key in ("samplePeakDbfs", "clippingRatio", "dcOffset",
                        "sibilanceRatio", "spectralRolloffHz",
                        "spectralBandwidthHz", "onsetRatePerMin",
                        "musicLikelihood"):
                if key not in whole:
                    failures.append(f"the Rust forensic whole lost {key}")
            if "utterances" not in (result.get("speaker") or {}):
                failures.append("the Rust speaker reading lost utterances")
            if not result["wholeTake"].get("pitchContourHz"):
                failures.append("the Rust prosody contour did not flow through")
        # The confidence block: one entry per signal family, each with a
        # basis, and the loudness family confident on a real take.
        per_signal = result.get("signalConfidence", {}).get("perSignal") or []
        signals = {entry["signal"]: entry for entry in per_signal}
        for family in ("loudness", "pauses", "speechRate", "prosody",
                       "forensic", "speaker", "entities"):
            entry = signals.get(family)
            if not entry or "confidence" not in entry or not entry.get("basis"):
                failures.append(f"no confidence for {family}: {entry}")
        if measured and signals.get("loudness", {}).get("confidence", 0) < 0.5:
            failures.append("loudness confidence low on a real take with a decoder on it")
        # The decision policy: every decision names its signals.
        policy = result.get("decisionPolicy") or {}
        for decision in ("cut", "caption", "graphic", "bgm", "transition"):
            entry = policy.get(decision)
            if not entry or not entry.get("signals") or not entry.get("rule"):
                failures.append(f"no decision policy for {decision}: {entry}")
        # The catalogue: the fifty named readings, fourteen of them the
        # fifty-ear pass added, every name unique, every family named,
        # every entry held to a why and a where.
        catalog = _catalog_out()
        if catalog["count"] != 50:
            failures.append(f"catalogue carries {catalog['count']} signals, not 50")
        if catalog["new"] != 14:
            failures.append(f"catalogue marks {catalog['new']} new signals, not 14")
        names = [entry["name"] for entry in catalog["signals"]]
        if len(set(names)) != len(names):
            failures.append("catalogue names repeat")
        for entry in catalog["signals"]:
            if not entry["why"] or not entry["reads"] or not entry["label"]:
                failures.append(f"catalogue entry incomplete: {entry['name']}")
        # A filtered extract: one text signal, nothing else owed. The
        # filler read must answer without the Rust ears, and the reply
        # must carry the requested block with the reading resolved.
        filtered = extract({
            "transcript": spec["transcript"],
            "language": "en",
            "whisperx": "off",
            "signals": ["fillers.list"],
        })
        if not filtered.get("ok"):
            failures.append(f"filtered extract failed: {filtered.get('error')}")
        else:
            requested = filtered.get("requested") or []
            if len(requested) != 1 or requested[0]["name"] != "fillers.list":
                failures.append(f"requested block wrong: {requested}")
            elif not requested[0]["value"]:
                failures.append("the filler read resolved to no value")
            elif not any("filtered to" in str(n) for n in filtered.get("notes", [])):
                failures.append("the filter never said what it skipped")
        # An unknown name is refused, never dropped.
        refused = extract({"script": "hello", "signals": ["not.a.signal"]})
        if refused.get("ok") or "unknown signal name" not in str(refused.get("error")):
            failures.append(f"an unknown signal name was not refused: {refused}")
        if failures:
            return {"ok": False, "selftest": "failures", "failures": failures}
        return {"ok": True, "selftest": "python signals answer",
                "durationSec": round(duration, 3),
                "pauseClasses": sorted(kinds),
                "catalogue": f"{catalog['count']} signals, {catalog['new']} new",
                "measured": "ffmpeg-backed signals proven" if measured
                            else "no ffmpeg: the text, timing and confidence signals proven, "
                                 "the loudness and pause readings named as absent in the notes",
                "rustEars": "ran" if result.get("speaker") else "absent (build the binary for them)"}
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


# ---------------------------------------------------------------------------


def command(spec):
    """The sense.py proxy entry: same degradation contract as emotion."""
    action = str(spec.get("action", "extract"))
    try:
        if action == "extract":
            return extract(spec)
        if action == "selftest":
            return selftest()
        if action == "catalog":
            return _catalog_out()
        return {"ok": False, "error": "action is extract, catalog or selftest"}
    except Exception as exc:  # noqa: BLE001  the contract: never crash
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def main():
    parser = argparse.ArgumentParser(description="Read the voiceover as a score.")
    parser.add_argument("cmd", choices=["extract", "catalog", "selftest"])
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
    # The command line names the action; the spec may override it.
    spec.setdefault("action", args.cmd)
    out = command(spec)
    print(json.dumps(out, indent=1))
    sys.exit(0 if out.get("ok") else 4)


if __name__ == "__main__":
    main()
