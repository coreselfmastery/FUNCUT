#!/usr/bin/env python3
"""concat-voice-signals: the voiceover read as a score, not just words.

Pure Python standard library, same contract as the other sense scripts:
`voice_signals.py <extract|catalog|selftest> --spec -` reads one JSON spec
from stdin (or argv) and prints one JSON object on stdout.

`catalog` answers the signal catalogue: the seventy-one named readings, grouped
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
import difflib
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
    "motion": {
        "signals": ["motion.emphasisHits", "motion.kineticScale",
                    "motion.pitchDirection", "motion.pitchTravel",
                    "motion.rateTempo", "motion.pauseDwell",
                    "motion.onsetSync", "motion.rhythmStagger",
                    "motion.sentenceEntry", "motion.emotionEase",
                    "motion.arcTurn", "motion.entityCard",
                    "motion.topicChapter", "motion.motifReturn",
                    "motion.speakerTurn", "motion.confidenceGate",
                    "motion.sibilanceTexture", "motion.reverbSpace",
                    "motion.noiseGrain", "motion.levelPulse"],
        "rule": "The motion-design read: emphasis and onsets set hit timing, "
                "pitch and level set travel, rate and rhythm set speed and "
                "stagger, pauses and sentences set holds and entries, arc and "
                "emotion set easing, entities/topics/motifs/speakers set hero "
                "cards and callbacks, and the forensic texture sets grain, "
                "shimmer and background space. Every motion choice names the "
                "reading that moved it.",
    },
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
# The signal catalogue: the seventy-one named readings, one button each
# ---------------------------------------------------------------------------

# Every reading the profile answers, named, grouped by family, and held
# to a why: the decision (or decisions) the edit spends it on, in the
# decision policy's own vocabulary. `where` says where the reading lands
# - ("take", <dotted path from the reply's root>) for a whole-profile
# reading, ("line", <dotted path within one line row>) for a per-line
# one - so a filtered extract can resolve exactly what was asked. `rust`
# marks the readings that need the binary's ears; `new` marks the
# thirty-four the two catalogue passes added.
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
    {"name": "loudness.momentaryLufs", "family": "Loudness",
     "label": "Momentary curve",
     "reads": "The ebur128 momentary-loudness curve, at most 256 [t, LUFS] "
              "points, times in file seconds.",
     "why": "The bed's volume ride keys itself on the measured envelope: "
            "the duck follows the voice's own rise and fall, not a span "
            "average (decision: the bed's volume keys).",
     "where": ("take", "wholeTake.loudness.momentaryLufs"), "rust": False,
     "new": False},
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
    {"name": "speechRate.wordsPerMinute", "family": "Speech rate",
     "label": "Words per minute",
     "reads": "Words per minute, per line.",
     "why": "The pacing class (punchy, conversational, slow) sizes every "
            "shot's drift (decision: pacing).",
     "where": ("line", "speechRate.wordsPerMinute"), "rust": False, "new": False},
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
              "energy) otherwise - floored at 0.85 for the words a script "
              "confirmed, and carrying `corrected` on the ones whose "
              "spelling the script replaced.",
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
              "earns (stat, lower third, timeline), whether the NER "
              "measured it or the regex complement found it, and - for the "
              "numbers - the parsed value and unit ($10,450 is 10450 USD, "
              "45% is 45 percent) with the char span the words occupy.",
     "why": "Money becomes a stat card, a name a lower third, a date a "
            "timeline (decision: graphic) - and an approximate reading "
            "earns no card that claims exactness. The parsed value and "
            "unit are what a chart or a stat card can plot, not only "
            "quote.",
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
    # The motion-design family: twenty readings that turn the voice profile
    # into motion choices. They add no audio pass; they are derived from
    # the readings above and name the motion decision each one moves.
    {"name": "motion.emphasisHits", "family": "Motion design",
     "label": "Emphasis hits",
     "reads": "Every stressed word with its time, strength and suggested "
              "scale pop.",
     "why": "A kinetic word lands on the emphasis, not on the sentence "
            "start (decision: motion).",
     "where": ("take", "motion.emphasisHits"), "rust": True, "new": True},
    {"name": "motion.kineticScale", "family": "Motion design",
     "label": "Kinetic scale",
     "reads": "A per-line scale travel from emphasis strength and pitch "
              "range.",
     "why": "A performed line grows further than a reported one; the card "
            "never scales by a guessed constant (decision: motion).",
     "where": ("take", "motion.kineticScale"), "rust": True, "new": True},
    {"name": "motion.pitchDirection", "family": "Motion design",
     "label": "Pitch direction",
     "reads": "Whether the line's pitch rises, falls or stays flat, with "
              "the measured slope.",
     "why": "A rising line moves up-screen; a falling line settles down - "
            "the motion follows the voice (decision: motion).",
     "where": ("take", "motion.pitchDirection"), "rust": True, "new": True},
    {"name": "motion.pitchTravel", "family": "Motion design",
     "label": "Pitch travel",
     "reads": "The slope converted to a bounded pixel travel.",
     "why": "The keyframe's vertical distance, so a steep question lifts "
            "further than a flat statement (decision: motion).",
     "where": ("take", "motion.pitchTravel"), "rust": True, "new": True},
    {"name": "motion.rateTempo", "family": "Motion design",
     "label": "Rate tempo",
     "reads": "Words per minute converted to a 0.6-1.6 animation speed.",
     "why": "Punchy delivery moves faster; slow delivery lets a card "
            "breathe (decision: motion).",
     "where": ("take", "motion.rateTempo"), "rust": True, "new": True},
    {"name": "motion.pauseDwell", "family": "Motion design",
     "label": "Pause dwell",
     "reads": "Each classified pause with its hold and frame dwell.",
     "why": "A dramatic hold keeps the frame still; a scene turn is where "
            "the next hero enters (decision: motion).",
     "where": ("take", "motion.pauseDwell"), "rust": True, "new": True},
    {"name": "motion.onsetSync", "family": "Motion design",
     "label": "Onset sync",
     "reads": "The spectral-flux onsets, timed and mapped to their line.",
     "why": "Hits, flashes and SFX land on measured transients rather than "
            "on a metronome (decision: motion).",
     "where": ("take", "motion.onsetSync"), "rust": True, "new": True},
    {"name": "motion.rhythmStagger", "family": "Motion design",
     "label": "Rhythm stagger",
     "reads": "The rhythm layers and the stagger they imply.",
     "why": "Multi-layer cards enter on the delivery's own rhythm instead "
            "of all at once (decision: motion).",
     "where": ("take", "motion.rhythmStagger"), "rust": True, "new": True},
    {"name": "motion.sentenceEntry", "family": "Motion design",
     "label": "Sentence entry",
     "reads": "The sentence type mapped to the preset its entry wants.",
     "why": "A question opens a kinetic caption, an exclamation a sting, a "
            "fragment a pull quote (decision: motion).",
     "where": ("take", "motion.sentenceEntry"), "rust": True, "new": True},
    {"name": "motion.emotionEase", "family": "Motion design",
     "label": "Emotion ease",
     "reads": "The arc shape mapped to a spring, emphasis, ease-out or "
              "linear curve.",
     "why": "The easing is the emotional read's own shape, not a house "
            "default (decision: motion).",
     "where": ("take", "motion.emotionEase"), "rust": True, "new": True},
    {"name": "motion.arcTurn", "family": "Motion design",
     "label": "Arc turn",
     "reads": "Where valence or arousal turns in the emotional arc.",
     "why": "The turn is the scene change: the background and hero swap "
            "there, not on an arbitrary cut (decision: motion).",
     "where": ("take", "motion.arcTurn"), "rust": True, "new": True},
    {"name": "motion.entityCard", "family": "Motion design",
     "label": "Entity hero",
     "reads": "Each timed entity with the card it earns and its parsed "
              "value.",
     "why": "A number, name or date gets a hero card whose motion is sized "
            "to its importance (decision: motion).",
     "where": ("take", "motion.entityCard"), "rust": True, "new": True},
    {"name": "motion.topicChapter", "family": "Motion design",
     "label": "Chapter entry",
     "reads": "Scene-turn pauses as chapter entries with the next line's "
              "headline.",
     "why": "A chapter card enters on the topic change and leaves before "
            "the narration resumes (decision: motion).",
     "where": ("take", "motion.topicChapter"), "rust": True, "new": True},
    {"name": "motion.motifReturn", "family": "Motion design",
     "label": "Motif return",
     "reads": "Every repeated phrase and where it returns.",
     "why": "The same visual returns with the same motion, so the callback "
            "reads as design (decision: motion).",
     "where": ("take", "motion.motifReturn"), "rust": True, "new": True},
    {"name": "motion.speakerTurn", "family": "Motion design",
     "label": "Speaker turn",
     "reads": "Each speaker change, timed and mapped to its line.",
     "why": "A turn flips the lower-third and the frame side; a monologue "
            "keeps one hero (decision: motion).",
     "where": ("take", "motion.speakerTurn"), "rust": True, "new": True},
    {"name": "motion.confidenceGate", "family": "Motion design",
     "label": "Confidence gate",
     "reads": "The words under 0.5 confidence with their times.",
     "why": "An unsure word gets no glitch, no scale pop and no hero - the "
            "motion never celebrates a guess (decision: motion).",
     "where": ("take", "motion.confidenceGate"), "rust": True, "new": True},
    {"name": "motion.sibilanceTexture", "family": "Motion design",
     "label": "Sibilance texture",
     "reads": "The take's sibilance ratio as a shimmer intensity.",
     "why": "A bright, sibilant voice can carry a light shimmer; a dull one "
            "gets none (decision: motion).",
     "where": ("take", "motion.sibilanceTexture"), "rust": True, "new": True},
    {"name": "motion.reverbSpace", "family": "Motion design",
     "label": "Reverb space",
     "reads": "The reverb tail as a background blur and parallax depth.",
     "why": "A roomy take earns a deeper, softer background; a dry one "
            "stays crisp (decision: motion).",
     "where": ("take", "motion.reverbSpace"), "rust": True, "new": True},
    {"name": "motion.noiseGrain", "family": "Motion design",
     "label": "Noise grain",
     "reads": "The floor and spectral flatness as a grain intensity.",
     "why": "A noisy take wears a matching grain so the design sits in the "
            "same world as the picture (decision: motion).",
     "where": ("take", "motion.noiseGrain"), "rust": True, "new": True},
    {"name": "motion.levelPulse", "family": "Motion design",
     "label": "Level pulse",
     "reads": "Each line's lift over the take as a scale pulse.",
     "why": "A lifted line pulses a little harder; a quiet one holds back "
            "(decision: motion).",
     "where": ("take", "motion.levelPulse"), "rust": True, "new": True},
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
    """The catalogue whole: what the seventy signals are and why each one
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
        "Speakers": "speaker", "Motion design": "motion",
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
        "Speakers": "speaker", "Motion design": "motion",
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
# The script↔transcript alignment: the script's wording over measured times
# ---------------------------------------------------------------------------


def _script_text(spec):
    """The raw script string when the spec carries one, else None.

    A transcript and a script are different artefacts: the transcript is
    what was SAID (measured times, ASR-mangled wording), the script is
    what was MEANT (clean wording, no times). When the caller gives both,
    the script must not become the lines - its times are fiction - but
    its wording is the authority the transcript's mishearings are
    corrected against. This reader exists so `_collect_lines` can stay
    exactly as it is: the transcript wins the lines, the script wins the
    words.
    """
    script = spec.get("script")
    if isinstance(script, str) and script.strip():
        return script
    return None


def _norm_word(word):
    """A word reduced to what a comparison should see: lowercase, letters
    and digits only, punctuation gone. The character class keeps
    everything from U+0080 up on purpose - an ASCII-only strip would
    reduce every Bengali or Hindi word to the empty string, and a
    non-Latin narration would align to nothing at all."""
    return re.sub(r"[^a-z0-9\u0080-\uffff]+", "", str(word).lower())


def _align_script(lines, script):
    """Align the transcript's lines with the script's sentences, in place,
    and return the summary the profile carries as scriptAlignment.

    The rule the whole function serves: the transcript's TIMES are
    measured and the script's WORDS are authored, so the alignment lets
    the script's wording correct the transcript's while every start and
    end survives untouched. Matching runs two levels, both monotonic -
    a repeated phrase ("this is the first line" twice in a take) must
    not pull every later line back onto the first sentence that fits:

    - sentence level: each line picks the best-ratio script sentence
      from a forward window of four (a line may skip short scripted
      asides, never reach back), and a pair only counts at ratio 0.30.
      At 0.50 the script sentence replaces the line's text outright -
      below that the wording is too weakly paired to override what was
      actually recorded, and the transcript text stays.
    - word level: a difflib pass over the normalised words. `equal`
      blocks are words the script confirms; a 1:1 `replace` is the ASR
      mishearing shape ("maket" for "market") and the script word
      replaces the transcript word's text, keeping its times; n:m
      blocks are left alone - which side of a merge an author meant is
      a guess, and this module does not guess. Words said but not
      scripted (deletes) are kept and counted: they were spoken.
      Script words never spoken (inserts) are the omissions.

    The per-line bookkeeping lands on the line dict under a private
    `_script` key (word indices the assembly reads and then strips);
    nothing private is allowed to survive into a payload.
    """
    sentences = [piece.strip() for piece in
                 re.split(r"(?<=[.!?])\s+|\n+", str(script).strip())
                 if piece.strip()]
    sentence_words = [sentence.split() for sentence in sentences]
    sentence_norm = [[_norm_word(w) for w in words] for words in sentence_words]

    matched = 0
    corrected_words = 0
    confirmed_words = 0
    insertions = 0
    omissions = 0
    pairings = []
    cursor = 0
    for index, line in enumerate(lines):
        text = str(line.get("text") or "")
        # The word dicts `_words_of` will keep (dict, non-empty text,
        # parseable times), so the indices recorded here are the same
        # indices the per-line assembly iterates - an untimed word
        # dropped by the one would otherwise shift every index recorded
        # by the other. A line with no timed words still aligns, over
        # its split text; it just has no words to annotate.
        word_dicts = []
        for word in line.get("words") or []:
            if not isinstance(word, dict):
                continue
            word_text = str(word.get("text") or word.get("word") or "").strip()
            try:
                float(word.get("start"))
                float(word.get("end"))
            except (TypeError, ValueError):
                continue
            if word_text:
                word_dicts.append(word)
        if word_dicts:
            t_words = [str(w.get("text") or w.get("word") or "").strip()
                       for w in word_dicts]
        else:
            t_words = text.split()
        t_norm = [_norm_word(w) for w in t_words]
        t_joined = " ".join(w for w in t_norm if w)

        # The sentence pick: best ratio over the forward window. Four is
        # wide enough to step over a short scripted aside the speaker
        # skipped, and narrow enough that a line cannot reach into a
        # later section and steal its sentence.
        best_j = None
        best_ratio = 0.0
        for j in range(cursor, min(cursor + 4, len(sentences))):
            s_joined = " ".join(w for w in sentence_norm[j] if w)
            ratio = difflib.SequenceMatcher(None, t_joined, s_joined).ratio()
            if ratio > best_ratio:
                best_ratio, best_j = ratio, j
        if best_j is None or best_ratio < 0.30:
            # No honest pairing: the line keeps its text, its words and
            # its silence - an unpaired line is a fact about the take,
            # not an error to paper over.
            continue
        matched += 1
        cursor = best_j + 1
        s_words = sentence_words[best_j]
        s_norm = sentence_norm[best_j]

        confirmed = []
        corrected = []
        differed = []
        for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(
                None, t_norm, s_norm).get_opcodes():
            if tag == "equal":
                confirmed.extend(range(i1, i2))
            elif tag == "replace":
                if i2 - i1 == 1 and j2 - j1 == 1 and s_norm[j1]:
                    # One transcript word against one script word. The
                    # design brief drew a 0.55 near-miss ratio here (the
                    # "maket"/"market" ASR mishearing scores 0.91) to
                    # split a mishearing from a rewrite - and then made
                    # both arms replace anyway, because the script is
                    # authoritative at any ratio. So no ratio gate: the
                    # script word takes the transcript word's place and
                    # its measured times, and the index is marked
                    # corrected so the assembly floors the word's
                    # confidence. Only a script word that normalises to
                    # nothing (a bare dash the splitter made a word of)
                    # is left alone.
                    if word_dicts and i1 < len(word_dicts):
                        word_dicts[i1]["text"] = s_words[j1]
                    corrected.append(i1)
                else:
                    # n:m: a merge or a split no word-by-word pass can
                    # apportion honestly. The transcript words stay as
                    # spoken and are recorded as differed.
                    differed.extend(range(i1, i2))
            elif tag == "delete":
                # Said but not scripted - keep the words (they were
                # spoken) and count them so the summary can say it.
                insertions += i2 - i1
            elif tag == "insert":
                # Scripted but never spoken.
                omissions += j2 - j1

        confirmed_words += len(confirmed)
        corrected_words += len(corrected)
        if best_ratio >= 0.50:
            line["text"] = sentences[best_j]
        if word_dicts:
            line["_script"] = {"confirmed": confirmed, "corrected": corrected,
                               "differed": differed}
        pairings.append({"line": index, "sentence": best_j,
                         "ratio": round(best_ratio, 3),
                         "textReplaced": best_ratio >= 0.50})

    total = len(lines)
    coverage = round(matched / total, 3) if total else 0.0
    if coverage >= 0.6:
        verdict = "aligned"
    elif coverage >= 0.2:
        verdict = "partial"
    else:
        verdict = "unaligned"
    return {
        "matched": matched,
        "unmatched": total - matched,
        "correctedWords": corrected_words,
        "confirmedWords": confirmed_words,
        "insertions": insertions,
        "omissions": omissions,
        "scriptSentences": len(sentences),
        "transcriptLines": total,
        "coverage": coverage,
        "verdict": verdict,
        # The pairings, recorded whether the sentence's text replaced the
        # line's or not: a director re-checking one line wants to see
        # which script sentence it was held against, and at what ratio.
        "pairings": pairings,
    }


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


def _source_rate_hz(audio):
    """The source file's own sample rate, as ffprobe reads it. The
    formants confidence gate and the loudness basis both want the
    SOURCE's band: the Rust ear measures over a 16 kHz mono handoff,
    so its own rate says nothing about the take's band, and the
    forensic `whole` block the old probe read carries no rate at all.
    `None` when there is no audio, no probe, or no answer - an
    unmeasured band, not a wide one."""
    if not audio:
        return None
    _, probe = _ffmpeg({})
    rc, out, _ = _run([probe, "-v", "error", "-select_streams", "a:0",
                       "-show_entries", "stream=sample_rate",
                       "-of", "default=noprint_wrappers=1:nokey=1", str(audio)], 30)
    if rc != 0:
        return None
    try:
        rate = int((out or b"").decode(errors="replace").strip().splitlines()[0])
    except (IndexError, ValueError):
        return None
    return rate if rate > 0 else None


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


def _decimated_momentary(series, most=256):
    """The momentary curve itself, decimated to at most `most` points:
    [[t, LUFS], ...], unmeasured readings dropped, times ascending,
    values rounded for the wire. The per-line fold answers 'how loud was
    this line'; this is the shape the fold flattened away - the envelope
    a bed's volume ride keys itself on, so the duck follows the voice's
    measured rise instead of a span average."""
    points = [(t, m) for t, m, _ in series if m is not None]
    if not points:
        return None
    if len(points) <= most:
        picked = points
    else:
        step = len(points) / most
        last = len(points) - 1
        picked = [points[min(last, int(i * step))] for i in range(most)]
    return [[round(t, 3), round(m, 1)] for t, m in picked]


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

# The value/unit readers: what a MONEY or PERCENT entity is worth, as a
# number a stat card can format. The raw text "$10,450" is a string every
# UI would have to parse again, and every UI parses it differently - one
# drops the commas, one keeps them, one treats "lakh" as a word. Here it
# is parsed once, the same way, for the anno entities and the regex
# complement alike.
_CURRENCY_SYMBOLS = (("$", "USD"), ("£", "GBP"), ("€", "EUR"),
                     ("৳", "BDT"), ("₹", "INR"), ("¥", "JPY"))
_CURRENCY_WORDS = re.compile(
    r"\b(dollars?|euros?|pounds?|rupees?|taka|yen|cents?)\b", re.I)
# The multiplier rides the number: "3.5 million", "10k", "2 lakh". The
# long words are tried before the single letters and the whole group sits
# behind a word boundary, so the "b" of "10 bucks" is never read as
# billions - the silent order bug a naive (k|m|b) alternation ships.
_MONEY_NUMBER = re.compile(
    r"([0-9][0-9,]*(?:\.[0-9]+)?)\s*(million|billion|trillion|lakh|crore|k|m|b)?\b",
    re.I)
_MULTIPLIERS = {"k": 1e3, "m": 1e6, "b": 1e9, "million": 1e6,
                "billion": 1e9, "trillion": 1e12, "lakh": 1e5, "crore": 1e7}


def _entity_value_unit(text, entity_type):
    """(value, unit) for an entity a stat card formats, (None, None) when
    the text carries no number this reader trusts.

    MONEY: a symbol names the currency ("$10,450" → 10450 USD, "৳500" →
    500 BDT) and multiplies before it lands in the value ("€3.5 million"
    → 3500000 EUR). Without a symbol a currency word names itself
    ("10,450 dollars" → 10450 dollars) and a South-Asian multiplier
    names itself too ("10 lakh" → 1000000 lakh; "₹2 crore" is 20000000
    INR, the symbol wins and the multiplier still multiplies). PERCENT:
    the number before the sign. DATE/CARDINAL/QUANTITY: a bare 1500-2100
    integer is a year; any other number is a count. Everything else is
    left as text for the UI to render - a value this reader guessed
    would be worse than none.
    """
    text = str(text or "").strip()
    if not text:
        return None, None
    kind = str(entity_type or "").strip().upper()

    def number_of(raw):
        try:
            return float(str(raw).replace(",", ""))
        except ValueError:
            return None

    def tidy(value):
        # Integral floats become ints ("10450.0" reads as a bug in a
        # card) and the multipliers' float noise is sanded off first.
        value = round(value, 6)
        if abs(value - round(value)) < 1e-6:
            return int(round(value))
        return round(value, 3)

    if kind == "MONEY":
        currency = next((code for symbol, code in _CURRENCY_SYMBOLS
                         if symbol in text), None)
        match = _MONEY_NUMBER.search(text)
        if not match:
            return None, None
        value = number_of(match.group(1))
        if value is None:
            return None, None
        multiplier = (match.group(2) or "").lower()
        if multiplier:
            value *= _MULTIPLIERS[multiplier]
        if currency:
            return tidy(value), currency
        word = _CURRENCY_WORDS.search(text)
        if word:
            return tidy(value), word.group(0).lower()
        if multiplier:
            return tidy(value), multiplier
        return tidy(value), None
    if kind == "PERCENT":
        match = re.search(r"([0-9][0-9,]*(?:\.[0-9]+)?)\s*%", text)
        if not match:
            return None, None
        value = number_of(match.group(1))
        if value is None:
            return None, None
        return tidy(value), "percent"
    if kind in ("DATE", "CARDINAL", "QUANTITY"):
        # A bare four-digit 1500-2100 integer is a year whatever the NER
        # labelled it - a "CARDINAL 2026" is still the year 2026 - and a
        # year carries no count unit. Other dates ("March 4, 2026") are
        # left alone: which number in a written date is THE value is a
        # judgement this reader declines to make.
        if re.fullmatch(r"[0-9]{4}", text):
            year = int(text)
            if 1500 <= year <= 2100:
                return year, "year"
        if kind == "DATE":
            return None, None
        match = re.search(r"[0-9][0-9,]*(?:\.[0-9]+)?", text)
        if not match:
            return None, None
        value = number_of(match.group(0))
        if value is None:
            return None, None
        return tidy(value), "count"
    return None, None


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
                # The span here is exact - the regex found the text
                # itself, so the offsets are the match's own, not a
                # search - and the value/unit ride the same helper the
                # anno entities are enriched with, so the two sources of
                # a MONEY agree on what it is worth.
                value, unit = _entity_value_unit(match.group(0), kind)
                entry = {
                    "lineIndex": i,
                    "text": match.group(0),
                    "entityType": kind,
                    "confidence": 0.75,
                    "start": start,
                    "end": end,
                    "approximate": True,
                    "card": card,
                    "span": [span[0], span[1]],
                    "source": "regex-complement",
                }
                if value is not None:
                    entry["value"] = value
                if unit is not None:
                    entry["unit"] = unit
                out.append(entry)
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


def _enrich_entities(entities, lines):
    """Give every entity the number it is worth and the span it sits at.

    The value/unit parse turns "$10,450" into 10450 USD, a stat card
    formats a number, and the regex complement already carries its own -
    this pass is for the anno entities, which arrive as raw text. The
    span is the entity's character offsets in its line's text (first
    occurrence), the same coordinates the Rust ear's word mapping uses,
    so a UI highlighting the line can lift the entity out without
    re-finding it. Both are additive and never overwrite what an ear
    measured itself; an entity whose text cannot be found in its line
    (a drifted transcript) simply carries no span rather than a wrong
    one.
    """
    for entity in entities:
        if not isinstance(entity, dict):
            continue
        if "value" not in entity or "unit" not in entity:
            value, unit = _entity_value_unit(entity.get("text"),
                                             entity.get("entityType"))
            if value is not None and "value" not in entity:
                entity["value"] = value
            if unit is not None and "unit" not in entity:
                entity["unit"] = unit
        if "span" in entity:
            continue
        index = entity.get("lineIndex")
        text = str(entity.get("text") or "")
        if not text or not isinstance(index, int) or not 0 <= index < len(lines):
            continue
        at = str(lines[index].get("text") or "").find(text)
        if at >= 0:
            entity["span"] = [at, at + len(text)]
    return entities


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
    # The walk is sliced, never indexed: the runtime copy of this script
    # lives two or three levels deep in the sense runtime's own temp
    # directory, where the third or fourth parent does not exist, and
    # indexing one that is not there raised IndexError before the walk
    # could even start - which took the whole voice-signals read down
    # with it, on every run whose ears were simply absent. A shallow
    # tree offers fewer bases; the absent ears stay absent, as designed.
    for base in _rust_binary_search_bases(here):
        for profile in ("release", "debug"):
            built = base / "target" / profile / "voice-signals"
            if built.is_file():
                return str(built)
    return None


def _rust_binary_search_bases(here):
    """The two checkout roots beside the sense scripts, if they exist.

    Runtime copies can live in a shallow temporary directory. Slicing the
    parents sequence returns the available roots without indexing a parent
    that the runtime tree does not have.
    """
    return list(Path(here).parents)[2:4]


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
    # The VAD block, but only when the caller tuned the ffmpeg silence
    # pass: the two silence ears used to measure under different rules -
    # a 120 ms minSilenceMs caught a breath for pauses.classes while the
    # Rust ear's fixed 0.20 s merged it inside an utterance. The tuning
    # crosses now (thresholdDb as the negative dB silencedetect means,
    # minSilenceMs as seconds), so both ears answer with one rule; a
    # caller who tuned nothing leaves each ear its own defaults.
    _vad = {}
    if spec.get("thresholdDb") is not None:
        try:
            _vad["thresholdDb"] = -abs(float(spec["thresholdDb"]))
        except (TypeError, ValueError):
            pass
    if spec.get("minSilenceMs") is not None:
        try:
            _vad["minSilenceSecs"] = max(0.05, float(spec["minSilenceMs"]) / 1000.0)
        except (TypeError, ValueError):
            pass
    if _vad:
        rust_spec["vad"] = _vad
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
    want. Returns (words by ORIGINAL line index, note): only timed lines
    can be aligned, but the answer is keyed by the line's own index in
    the full list, so an untimed line before a timed one cannot shift
    every later line's words onto the wrong line."""
    try:
        import whisperx  # noqa: F401  the heavy import, guarded on purpose
    except Exception:
        return None, "whisperx is not installed (pip install whisperx); " \
                     "whisper token-timestamp words are used as-is"
    try:
        timed = [(i, l) for i, l in enumerate(lines)
                 if isinstance(l.get("start"), (int, float))]
        segments = [{"start": l.get("start"), "end": l.get("end"),
                     "text": str(l.get("text") or "")}
                    for _, l in timed]
        if not segments:
            return None, "no timed lines to align"
        device = os.environ.get("FUNCUT_VOICE_SIGNALS_DEVICE", "cpu")
        audio_data = whisperx.load_audio(audio)
        model, metadata = whisperx.load_align_model(
            language_code=language or "en", device=device)
        aligned = whisperx.align(segments, model, metadata, audio_data, device,
                                 return_char_alignments=False)
        by_line = {}
        for (original_index, _), seg in zip(timed, aligned.get("segments") or []):
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
            by_line[original_index] = words
        return by_line, None
    except Exception as exc:
        return None, f"whisperx alignment failed: {type(exc).__name__}: {exc}"


# ---------------------------------------------------------------------------
# The director hints: the wiring
# ---------------------------------------------------------------------------


def _motion_design(lines_out, pauses, motifs, entities, arc, speaker,
                   forensic_whole, whole_prosody=None):
    """The twenty motion-design readings, derived from the profile.

    They add no pass: each one turns a reading the profile already made
    into the motion choice it can justify. The shapes are deliberately
    small and timed - a UI draws them as a lane, and a director spends
    them as keyframes, entrances, holds and texture.
    """
    def number(value):
        return value if isinstance(value, (int, float)) else None

    def nearest_line(moment):
        best = None
        for line in lines_out:
            start, end = number(line.get("start")), number(line.get("end"))
            if start is None or end is None:
                continue
            if start <= moment <= end:
                return line.get("index")
            distance = min(abs(moment - start), abs(moment - end))
            if best is None or distance < best[0]:
                best = (distance, line.get("index"))
        return best[1] if best else None

    def next_text(moment):
        for line in lines_out:
            start = number(line.get("start"))
            if start is not None and start >= moment:
                return str(line.get("text") or "")
        return ""

    emphasis_hits = []
    kinetic_scale = []
    pitch_direction = []
    pitch_travel = []
    rate_tempo = []
    rhythm_stagger = []
    sentence_entry = []
    confidence_gate = []
    level_pulse = []
    entry_presets = {
        "question": "funcut.caption.kinetic",
        "exclamation": "funcut.sting.wordmark",
        "fragment": "funcut.quote.pull",
        "statement": "funcut.lower-third.minimal",
    }

    for line in lines_out:
        index = line.get("index")
        prosody = line.get("prosody") or {}
        rate = line.get("speechRate") or {}
        loudness = line.get("loudness") or {}
        sentence = (line.get("sentence") or {}).get("type") or "statement"

        emphasis = prosody.get("emphasisWords") or []
        peak = 0.0
        for word in emphasis:
            strength = number(word.get("strength")) or 0.5
            peak = max(peak, strength)
            at = number(word.get("t"))
            if at is None:
                at = number(line.get("start")) or 0.0
            emphasis_hits.append({
                "line": index,
                "at": round(at, 3),
                "word": word.get("wordText") or word.get("word"),
                "strength": round(strength, 3),
                "scale": round(1.0 + 0.25 * strength, 3),
            })

        pitch_range = number(prosody.get("pitchRangeSemitones"))
        scale = 1.0 + 0.18 * peak
        if pitch_range is not None:
            scale += min(max(pitch_range, 0.0), 12.0) / 12.0 * 0.12
        kinetic_scale.append({
            "line": index,
            "start": line.get("start"),
            "scale": round(min(scale, 1.35), 3),
        })

        slope = number(prosody.get("pitchSlopeSemitonesPerSec"))
        final_rise = prosody.get("finalRise")
        if final_rise is True or (slope is not None and slope > 0.4):
            direction = "rise"
        elif slope is not None and slope < -0.4:
            direction = "fall"
        else:
            direction = "flat"
        pitch_direction.append({
            "line": index,
            "direction": direction,
            "semitonesPerSec": round(slope, 3) if slope is not None else None,
            "finalRise": final_rise,
        })
        pitch_travel.append({
            "line": index,
            "semitonesPerSec": round(slope, 3) if slope is not None else None,
            "travelPx": round(max(-48.0, min(48.0, (slope or 0.0) * 8.0)), 1),
        })

        wpm = number(rate.get("wordsPerMinute"))
        rate_tempo.append({
            "line": index,
            "start": line.get("start"),
            "end": line.get("end"),
            "wpm": wpm,
            "speed": round(max(0.6, min(1.6, (wpm or 150.0) / 150.0)), 3),
        })

        # The rhythm layers are a WHOLE-TAKE reading: the Rust ear emits
        # them at prosody.rhythmLayers and never per line (LineProsody
        # carries no layers), so the stagger draws on the take's own
        # layers, handed in beside the forensic whole - the per-line
        # prosody keeps the fields that ARE per-line (emphasis, pitch,
        # rise) above.
        layers = (whole_prosody or {}).get("rhythmLayers") or []
        rhythm_stagger.append({
            "line": index,
            "layers": layers,
            "staggerSec": round(0.06 * len(layers), 3),
        })
        sentence_entry.append({
            "line": index,
            "start": line.get("start"),
            "end": line.get("end"),
            "type": sentence,
            "preset": entry_presets.get(sentence, entry_presets["statement"]),
        })

        for word in line.get("words") or []:
            confidence = number(word.get("confidence"))
            if confidence is not None and confidence < 0.5:
                confidence_gate.append({
                    "line": index,
                    "at": word.get("start"),
                    "word": word.get("text"),
                    "confidence": round(confidence, 3),
                })

        lift = number(loudness.get("relativeLu"))
        if lift is not None:
            level_pulse.append({
                "line": index,
                "relativeLu": round(lift, 2),
                "scale": round(1.0 + max(-0.08, min(0.12, lift * 0.02)), 3),
            })

    pause_dwell = []
    topic_chapter = []
    for pause in pauses:
        start = number(pause.get("start"))
        if start is None:
            continue
        duration = number(pause.get("durationSec")) or 0.0
        pause_dwell.append({
            "at": round(start, 3),
            "class": pause.get("class"),
            "holdSec": round(duration, 3),
            "dwellFrames": int(round(duration * 30)),
        })
        if pause.get("class") == "scene_turn":
            topic_chapter.append({
                "at": round(start, 3),
                "reason": "scene_turn",
                "headline": next_text(start),
            })

    onsets = (forensic_whole or {}).get("fluxOnsetsSec") or []
    onset_sync = [
        {
            "at": round(float(moment), 3),
            "line": nearest_line(float(moment)),
            "strength": 1.0,
        }
        for moment in onsets[:240]
        if isinstance(moment, (int, float))
    ]

    arc_turn = []
    emotion_ease = []
    if isinstance(arc, dict):
        valence = arc.get("valenceCurve") or []
        arousal = arc.get("arousalCurve") or []
        for index in range(1, min(len(valence), len(lines_out))):
            turn = (valence[index] * valence[index - 1] < 0
                    or abs(arousal[index] - arousal[index - 1]) >= 0.25)
            if turn:
                arc_turn.append({
                    "at": lines_out[index].get("start"),
                    "line": index,
                    "from": round(valence[index - 1], 3),
                    "to": round(valence[index], 3),
                    "shape": arc.get("arcShape"),
                })
        ease = {
            "rise": "spring",
            "fall": "ease-out",
            "wave": "emphasis",
            "flat": "linear",
        }.get(str(arc.get("arcShape") or ""), "emphasis")
        emotion_ease.append({
            "shape": arc.get("arcShape"),
            "ease": ease,
            "arcScore": arc.get("arcScore"),
            "valence": round(sum(valence) / len(valence), 3) if valence else None,
        })

    entity_card = [
        {
            "at": entity.get("at"),
            "line": nearest_line(entity.get("at")),
            "kind": entity.get("kind"),
            "card": entity.get("card"),
            "text": entity.get("text"),
            "value": entity.get("value"),
            "unit": entity.get("unit"),
        }
        for entity in entities
        if number(entity.get("at")) is not None
    ]

    motif_return = []
    for motif in motifs:
        for index in motif.get("lineIndices") or []:
            if not isinstance(index, int) or index <= 0 or index >= len(lines_out):
                continue
            motif_return.append({
                "at": lines_out[index].get("start"),
                "line": index,
                "phrase": motif.get("phrase"),
                "count": motif.get("count"),
            })

    speaker_turn = []
    for moment in (speaker or {}).get("changes") or []:
        if isinstance(moment, (int, float)):
            speaker_turn.append({
                "at": round(float(moment), 3),
                "line": nearest_line(float(moment)),
            })

    floor = (forensic_whole or {}).get("noiseFloorDbfs")
    flatness = (forensic_whole or {}).get("spectralFlatness")
    grain = 0.0
    if isinstance(floor, (int, float)):
        grain = max(0.0, min(1.0, (floor + 60.0) / 40.0))
    if isinstance(flatness, (int, float)):
        grain = max(grain, min(1.0, flatness * 2.0))
    noise_grain = [{
        "noiseFloorDbfs": floor,
        "spectralFlatness": flatness,
        "grain": round(grain, 3),
    }]

    sibilance = (forensic_whole or {}).get("sibilanceRatio")
    sibilance_texture = [{
        "ratio": sibilance,
        "shimmer": round(min(1.0, (sibilance or 0.0) * 2.0), 3),
    }]
    reverb = (forensic_whole or {}).get("reverbTailSec")
    reverb_space = [{
        "tailSec": reverb,
        "blurPx": round(min(24.0, (reverb or 0.0) * 40.0), 1),
    }]

    return {
        "emphasisHits": emphasis_hits,
        "kineticScale": kinetic_scale,
        "pitchDirection": pitch_direction,
        "pitchTravel": pitch_travel,
        "rateTempo": rate_tempo,
        "pauseDwell": pause_dwell,
        "onsetSync": onset_sync,
        "rhythmStagger": rhythm_stagger,
        "sentenceEntry": sentence_entry,
        "emotionEase": emotion_ease,
        "arcTurn": arc_turn,
        "entityCard": entity_card,
        "topicChapter": topic_chapter,
        "motifReturn": motif_return,
        "speakerTurn": speaker_turn,
        "confidenceGate": confidence_gate,
        "sibilanceTexture": sibilance_texture,
        "reverbSpace": reverb_space,
        "noiseGrain": noise_grain,
        "levelPulse": level_pulse,
    }


def _signal_confidence(sources, audio, rust, wx_words, series, notes,
                        script_alignment=None):
    """One confidence per signal family, with the basis a director can
    weigh: which ear answered, and what it was built on. A signal with a
    low confidence is not wrong - it is one the plan should lean on less,
    and the basis says why. Formants on a phone-band take, reverb from a
    single-channel envelope, emphasis without word timings: each is
    honest about what it is. The script alignment, when it ran, is part
    of that honesty: a word stream whose wording the author verified is
    a different footing than raw ASR output, and the wordConfidence row
    says so in its basis."""
    rust_up = bool(rust)
    band = None
    # The source's own rate, probed where the audio exists: the ear's
    # handoff WAV is always 16 kHz, so the ear's rate (and the forensic
    # block the old probe read, which carries none) can never answer
    # the band question.
    sample_rate = _source_rate_hz(audio if isinstance(audio, str) else None)
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

    # Word confidence: whisperx scores or the measured gate - and the
    # script alignment, when it held, is a footing worth naming: the
    # basis says the confirmed words are floored at 0.85, and a verdict
    # of "aligned" (most lines paired) lifts the row by a notch because
    # most of the stream is author-verified. A "partial" verdict names
    # the floor but earns no lift - half a verification is not one.
    verdict = (script_alignment or {}).get("verdict")
    script_basis = ("; script-confirmed words floored at 0.85"
                    if verdict in ("aligned", "partial") else "")
    script_lift = 0.05 if verdict == "aligned" else 0.0
    if str(sources.get("wordConfidence") or "").startswith("whisperx"):
        entry("wordConfidence", min(0.95, 0.9 + script_lift),
              "whisperx alignment scores" + script_basis)
    elif sources.get("wordConfidence"):
        entry("wordConfidence", min(0.95, 0.6 + script_lift),
              "measured gate (timing, silence overlap, energy) - a screen, "
              "not a score" + script_basis)
    else:
        entry("wordConfidence", min(0.95, 0.3 + script_lift),
              "no audio: word confidence is absent" + script_basis)

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
        # The motion family is derived, not measured: its confidence is
        # the floor of the readings it spends, and its basis names them.
        entry("motion", (0.8 if (sources.get("prosody") or sources.get("pauses"))
                         else 0.5),
              "derived from the profile's line, pause, prosody, forensic, "
              "speaker, entity, motif and arc readings; no extra pass")
    else:
        for name in ("prosody", "forensic", "speaker", "entities", "formants"):
            basis = next(
                (n for n in notes if "filtered to" in n
                 or "voice-signals binary" in n), None)
            entry(name, 0.0, basis or "the Rust ear is not built: "
                                     "cargo build -p voice-signals")
        entry("motion", 0.4, "derived from text, pause and rate readings "
                             "only; the Rust ears did not answer")

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
    # The arc object is emotion.py's reply passed through verbatim, and
    # its shape key is `arcShape` - reading `shape` never matched and the
    # building cut style was unreachable for every real read.
    if arc and str(arc.get("arcShape") or "") == "rise" and cut_style == "mixed":
        cut_style = "building"
    gated = [{"lineIndex": l["index"], "word": w["text"], "confidence": w["confidence"]}
             for l in lines_out for w in l.get("words") or []
             if w.get("confidence") is not None and w["confidence"] < 0.5]
    speaker_changes = []
    if rust and rust.get("speaker", {}).get("changes"):
        speaker_changes = rust["speaker"]["changes"]
    questions = [l["index"] for l in lines_out
                 if l.get("sentence", {}).get("type") == "question"]
    emphasis = [{"lineIndex": l["index"], "words": [
                    w.get("wordText") or w.get("word")
                    for w in (l.get("prosody") or {}).get("emphasisWords") or []
                    if w.get("wordText") or w.get("word")]}
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


def _bgm_decision(rust, speech_ratio, duration=None):
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
        # The rate, never the raw count: a count bar is a length claim,
        # not a density one - a 30-minute narration clears 60 raw onsets
        # in dead speech. onsetRatePerMin >= 60 is one onset per second
        # sustained, the same bar the director reads. An older ear that
        # did not serialize the rate is answered honestly by deriving it
        # from the count over the take's duration; with neither a rate
        # nor a duration the gate stays silent rather than guess.
        onset_rate = whole.get("onsetRatePerMin")
        if not isinstance(onset_rate, (int, float)):
            onset_rate = onsets / duration * 60.0 if duration else None
        if flatness is not None and flatness > 0.25 and \
                isinstance(onset_rate, (int, float)) and onset_rate >= 60.0:
            return {"recommendation": "skip",
                    "reason": f"spectral flatness {flatness:.2f} with an onset "
                              f"rate of {onset_rate:.0f}/min: "
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
    # The script↔transcript alignment: when the caller measured a take AND
    # wrote what it was meant to say, the two are reconciled here - before
    # any ear reads the lines, so every later pass (the language scan,
    # whisperx, the Rust NER, the regex complement) sees the corrected
    # wording and never the mishearing. It fires only when BOTH exist: a
    # script alone still becomes the lines exactly as before
    # (`_collect_lines` owns that path untouched), and a transcript alone
    # is never second-guessed - there is nothing to correct it against.
    script_alignment = None
    script_raw = _script_text(spec)
    if script_raw and any(isinstance(l.get("start"), (int, float)) for l in lines):
        script_alignment = _align_script(lines, script_raw)
    # The `signals` filter: a manual extraction of named readings, not
    # the whole profile. Only the passes the asked-for signals need run -
    # a filler read costs no ffmpeg pass and no Rust ear - and the reply
    # carries a `requested` block with each reading, its source and its
    # confidence, beside the (honestly degraded) profile.
    wanted = _wanted_signals(spec)
    if isinstance(wanted, dict):
        return {"ok": False, "error": wanted["error"]}
    # The motion-design readings are derived from the rest of the profile,
    # so asking for one owes every pass it derives from.
    need_motion = wanted is None or any(
        name.startswith("motion.") for name in wanted)
    need_pauses = need_motion or wanted is None or any(
        name in ("pauses.classes", "pauses.speechRatio", "words.confidence")
        for name in wanted)
    # The measured word gate reads the momentary series, so a
    # words.confidence ask owes the loudness pass too - WhisperX's own
    # scores replace the gate only when the alignment actually ran.
    need_loudness = need_motion or wanted is None or any(
        name.startswith("loudness.") for name in wanted) \
        or "words.confidence" in wanted
    need_rust = need_motion or wanted is None or any(
        CATALOGUE_BY_NAME[name]["rust"] for name in wanted)
    need_whisperx = need_motion or wanted is None or "words.confidence" in wanted
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
        "motion": None,
        "alignment": "whisper token timestamps",
    }
    # The alignment source, named when the script pass ran: the word
    # stream the profile carries is now a hybrid (measured times,
    # authored wording), and `sources` is where the profile says what
    # every reading stands on.
    if script_alignment is not None:
        sources["alignment"] = ("script↔transcript word alignment (difflib); "
                                "script wording is authoritative")

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
            # WhisperX would otherwise claim the alignment source alone;
            # when the script pass also ran, the two are composed - the
            # times are whisperx's, the wording the script's - because a
            # source line that names one and silently drops the other is
            # the same lie an absent ear tells.
            sources["alignment"] = (
                "whisperx aligned words + script confirmation "
                "(script wording authoritative)"
                if script_alignment is not None
                else "whisperx aligned words with scores")
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
        if wx_words and wx_words.get(i):
            words = wx_words[i]
        confidences = []
        if words:
            if wx_words:
                confidences = [dict(w, confidence=round(w.get("score") or 0.0, 3))
                               for w in words]
            else:
                confidences = _word_confidences(words, silence_spans, series)
            words = confidences
        # The script confirmation: the alignment marked which words the
        # script verified (equal blocks) and which it corrected (1:1
        # replaces). Both are script-verified, so both floor at 0.85 - a
        # word the author wrote is not one the caption gate should drop
        # - and the flag says which is which, confirmed or corrected. The
        # indices address the line's own words; when whisperx replaced
        # them the mapping is best-effort (whisperx re-aligned the
        # corrected text, so the two word streams agree in order even
        # where their counts drift), and an index past either list's end
        # simply never fires.
        script_meta = line.get("_script")
        if script_meta and words:
            confirmed = set(script_meta.get("confirmed") or [])
            corrected = set(script_meta.get("corrected") or [])
            for position, word in enumerate(words):
                if position in corrected:
                    word["confidence"] = round(max(word.get("confidence") or 0.0, 0.85), 3)
                    word["corrected"] = True
                elif position in confirmed:
                    word["confidence"] = round(max(word.get("confidence") or 0.0, 0.85), 3)
                    word["scriptConfirmed"] = True
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
                 "syllables": _syllables(w["text"]),
                 # The script flags only when true: a transcript-only
                 # caller's words stay byte-for-byte the shape they
                 # always were, and a script-confirmed word says so.
                 **({"scriptConfirmed": True} if w.get("scriptConfirmed") else {}),
                 **({"corrected": True} if w.get("corrected") else {})}
                for w in words
            ],
        })

    # Strip the alignment's private bookkeeping. The `_script` key is the
    # alignment's scratch space between `_align_script` and the assembly
    # above; the `lines` list is reused below (the motifs and the entity
    # complement read it, both of which want the corrected `text` and
    # the raw `words`), and nothing private may leak into a payload or
    # surprise a later pass.
    for line in lines:
        line.pop("_script", None)

    # Entities: anno (Rust) plus the regex complement.
    entities = list((rust or {}).get("entities") or [])
    if entities:
        for entity in entities:
            entity["source"] = "anno"
    entities = _merge_entities(entities, _entity_complement(lines))
    entities = _enrich_entities(entities, lines)
    # The complement ran on the text whenever there were lines, so the
    # entities reading is sourced even when the Rust ear never was: a
    # filtered entities ask answers from the complement and is not a
    # degradation. But the source note is not availability: the NER
    # ear's own note is (DEG-01), and the complement never counts -
    # entitiesAvailable stays False until anno itself answered.
    if lines and not sources.get("entities"):
        sources["entities"] = "regex complement (python)"

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
    bgm = _bgm_decision(rust, speech_ratio, duration)
    hints = _director_hints(lines_out, pauses, rust, language, arc, bgm)
    motifs_out = _motifs(lines)
    motion = _motion_design(
        lines_out, pauses, motifs_out, entities, arc,
        (rust or {}).get("speaker"),
        (rust or {}).get("forensic", {}).get("whole"),
        (rust or {}).get("prosody"))
    sources["motion"] = ("derived from the profile's own readings "
                         "(no extra pass)")
    # The filter's own note lands before the confidence builder reads the
    # notes, so an ear the ask never needed explains itself as skipped by
    # request rather than as broken.
    if wanted is not None:
        notes.append(
            f"filtered to {len(wanted)} signal(s): the passes the ask does "
            "not need did not run")
    confidence = _signal_confidence(sources, audio, rust, wx_words, series,
                                    notes, script_alignment)

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
    # fallback rather than trust the reading as measured. The ear's own
    # sources note is the answer; the complement's note (set above for
    # the reading's honesty) never counts.
    entities_available = bool((rust or {}).get("sources", {}).get("entities"))
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
        "motifs": motifs_out,
        "motion": motion,
        "entities": entities,
        "entitiesAvailable": entities_available,
        "arc": arc,
        # The alignment block, only when the caller gave both a transcript
        # and a script: omitted otherwise, so a transcript-only caller's
        # payload keeps exactly the shape it has always carried.
        **({"scriptAlignment": script_alignment}
           if script_alignment is not None else {}),
        "speaker": (rust or {}).get("speaker"),
        "wholeTake": {
            "loudness": {"integratedLufs": integrated,
                         "loudnessRangeLu": lra,
                         "truePeakDbfs": true_peak,
                         # The curve the per-line fold flattens away:
                         # the bed's volume ride keys itself on this.
                         "momentaryLufs": _decimated_momentary(series)},
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
            # The intended narration, carried beside the transcript so the
            # alignment pass has both: it matches the transcript word for
            # word (the fourth sentence keeps the transcript's lowercase
            # "it" - a capital would flip the 0.9 s hold before it from a
            # dramatic mid-sentence hold to a scene turn and break the
            # pause-class assertions below), so the honest reading of the
            # exact-match case is every word confirmed, none corrected.
            "script": "Um, so this is the first line? This is the first "
                      "line again. The third line lands here. "
                      "it earned $10,450 in 2026.",
            "language": "en",
            "whisperx": "off",
        }
        # Keep the selftest honest about the Rust ears: they run if the
        # binary is built, and their absence is a note, not a failure.
        result = extract(spec)
        failures = []
        # A runtime copy can be only one directory below its temporary
        # root. The checkout-root walk must then be empty, not an IndexError
        # that aborts the entire voice-signals stage when its Rust binary is
        # absent.
        shallow_runtime = Path("/runtime")
        if _rust_binary_search_bases(shallow_runtime):
            failures.append("a shallow runtime tree exposed nonexistent checkout roots")
        if not result.get("ok"):
            return {"ok": False, "selftest": "extract failed", "error": result.get("error")}
        lines = result["lines"]
        # The motion family adds no pass, so its shape is provable without
        # ffmpeg: every one of the twenty readings must be present, and the
        # per-line derivations must answer for every line.
        motion = result.get("motion") or {}
        for name in ("emphasisHits", "kineticScale", "pitchDirection",
                     "pitchTravel", "rateTempo", "pauseDwell", "onsetSync",
                     "rhythmStagger", "sentenceEntry", "emotionEase", "arcTurn",
                     "entityCard", "topicChapter", "motifReturn", "speakerTurn",
                     "confidenceGate", "sibilanceTexture", "reverbSpace",
                     "noiseGrain", "levelPulse"):
            if name not in motion:
                failures.append(f"motion.{name} missing from the profile")
        for name in ("kineticScale", "pitchDirection", "pitchTravel",
                     "rateTempo", "rhythmStagger", "sentenceEntry"):
            if len(motion.get(name) or []) != len(lines):
                failures.append(f"motion.{name} did not answer every line")
        # rhythmStagger draws on the WHOLE-TAKE prosody: the Rust ear
        # emits the rhythm layers at prosody.rhythmLayers and never per
        # line, so the reading is proven by handing _motion_design a
        # fully-populated whole-take prosody directly - two differing
        # layers must yield non-empty layers and a non-zero stagger for
        # every line, whatever the per-line prosody carries.
        two_layers = [
            {"eigenvalue": 0.12, "periodSec": 2.4, "strength": 0.8},
            {"eigenvalue": 0.41, "periodSec": 1.1, "strength": 0.4},
        ]
        staged = _motion_design(
            lines, result["pauses"], result["motifs"], result["entities"],
            result.get("arc"), result.get("speaker"),
            (result.get("wholeTake") or {}).get("forensic"),
            {"rhythmLayers": two_layers})
        staggered = staged.get("rhythmStagger") or []
        if len(staggered) != len(lines):
            failures.append("rhythmStagger did not answer every line on the whole-take read")
        elif not all(entry.get("layers") == two_layers for entry in staggered):
            failures.append(f"the whole-take layers did not reach the stagger: "
                            f"{staggered[:1]}")
        elif not all((entry.get("staggerSec") or 0.0) > 0.0 for entry in staggered):
            failures.append("differing layers staggered the entries by nothing")
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
        # The script↔transcript alignment: the spec carries both, and the
        # script matches the transcript word for word, so the honest
        # reading is every word confirmed, none corrected, verdict
        # aligned - and every output word floored at 0.85 carrying the
        # flag that says why.
        alignment = result.get("scriptAlignment") or {}
        if alignment.get("verdict") != "aligned":
            failures.append(
                f"script alignment verdict {alignment.get('verdict')!r}, not 'aligned'")
        if alignment.get("correctedWords") != 0:
            failures.append(
                f"an exact script corrected {alignment.get('correctedWords')} words")
        if alignment.get("confirmedWords") != 23:
            failures.append(
                f"confirmed {alignment.get('confirmedWords')} words, not all 23")
        if alignment.get("matched") != 4 or alignment.get("unmatched") != 0:
            failures.append(f"the pairing counts drifted: {alignment}")
        if not all(w.get("scriptConfirmed") and (w.get("confidence") or 0) >= 0.85
                   for line in lines for w in line["words"]):
            failures.append("a script-confirmed word lost its flag or its 0.85 floor")
        if any(w.get("corrected") for line in lines for w in line["words"]):
            failures.append("a word was marked corrected against an exact script")
        if "script" not in str(result.get("sources", {}).get("alignment") or ""):
            failures.append(f"the alignment source was not named: "
                            f"{result.get('sources', {}).get('alignment')}")
        # The mishearing correction, on a small direct case: the ASR heard
        # "maket", the script wrote "market" - the word's text is
        # corrected, its measured times are kept, the line takes the
        # script's sentence, and the corrected index is recorded for the
        # assembly to floor.
        fake = [{"text": "the maket crashed today", "start": 0.0, "end": 1.5,
                 "words": [{"text": "the", "start": 0.0, "end": 0.3},
                           {"text": "maket", "start": 0.3, "end": 0.9},
                           {"text": "crashed", "start": 0.9, "end": 1.2},
                           {"text": "today", "start": 1.2, "end": 1.5}]}]
        fixed = _align_script(fake, "the market crashed today.")
        if fixed.get("correctedWords") != 1 or fixed.get("verdict") != "aligned":
            failures.append(f"the mishearing case did not correct: {fixed}")
        elif fake[0]["words"][1]["text"] != "market":
            failures.append(
                f"the misheard word reads {fake[0]['words'][1]['text']!r}, not 'market'")
        elif fake[0]["words"][1].get("start") != 0.3 or \
                fake[0]["words"][1].get("end") != 0.9:
            failures.append("the correction moved the word's measured times")
        elif fake[0]["text"] != "the market crashed today.":
            failures.append(f"the line text reads {fake[0]['text']!r}")
        elif fake[0].get("_script", {}).get("corrected") != [1]:
            failures.append(
                f"the corrected index was not recorded: {fake[0].get('_script')}")
        # The value/unit reader: the shapes a stat card formats, currencies
        # by symbol, the South-Asian multipliers, and the year rule.
        for text, kind, want in (("$10,450", "MONEY", (10450, "USD")),
                                 ("£2,000", "MONEY", (2000, "GBP")),
                                 ("€3.5 million", "MONEY", (3500000, "EUR")),
                                 ("৳500", "MONEY", (500, "BDT")),
                                 ("₹1,000", "MONEY", (1000, "INR")),
                                 ("¥1000", "MONEY", (1000, "JPY")),
                                 ("10,450 dollars", "MONEY", (10450, "dollars")),
                                 ("10 lakh", "MONEY", (1000000, "lakh")),
                                 ("45%", "PERCENT", (45, "percent")),
                                 ("12.5 %", "PERCENT", (12.5, "percent")),
                                 ("2026", "DATE", (2026, "year")),
                                 ("1972", "CARDINAL", (1972, "year")),
                                 ("three", "CARDINAL", (None, None))):
            got = _entity_value_unit(text, kind)
            if got != want:
                failures.append(
                    f"_entity_value_unit({text!r}, {kind!r}) = {got}, not {want}")
        # The enrichment: the money entity carries its value, its unit and
        # its character span in the line ("it earned $10,450 in 2026." -
        # the money sits at chars 10..17), whichever ear found it.
        enriched = [e for e in result["entities"] if e.get("text") == "$10,450"]
        if not enriched:
            failures.append("no $10,450 entity to check the enrichment against")
        else:
            if enriched[0].get("value") != 10450 or enriched[0].get("unit") != "USD":
                failures.append(f"the money entity was not enriched: {enriched[0]}")
            if enriched[0].get("span") != [10, 17]:
                failures.append(
                    f"the money entity's span is {enriched[0].get('span')}, not [10, 17]")
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
        # The bgm gate reads the RATE, never the raw count: a 30-minute dry
        # narration carries 360 onsets (one every 5 s) - a count bar of 60
        # would strip its music bed - while its rate is 12/min, far below
        # the one-onset-a-second bar the director reads. A dense take (a
        # click every 0.5 s, 120/min) skips. Both proven directly, so the
        # gate answers without the Rust ears on this machine.
        dry_take = {"forensic": {"whole": {
            "noiseFloorDbfs": -60.0, "snrDb": 30.0,
            "spectralFlatness": 0.4, "musicLikelihood": 0.1,
            "onsetRatePerMin": 12.0,
            "fluxOnsetsSec": [i * 5.0 for i in range(360)],
        }}}
        long_dry = _bgm_decision(dry_take, 0.9, 1800.0)
        if long_dry["recommendation"] != "add":
            failures.append(
                f"a 30-minute dry narration said {long_dry['recommendation']}, not add: "
                f"{long_dry}")
        # The same take without the rate key (an older ear) is answered by
        # the derived rate - count over duration - and still beds.
        older_ear = {"forensic": {"whole": {
            k: v for k, v in dry_take["forensic"]["whole"].items()
            if k != "onsetRatePerMin"}}}
        derived = _bgm_decision(older_ear, 0.9, 1800.0)
        if derived["recommendation"] != "add":
            failures.append(
                f"the derived-rate fallback said {derived['recommendation']}, not add: "
                f"{derived}")
        dense_take = {"forensic": {"whole": {
            "noiseFloorDbfs": -60.0, "snrDb": 30.0,
            "spectralFlatness": 0.4, "musicLikelihood": 0.1,
            "onsetRatePerMin": 120.0,
            "fluxOnsetsSec": [i * 0.5 for i in range(120)],
        }}}
        dense = _bgm_decision(dense_take, 0.9, 60.0)
        if dense["recommendation"] != "skip":
            failures.append(
                f"a 120-onsets-a-minute take said {dense['recommendation']}, not skip: "
                f"{dense}")
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
        for decision in ("cut", "caption", "graphic", "bgm", "transition",
                         "motion"):
            entry = policy.get(decision)
            if not entry or not entry.get("signals") or not entry.get("rule"):
                failures.append(f"no decision policy for {decision}: {entry}")
        # The catalogue: seventy-one named readings, thirty-four of them
        # the two catalogue passes added, every name unique, every family
        # named, every entry held to a why and a where.
        catalog = _catalog_out()
        if catalog["count"] != 71:
            failures.append(f"catalogue carries {catalog['count']} signals, not 71")
        if catalog["new"] != 34:
            failures.append(f"catalogue marks {catalog['new']} new signals, not 34")
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
                "scriptAlignment": f"{alignment.get('verdict')}: "
                                   f"{alignment.get('confirmedWords')} words confirmed, "
                                   f"{alignment.get('correctedWords')} corrected",
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
