// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The `voice_signals` profile, read as one typed document.
//!
//! AUTO EDIT used to reach into the reply ad hoc - a `pointer` for a pace
//! here, a `pauses` scan there - so every stage that wanted a new signal
//! grew another walk over raw JSON, and the signals nobody happened to
//! walk stayed unread: entities, motifs, speaker changes, the noise floor,
//! the word-confidence gate, the fillers, the director hints. The profile
//! measured roughly three dozen signals and the button spent eight of
//! them.
//!
//! This module is the one place the reply is turned into decisions'
//! inputs. Every field is an `Option` (or an empty vector) because the
//! profile is honest about what it could not measure, and the run has to
//! be too: "no pace measured" and "measured slow" are different facts, and
//! a stage that cannot tell them apart invents evidence. Each signal also
//! carries the `signalConfidence` weight the profile gave it, so a stage
//! can lean less on a phone-band formant read than on a whisperx-aligned
//! one instead of treating every reading as equal.
//!
//! The reader is deliberately permissive about shape - the Rust ear and
//! the Python pass name the same things slightly differently, and replies
//! move - and strict about meaning: a missing field stays missing.

use std::collections::BTreeMap;

use serde_json::Value;

/// One line of the take's delivery, as the profile measured it.
#[derive(Clone, Debug, Default)]
pub struct LineRead {
    /// Where the line begins in the take, in file seconds.
    pub start: f64,
    /// Where it ends, in file seconds.
    pub end: f64,
    /// The line's own words.
    pub text: String,
    /// The rate class: punchy, conversational, editorial or slow.
    pub pace: String,
    /// Words per minute, when the timings supported one.
    pub words_per_minute: Option<f64>,
    /// Syllables per second: the rate a caption's hold is sized on, which
    /// words-per-minute cannot answer for a line of long words.
    pub syllables_per_sec: Option<f64>,
    /// The pitch's slope across the line, in semitones per second: rising
    /// delivery builds, falling delivery settles.
    pub pitch_slope: Option<f64>,
    /// The line's pitch range, in semitones: a wide range is a line that
    /// performs, a narrow one is a line that reports.
    pub pitch_range: Option<f64>,
    /// The line's mean momentary loudness, in LUFS.
    pub mean_lufs_m: Option<f64>,
    /// The line's level against the take's integrated level, in LU.
    pub relative_lu: Option<f64>,
    /// The line's mean voiced pitch, in Hz: the register the line sits in.
    /// The take-wide `mean_f0_hz` is its reference, and the distance
    /// between the two is the lift the delivery put under the line.
    pub pitch_mean_hz: Option<f64>,
    /// Whether the pitch's terminal rise was measured, acoustically, by
    /// the prosody ear - the confirmation the punctuation's own reading
    /// (`rise_confirms`) cross-checks rather than replaces.
    pub final_rise: Option<bool>,
    /// The share of the line's frames the pitch tracker found a voice in.
    /// A line with barely any voice under it is a line the transcript
    /// probably misaligned, and its caption belongs behind the gate.
    pub voiced_ratio: Option<f64>,
    /// The line's harmonics-to-noise ratio, in dB, from the forensic ear.
    pub hnr_db: Option<f64>,
    /// The line's pitch-period variation (0-1): a hoarse or unstable read.
    pub jitter: Option<f64>,
    /// The line's amplitude variation (0-1).
    pub shimmer: Option<f64>,
    /// question, exclamation, statement or fragment.
    pub sentence_type: String,
    /// Whether the pitch's terminal rise confirmed (or contradicted) the
    /// punctuation's reading of the sentence.
    pub rise_confirms: Option<bool>,
    /// The words the delivery stressed, with their strength, timed.
    pub emphasis: Vec<Emphasis>,
    /// The words the confidence gate is unsure of, timed.
    pub unsure_words: Vec<UnsureWord>,
    /// The filler words the line carries, timed, with whether the profile
    /// calls them a safe removal or a context call.
    pub fillers: Vec<Filler>,
    /// The speaker holding the line, when diarization ran.
    pub speaker: Option<u64>,
}

/// A stressed word, as the prosody ear heard it.
#[derive(Clone, Debug)]
pub struct Emphasis {
    /// The word itself.
    pub word: String,
    /// Where it lands in the take, in file seconds.
    pub at: f64,
    /// How hard the energy peaked on it.
    pub strength: f64,
}

/// A word the confidence gate is unsure of: below the 0.5 the profile
/// calls a caption gate.
#[derive(Clone, Debug)]
pub struct UnsureWord {
    /// The word itself.
    pub word: String,
    /// The measured confidence, 0..1.
    pub confidence: f64,
    /// Where it lands in the take, in file seconds.
    pub at: f64,
}

/// A filler word: `safe-remove` for um and uh, `context-call` for the
/// discourse markers a director decides about rather than deletes.
#[derive(Clone, Debug)]
pub struct Filler {
    /// The word itself.
    pub word: String,
    /// safe-remove or context-call.
    pub kind: String,
    /// Where it lands in the take, in file seconds.
    pub at: f64,
}

/// One classified silence.
#[derive(Clone, Debug)]
pub struct Pause {
    /// Where the silence opens, in file seconds.
    pub start: f64,
    /// Where it closes, in file seconds.
    pub end: f64,
    /// breath, beat, dramatic, scene_turn or dead_air.
    pub class: String,
    /// Whether the profile says to keep it.
    pub keep: bool,
}

/// An entity the line names, with the artefact its kind asks for.
#[derive(Clone, Debug)]
pub struct Entity {
    /// The take the words were said in.
    pub take: String,
    /// MONEY, PER, ORG, DATE, PERCENT, PII, ...
    pub kind: String,
    /// The words the line says.
    pub text: String,
    /// The artefact: stat, lower-third, timeline, redact.
    pub card: String,
    /// Where it lands in the take, when the NER could time it.
    pub at: Option<f64>,
    /// Whether the reading is the NER's own or the regex complement's
    /// fallback. An approximate entity still earns a lower-third; it does
    /// not earn a stat or a timeline, which claim exactness.
    pub approximate: bool,
    /// The producer's own confidence in the reading, 0..1: the NER ear's
    /// per-entity score, the complement's flat 0.75. `None` when the
    /// producer wrote none - a foreign reply's card spends as it did.
    pub confidence: Option<f64>,
    /// The parsed magnitude, when the entity is a number the edit can
    /// chart: `$10,450` is 10450, `45%` is 45. The Python ear's own
    /// parsing, passed through - `None` when the words name no number,
    /// which is the honest answer for a name or a place.
    pub value: Option<f64>,
    /// What the value is counted in: USD, percent, INR, a year. `None`
    /// beside a `Some` value means the unit was not parsed, not that the
    /// value is unitless.
    pub unit: Option<String>,
}

/// A phrase the narration repeats: the visual or sound that should return
/// with it.
#[derive(Clone, Debug)]
pub struct Motif {
    /// The repeated phrase.
    pub phrase: String,
    /// How many distinct lines carry it.
    pub count: usize,
    /// The lines that carry it, in the transcript's order: the first is
    /// where the visual is established, the rest are where it returns.
    pub line_indices: Vec<usize>,
}

/// The take-wide readings: the facts that are about the whole file rather
/// than one line, and the decisions the profile itself derives from them.
#[derive(Clone, Debug, Default)]
pub struct TakeRead {
    /// The integrated level, in LUFS.
    pub integrated_lufs: Option<f64>,
    /// The noise floor, in dBFS. A dry voice sits under -45.
    pub noise_floor_dbfs: Option<f64>,
    /// Signal-to-noise ratio, in dB.
    pub snr_db: Option<f64>,
    /// The kind of noise the floor is made of: hiss, hum, rumble, ...
    /// The one forensic reading that says what a denoise should target.
    pub noise_type: String,
    /// Spectral flatness, 0..1: near 1 is hiss-like, near 0 is tonal.
    pub spectral_flatness: Option<f64>,
    /// The spectral centroid, in Hz: where the take's energy sits. A
    /// low centroid is a dull take, a high one a bright or hissy one.
    pub spectral_centroid_hz: Option<f64>,
    /// The times the spectral flux peaked, in file seconds: the
    /// transients a cut can land on, and the onsets the profile's own
    /// music rule counts.
    pub flux_onsets: Vec<f64>,
    /// Crest factor: peak over RMS. A high crest is a dynamic take.
    pub crest: Option<f64>,
    /// The take's dynamic range, in dB.
    pub dynamic_range_db: Option<f64>,
    /// The share of the take that is voiced, 0..1.
    pub voicing_rate: Option<f64>,
    /// The share of the take that is speech, 0..1.
    pub speech_ratio: Option<f64>,
    /// The profile's own cut style: fast, mixed, hold or building.
    pub cut_style: String,
    /// The rule the cut style carries, in the profile's words.
    pub cut_rule: String,
    /// The profile's font hint for the detected script.
    pub font: String,
    /// The profile's music-genre hint for the detected script.
    pub music_genre: String,
    /// The BGM decision the profile itself reached, and why.
    pub bgm_decision: Option<String>,
    /// The BGM reason, in the profile's words.
    pub bgm_reason: String,
    /// The speaker-change times diarization measured, in file seconds.
    pub speaker_changes: Vec<f64>,
    /// The turns diarization measured: who spoke over which span. A turn
    /// is the whole exchange, where a change is only its seam.
    pub speaker_turns: Vec<SpeakerTurn>,
    /// The lines whose punctuation asks a question, by index.
    pub questions: Vec<usize>,
    /// The take's tempo: the strongest rhythm layer's period, in seconds.
    /// The cut stage's own grid - a cut lands on a multiple of this.
    pub dominant_period_sec: Option<f64>,
    /// The rhythm layers the prosody read found, strongest first: the
    /// macro period the phrases sit on and the micro period inside them.
    pub rhythm_layers: Vec<RhythmLayer>,
    /// The phrase boundaries the prosody graph cut the take into: the
    /// narration's own breath-groups, each a natural seam for a visual
    /// change. `start`/`end` in file seconds, `label` the segmenter's own.
    pub phrases: Vec<PhraseSpan>,
    /// The take's RMS envelope, at most 256 points: the energy curve the
    /// drift and the pacing can read locally rather than only in summary.
    pub rms_envelope: Vec<f64>,
    /// The take's pitch contour, at most 256 points, `0.0` where the
    /// tracker found no voice: the curve the summary statistics flatten
    /// away - a rise the mean cannot show, a break the range cannot
    /// place. The hook's own lift is read from it below.
    pub pitch_contour_hz: Vec<f64>,
    /// The mean spectral flux: how fast the take's timbre moves, the
    /// honest input to a fast-or-held cut style when the profile derived
    /// none.
    pub flux_mean: Option<f64>,
    /// The take's mean F0 over voiced frames, in Hz: the register every
    /// line's own mean pitch is measured against.
    pub mean_f0_hz: Option<f64>,
    /// The loudest single sample, in dBFS: the ceiling a gain stage must
    /// respect.
    pub sample_peak_dbfs: Option<f64>,
    /// The share of samples at or over 99.9% of full scale: a take that
    /// already clips must not be raised, whatever the platform target
    /// asks for.
    pub clipping_ratio: Option<f64>,
    /// The share of the take's energy in the 4-8 kHz sibilance band: the
    /// harshness reading. A sibilant voice over a bed wants the bed
    /// further down, out of the band the voice's own clarity lives in.
    pub sibilance_ratio: Option<f64>,
    /// The mains frequency the floor hums at, when a Goertzel pass found
    /// one: the repair shopping list's most specific item.
    pub hum_hz: Option<f64>,
    /// The take's DC offset, 0-1 of full scale: a biased take wants a DC
    /// blocker before any other repair, and a denoise alone would leave
    /// the bias it should have removed.
    pub dc_offset: Option<f64>,
    /// The frequency under which 85% of the take's energy sits, in Hz: a
    /// low rolloff is a dark, muffled take whose clarity a presence lift
    /// restores - a reading the repair shopping list spends.
    pub spectral_rolloff_hz: Option<f64>,
    /// The take's spectral bandwidth, in Hz: how wide a band the voice
    /// occupies. A narrow, low band is the muffled reading's own second
    /// arm - the voice that never reached the presence band at all.
    pub spectral_bandwidth_hz: Option<f64>,
    /// The spectral-flux onset rate, in onsets per minute.
    pub onset_rate_per_min: Option<f64>,
    /// The forensic ear's own 0-1 reading of how music-like the take is:
    /// the honest second opinion the BGM decision cross-checks.
    pub music_likelihood: Option<f64>,
    /// The estimated reverb tail, in seconds. A reverberant voice smears
    /// over whatever sits under it, so the duck deepens with the tail.
    pub reverb_tail_sec: Option<f64>,
    /// The take's loudness range, in LU. A compressed take (a small range)
    /// amplifies whatever was squashed into it, so the gain stage caps
    /// what it will add.
    pub loudness_range_lu: Option<f64>,
    /// The take's true peak, in dBTP, when the loudness pass measured one.
    pub true_peak_dbfs: Option<f64>,
    /// How many utterances the VAD found: the count of pause-separated
    /// speech spans.
    pub utterances: Option<u64>,
    /// The mean utterance length, in seconds.
    pub mean_utterance_sec: Option<f64>,
    /// The longest silence between two utterances, in seconds.
    pub longest_silence_sec: Option<f64>,
    /// How much of the take each speaker holds, in seconds, by id: the
    /// share that says whether the take is one voice or a conversation.
    pub speaker_seconds: Vec<SpeakerShare>,
}

/// One speaker turn: who held the take over which span.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeakerTurn {
    /// Where the turn begins, in file seconds.
    pub start: f64,
    /// Where it ends, in file seconds.
    pub end: f64,
    /// The speaker id diarization gave it.
    pub speaker: u64,
}

/// One rhythm layer: the period the take's energy repeats on, and how
/// strong that repetition is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RhythmLayer {
    /// The layer's period in seconds; the first is the macro rhythm.
    pub period_sec: f64,
    /// The layer's relative strength, 0..1.
    pub strength: f64,
}

/// One phrase boundary the prosody graph cut the take into.
#[derive(Clone, Debug, PartialEq)]
pub struct PhraseSpan {
    /// Where the phrase begins, in file seconds.
    pub start: f64,
    /// Where it ends, in file seconds.
    pub end: f64,
    /// The segmenter's own label.
    pub label: String,
}

/// One speaker's share of the take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeakerShare {
    /// The speaker's id.
    pub id: u64,
    /// How much of the take this speaker holds, in seconds.
    pub speech_seconds: f64,
}

/// The whole profile for one take, with the confidence each family of
/// signals carries.
#[derive(Clone, Debug, Default)]
pub struct VoiceRead {
    /// The take file the reading is of.
    pub take: String,
    /// The take's length, in seconds.
    pub duration: Option<f64>,
    /// The lines, in play order.
    pub lines: Vec<LineRead>,
    /// Every silence the profile classified, not only the dead air.
    pub pauses: Vec<Pause>,
    /// The entities the narration names.
    pub entities: Vec<Entity>,
    /// The phrases the narration repeats.
    pub motifs: Vec<Motif>,
    /// The take-wide readings.
    pub take_wide: TakeRead,
    /// Confidence per signal family, keyed by the profile's own names.
    pub confidence: BTreeMap<String, f64>,
    /// What the profile said it could not measure, in its own words.
    pub degradations: Vec<String>,
}

impl VoiceRead {
    /// The profile for one take's reply, or `None` when the reply is not
    /// one - the stage then behaves as it did before the reading existed
    /// rather than pretending to a measurement it does not have.
    pub fn of(reply: &Value) -> Option<VoiceRead> {
        let take = reply.pointer("/audio/path").and_then(Value::as_str)?;
        let lines = reply["lines"]
            .as_array()
            .map(|lines| lines.iter().map(line_of).collect())
            .unwrap_or_default();
        Some(VoiceRead {
            take: take.to_owned(),
            duration: reply.pointer("/audio/durationSec").and_then(Value::as_f64),
            lines,
            pauses: pauses_of(reply),
            entities: entities_of(reply, take),
            motifs: motifs_of(reply),
            take_wide: take_wide_of(reply),
            confidence: confidence_of(reply),
            degradations: reply["degradations"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// The confidence a signal family carries, 0 when the profile did not
    /// rate it at all - so a caller multiplying by it can never treat an
    /// unrated signal as a certain one.
    pub fn weight(&self, family: &str) -> f64 {
        self.confidence.get(family).copied().unwrap_or(0.0)
    }

    /// The line whose span holds a file second, if any does. An
    /// untimed line - start and end both absent, parsed as a zero span -
    /// matches nothing: it is a line the recogniser read as text without
    /// times, and a zero span at second zero would otherwise let it
    /// claim the take's opening moments from the line that really holds
    /// them.
    pub fn line_at(&self, moment: f64) -> Option<&LineRead> {
        self.lines.iter().find(|line| {
            line.end > line.start && line.start <= moment + 0.05 && line.end > moment - 0.05
        })
    }

    /// The line's level over the take's integrated level, in LU. The
    /// producer writes `relativeLu` itself; a foreign reply may carry only
    /// `meanLufsM`, so the take's own integrated level completes it rather
    /// than losing the line's lift.
    pub fn line_lift(&self, line: &LineRead) -> Option<f64> {
        line.relative_lu.or_else(|| {
            line.mean_lufs_m
                .zip(self.take_wide.integrated_lufs)
                .map(|(mean, integrated)| mean - integrated)
        })
    }

    /// Whether the pause opening at `start` is one the profile says to
    /// keep: a dramatic hold or a scene turn, whatever the silence
    /// detector's duration would have suggested.
    pub fn keeps_pause_at(&self, start: f64) -> bool {
        self.pause_at(start).is_some_and(|pause| pause.keep)
    }

    /// The classified silence opening at `start`, if one does.
    pub fn pause_at(&self, start: f64) -> Option<&Pause> {
        self.pauses
            .iter()
            .find(|pause| (pause.start - start).abs() < 0.2)
    }

    /// The class the profile gave the silence at `start`: a scene turn
    /// wants a visible move, a dramatic beat a soft one, a breath the
    /// shortest trim.
    pub fn pause_class_at(&self, start: f64) -> Option<&str> {
        self.pause_at(start).map(|pause| pause.class.as_str())
    }

    /// The unsure words that fall inside a span, in file seconds.
    pub fn unsure_between(&self, from: f64, to: f64) -> Vec<&UnsureWord> {
        self.lines
            .iter()
            .flat_map(|line| line.unsure_words.iter())
            .filter(|word| word.at >= from && word.at <= to)
            .collect()
    }

    /// The safe-remove fillers that fall inside a span, in file seconds.
    pub fn fillers_between(&self, from: f64, to: f64) -> Vec<&Filler> {
        self.lines
            .iter()
            .flat_map(|line| line.fillers.iter())
            .filter(|filler| filler.kind == "safe-remove")
            .filter(|filler| filler.at >= from && filler.at <= to)
            .collect()
    }

    /// Whether the take is dry enough to want a bed: the profile's own
    /// rule, a noise floor under -45 dBFS. `None` when the floor was never
    /// measured, which is not the same answer as "not dry".
    pub fn wants_bed(&self) -> Option<bool> {
        self.take_wide.noise_floor_dbfs.map(|floor| floor < -45.0)
    }

    /// How strongly the take's energy actually repeats, 0..1: the
    /// strongest rhythm layer's strength. A period with no strength behind
    /// it is an artefact of the autocorrelation, so a stage that sizes a
    /// move on the period has to ask this first. `None` when no layer was
    /// measured, which is not the same answer as "no pulse".
    pub fn pulse_strength(&self) -> Option<f64> {
        self.take_wide
            .rhythm_layers
            .iter()
            .map(|layer| layer.strength)
            .fold(None, |best: Option<f64>, strength| {
                Some(best.map_or(strength, |best| best.max(strength)))
            })
    }

    /// The period a seam's blend may be sized on, in seconds: the derived
    /// dominant period when the pulse has strength to stand behind it, or
    /// the strongest layer's own period when the derived key never landed
    /// but the layer table did - the same reading from the row it was
    /// measured on, held to the same strength bar and the same period
    /// bounds (0.2-8 s) as the fallback arm: a derived period outside the
    /// bounds is an autocorrelation artefact on the derived key just as
    /// it is on a layer row. `None` when no layer with strength at or
    /// over 0.25 was measured.
    pub fn pulse_period(&self) -> Option<f64> {
        let derived = self
            .take_wide
            .dominant_period_sec
            .filter(|period| (0.2..=8.0).contains(period))
            .filter(|_| {
                self.pulse_strength()
                    .is_some_and(|strength| strength >= 0.25)
            });
        derived.or_else(|| {
            self.take_wide
                .rhythm_layers
                .iter()
                .filter(|layer| layer.strength >= 0.25 && (0.2..=8.0).contains(&layer.period_sec))
                .max_by(|left, right| left.strength.total_cmp(&right.strength))
                .map(|layer| layer.period_sec)
        })
    }

    /// How long a caption for this line needs on screen, in seconds: the
    /// reading time the profile's rates imply, which is the longer of two
    /// estimates of the same thing. Syllables and words are both proxies
    /// for characters - a syllable is about 3, a word about 5 - and
    /// captions are set at 15 characters a second, so a line of long words
    /// reads slower than its words-per-minute alone suggests. Taking the
    /// longer estimate is the safe direction: a caption under-held is a
    /// caption the viewer cannot finish.
    ///
    /// `None` when neither rate was measured, or the line has no duration
    /// to scale by - a caption's hold then stays whatever the timing alone
    /// asks for, rather than a guess at reading speed.
    pub fn caption_hold(&self, line: &LineRead) -> Option<f64> {
        const CHARS_PER_SECOND: f64 = 15.0;
        let duration = line.end - line.start;
        if duration <= 0.0 {
            return None;
        }
        let by_syllables = line
            .syllables_per_sec
            .map(|rate| rate * duration * 3.0 / CHARS_PER_SECOND);
        let by_words = line
            .words_per_minute
            .map(|wpm| wpm / 60.0 * duration * 5.0 / CHARS_PER_SECOND);
        match (by_syllables, by_words) {
            (Some(syllables), Some(words)) => Some(syllables.max(words)),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        }
    }

    /// The direction the line's delivery travelled: `Some(true)` when the
    /// pitch rose across it, `Some(false)` when it fell, `None` when no
    /// slope was measured or the slope is too flat to call a direction.
    /// A rise builds into what follows; a fall settles.
    pub fn delivery_rises(&self, line: &LineRead) -> Option<bool> {
        const FLAT: f64 = 0.5;
        line.pitch_slope
            .filter(|slope| slope.abs() >= FLAT)
            .map(|slope| slope > 0.0)
    }

    /// Whether the line's pitch range is wide enough to read as performed
    /// rather than reported, on the profile's own 6-semitone bar. `None`
    /// when no range was measured.
    pub fn performs(&self, line: &LineRead) -> Option<bool> {
        line.pitch_range.map(|range| range >= 6.0)
    }

    /// The speaker holding a file second, from the turns when diarization
    /// measured them and from the line's own label otherwise. `None` when
    /// neither names one, which is not the same answer as speaker 0.
    pub fn speaker_at(&self, moment: f64) -> Option<u64> {
        self.take_wide
            .speaker_turns
            .iter()
            .find(|turn| turn.start <= moment && turn.end > moment)
            .map(|turn| turn.speaker)
            .or_else(|| self.line_at(moment).and_then(|line| line.speaker))
    }

    /// The flux onset nearest a file second, when one lands inside the
    /// window: a transient a cut can land on instead of an arbitrary
    /// frame. `None` when the forensic ear measured no onsets there.
    pub fn onset_near(&self, moment: f64, window: f64) -> Option<f64> {
        self.take_wide
            .flux_onsets
            .iter()
            .copied()
            .filter(|onset| (onset - moment).abs() <= window)
            .min_by(|left, right| (left - moment).abs().total_cmp(&(right - moment).abs()))
    }

    /// What a denoise should target, in the profile's own words: the
    /// noise type, and the flatness that says whether the floor is hiss.
    /// `None` when the forensic ear named no noise type. The repairs the
    /// readings ask for are named in turn - a mains hum wants a notch, a
    /// DC offset wants a blocker, a muffled take wants a presence lift -
    /// so the shopping list the log carries says what to fix, not only
    /// what is wrong.
    pub fn noise_character(&self) -> Option<String> {
        let kind = self.take_wide.noise_type.trim();
        if kind.is_empty() {
            return None;
        }
        // A mains hum the Goertzel pass found is the one repair item that
        // names its own frequency: a notch at the mains is the fix, not a
        // broadband denoise that would eat the voice's own low end.
        if let Some(hz) = self.take_wide.hum_hz {
            return Some(format!(
                "{kind} with a {hz:.0} Hz hum: a notch at the mains, not a broadband denoise"
            ));
        }
        // A DC offset is the one repair a denoise cannot make: the bias
        // rides under the floor the profile measured, and only a blocker
        // (or a high-pass at the bottom of the band) removes it.
        if let Some(offset) = self
            .take_wide
            .dc_offset
            .filter(|offset| offset.abs() >= 0.005)
        {
            let percent = offset.abs() * 100.0;
            return Some(format!(
                "{kind} with a {percent:.1}% DC offset: a DC blocker first, then the denoise"
            ));
        }
        // A voice that never reached the presence band is muffled, not
        // noisy: the rolloff under 3.5 kHz and the band under 1.2 kHz
        // together say the clarity is missing rather than masked, and a
        // presence lift is the repair a denoise would only deepen.
        let muffled = match (
            self.take_wide.spectral_rolloff_hz,
            self.take_wide.spectral_bandwidth_hz,
        ) {
            (Some(rolloff), Some(bandwidth)) if rolloff < 3500.0 && bandwidth < 1200.0 => {
                Some(format!(
                    " (rolloff {rolloff:.0} Hz, band {bandwidth:.0} Hz: muffled, so a presence lift, not a denoise)"
                ))
            }
            _ => None,
        };
        Some(match self.take_wide.spectral_flatness {
            Some(flatness) if flatness > 0.25 => {
                format!(
                    "{kind} (flatness {flatness:.2}: broadband, so a spectral denoise fits){muffled}",
                    muffled = muffled.as_deref().unwrap_or_default()
                )
            }
            Some(flatness) => {
                format!(
                    "{kind} (flatness {flatness:.2}: tonal, so a notch fits better than a denoise){muffled}",
                    muffled = muffled.as_deref().unwrap_or_default()
                )
            }
            None => format!(
                "{kind}{muffled}",
                muffled = muffled.as_deref().unwrap_or_default()
            ),
        })
    }

    /// Whether the line's voice is clean enough to treat as measured: an
    /// HNR the forensic ear could read at 10 dB or better, a jitter
    /// under 5% and a shimmer under 6%. `None` when the ear read none of
    /// the three - a line with no voice-quality reading is not the same
    /// answer as a hoarse one, and a caller must not spend the one as the
    /// other.
    pub fn clean_voice(&self, line: &LineRead) -> Option<bool> {
        let hnr = line.hnr_db.map(|hnr| hnr >= 10.0);
        let jitter = line.jitter.map(|jitter| jitter <= 0.05);
        let shimmer = line.shimmer.map(|shimmer| shimmer <= 0.06);
        [hnr, jitter, shimmer]
            .into_iter()
            .fold(None, |held, reading| match (held, reading) {
                (None, next) => next,
                (keep, None) => keep,
                (Some(true), next) => next,
                (Some(false), _) => Some(false),
            })
    }

    /// How far the line's own register sits over the take's, in semitones:
    /// the lift the delivery put under the line. A peak that lands on a
    /// lifted line is a peak the take itself raised, so it outranks a peak
    /// of the same strength on a settled line. `None` when either pitch
    /// was never measured.
    pub fn register_lift(&self, line: &LineRead) -> Option<f64> {
        let take_f0 = self.take_wide.mean_f0_hz?;
        let line_f0 = line.pitch_mean_hz?;
        if take_f0 <= 0.0 || line_f0 <= 0.0 {
            return None;
        }
        Some(12.0 / std::f64::consts::LN_2 * (line_f0 / take_f0).ln())
    }

    /// The hook's own register lift, in semitones: the distance between
    /// the register the contour opened on (its first voiced third over
    /// the take's first three seconds) and the one it rose to (its last
    /// voiced third there). `None` when the contour carried fewer than
    /// six voiced points in the hook - an unmeasured rise is not a flat
    /// one, and a caller must not spend the one as the other.
    pub fn hook_lift(&self) -> Option<f64> {
        let duration = self.duration.filter(|duration| *duration > 0.0)?;
        let contour = &self.take_wide.pitch_contour_hz;
        if contour.is_empty() {
            return None;
        }
        let voiced: Vec<f64> = contour
            .iter()
            .enumerate()
            .filter(|(index, hz)| {
                let moment = *index as f64 * duration / contour.len() as f64;
                moment < 3.0 && **hz > 0.0
            })
            .map(|(_, hz)| *hz)
            .collect();
        if voiced.len() < 6 {
            return None;
        }
        let third = (voiced.len() / 3).max(1);
        let first = voiced[..third].iter().sum::<f64>() / third as f64;
        let last = voiced[voiced.len() - third..].iter().sum::<f64>() / third as f64;
        (first > 0.0).then(|| 12.0 / std::f64::consts::LN_2 * (last / first).ln())
    }

    /// Whether a phrase boundary opens within `window` of a file second:
    /// the narration's own breath-group turned there, which is a seam the
    /// picture can register even when no speaker changed and no question
    /// was asked.
    pub fn phrase_seam(&self, moment: f64, window: f64) -> bool {
        self.take_wide
            .phrases
            .iter()
            .any(|phrase| (phrase.start - moment).abs() <= window)
    }

    /// The take's normalized local energy at a file second, 0..1: where
    /// the moment sits between the envelope's quietest and loudest. The
    /// RMS envelope is the whole take's curve, so a shot placed over a
    /// loud stretch knows it - a lively section earns a livelier drift
    /// than the same-paced line over a quiet one. `None` when no envelope
    /// was measured.
    pub fn local_energy(&self, moment: f64) -> Option<f64> {
        let envelope = &self.take_wide.rms_envelope;
        if envelope.is_empty() || self.duration.unwrap_or(0.0) <= 0.0 {
            return None;
        }
        let position = (moment / self.duration.unwrap_or(0.0)).clamp(0.0, 1.0);
        let index = ((envelope.len() - 1) as f64 * position).round() as usize;
        let value = envelope[index.min(envelope.len() - 1)];
        let low = envelope.iter().copied().fold(f64::INFINITY, f64::min);
        let high = envelope.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if high - low < 1.0e-9 {
            return Some(0.5);
        }
        Some(((value - low) / (high - low)).clamp(0.0, 1.0))
    }

    /// The gain the mix stage may actually apply, after the safety
    /// readings cap what the platform target asked for: a take that
    /// already clips is never raised; a take whose peaks sit near full
    /// scale is only raised to the -1 dBFS (or -1 dBTP, when the loudness
    /// pass measured a true peak) ceiling; a heavily compressed take (a
    /// loudness range under 4 LU) takes at most +6 dB, because everything
    /// the compressor squashed in comes up with the voice.
    pub fn safe_gain(&self, wanted: f64) -> f64 {
        if wanted <= 0.0 {
            return wanted;
        }
        let mut gain = wanted;
        if self
            .take_wide
            .clipping_ratio
            .is_some_and(|ratio| ratio > 0.0)
        {
            return 0.0;
        }
        let ceiling = -1.0;
        if let Some(peak) = self
            .take_wide
            .true_peak_dbfs
            .or(self.take_wide.sample_peak_dbfs)
        {
            gain = gain.min(ceiling - peak);
        }
        if self
            .take_wide
            .loudness_range_lu
            .is_some_and(|lra| lra < 4.0)
        {
            gain = gain.min(6.0);
        }
        gain
    }

    /// Whether the take is a conversation rather than one voice, as the
    /// speakers' own shares answer it: a second speaker holding at least a
    /// fifth of the speech makes the take a dialogue, whatever the
    /// diarization's change points said. `None` when no shares were
    /// measured - an unmeasured take is not the same answer as a
    /// single-voice one.
    pub fn conversation(&self) -> Option<bool> {
        let shares = &self.take_wide.speaker_seconds;
        if shares.is_empty() {
            return None;
        }
        let mut held: Vec<f64> = shares.iter().map(|share| share.speech_seconds).collect();
        held.sort_by(|left, right| right.total_cmp(left));
        let total: f64 = held.iter().sum();
        if total <= 0.0 {
            return Some(false);
        }
        Some(held.get(1).is_some_and(|second| second / total >= 0.2))
    }

    /// Whether the take speaks in bursts, as the VAD's own rhythm answers
    /// it: six or more utterances at a mean of 1.2 s or under is a
    /// machine-gun delivery, the honest fallback for a cut style the
    /// profile derived none. `None` when the ear read no count. A count
    /// without its mean (an ear that answered the utterances and not
    /// their lengths) falls back to the count alone at twelve - twice
    /// the paired arm's bar, since a bare count knows nothing about how
    /// long each burst ran.
    pub fn punchy_delivery(&self) -> Option<bool> {
        match (self.take_wide.utterances, self.take_wide.mean_utterance_sec) {
            (Some(count), Some(mean)) => Some(count >= 6 && mean <= 1.2),
            (Some(count), None) => Some(count >= 12),
            (None, _) => None,
        }
    }

    /// The take's own breath-group length, in seconds: the mean utterance
    /// the VAD measured, which sizes a seam's blend the way the rhythm
    /// period would when the prosody read found no layer strong enough to
    /// stand behind one. `None` when no utterance rhythm was measured.
    pub fn breath_period(&self) -> Option<f64> {
        self.take_wide
            .mean_utterance_sec
            .filter(|sec| (0.2..=8.0).contains(sec))
    }

    /// Whether the take's energy turns over fast, as the flux's own mean
    /// answers against the level it moved from: a mean spectral flux at or
    /// over a third of the envelope's mean RMS is a take that jumps, the
    /// second opinion on a cut style the onsets' rate and the VAD's rhythm
    /// both failed to derive. `None` when either reading was never
    /// measured.
    pub fn moves_hard(&self) -> Option<bool> {
        let flux = self.take_wide.flux_mean?;
        let envelope = &self.take_wide.rms_envelope;
        if envelope.is_empty() {
            return None;
        }
        let mean_rms = envelope.iter().sum::<f64>() / envelope.len() as f64;
        if mean_rms <= 1.0e-6 {
            return None;
        }
        Some(flux / mean_rms >= 0.3)
    }
}

/// One line, from the profile's own row.
fn line_of(row: &Value) -> LineRead {
    let emphasis = row
        .pointer("/prosody/emphasisWords")
        .and_then(Value::as_array)
        .map(|words| {
            words
                .iter()
                .filter_map(|word| {
                    // The Rust ear names the word `wordText`, the Python
                    // pass names it `word`; both time it `t`.
                    let text = word["wordText"]
                        .as_str()
                        .or_else(|| word["word"].as_str())?
                        .trim();
                    if text.is_empty() {
                        return None;
                    }
                    Some(Emphasis {
                        word: text.to_lowercase(),
                        at: word["t"].as_f64().unwrap_or_default(),
                        // A missing strength stays missing rather than
                        // inventing a mid reading: the peaks' ranking and
                        // the clean-voice gate both spend it, and a
                        // half-strength the ear never measured is a lie
                        // on their scale. The Rust ear always writes it;
                        // a foreign producer that drops it loses the
                        // word's emphasis instead of guessing one.
                        strength: word["strength"].as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let unsure_words = row["words"]
        .as_array()
        .map(|words| {
            words
                .iter()
                .filter_map(|word| {
                    let confidence = word["confidence"].as_f64()?;
                    if confidence >= 0.5 {
                        return None;
                    }
                    Some(UnsureWord {
                        word: word["text"].as_str().unwrap_or_default().to_owned(),
                        confidence,
                        at: word["start"].as_f64().unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let fillers = row["fillers"]
        .as_array()
        .map(|fillers| {
            fillers
                .iter()
                .filter_map(|filler| {
                    Some(Filler {
                        word: filler["word"].as_str()?.to_owned(),
                        kind: filler["kind"].as_str().unwrap_or_default().to_owned(),
                        at: filler["start"].as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    LineRead {
        start: row["start"].as_f64().unwrap_or_default(),
        end: row["end"].as_f64().unwrap_or_default(),
        text: row["text"].as_str().unwrap_or_default().trim().to_owned(),
        pace: row
            .pointer("/speechRate/pace")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        words_per_minute: row
            .pointer("/speechRate/wordsPerMinute")
            .and_then(Value::as_f64),
        syllables_per_sec: row
            .pointer("/speechRate/syllablesPerSec")
            .and_then(Value::as_f64),
        pitch_slope: row
            .pointer("/prosody/pitchSlopeSemitonesPerSec")
            .and_then(Value::as_f64),
        pitch_range: row
            .pointer("/prosody/pitchRangeSemitones")
            .and_then(Value::as_f64),
        mean_lufs_m: row.pointer("/loudness/meanLufsM").and_then(Value::as_f64),
        relative_lu: row.pointer("/loudness/relativeLu").and_then(Value::as_f64),
        pitch_mean_hz: row.pointer("/prosody/pitchMeanHz").and_then(Value::as_f64),
        final_rise: row.pointer("/prosody/finalRise").and_then(Value::as_bool),
        voiced_ratio: row.pointer("/prosody/voicedRatio").and_then(Value::as_f64),
        hnr_db: row.pointer("/forensic/hnrDb").and_then(Value::as_f64),
        jitter: row.pointer("/forensic/jitter").and_then(Value::as_f64),
        shimmer: row.pointer("/forensic/shimmer").and_then(Value::as_f64),
        sentence_type: row
            .pointer("/sentence/type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        rise_confirms: row
            .pointer("/sentence/riseConfirms")
            .and_then(Value::as_bool),
        emphasis,
        unsure_words,
        fillers,
        speaker: row["speaker"].as_u64(),
    }
}

fn pauses_of(reply: &Value) -> Vec<Pause> {
    reply["pauses"]
        .as_array()
        .map(|pauses| {
            pauses
                .iter()
                .filter_map(|pause| {
                    Some(Pause {
                        start: pause["start"].as_f64()?,
                        end: pause["end"].as_f64()?,
                        class: pause["class"].as_str().unwrap_or_default().to_owned(),
                        keep: pause["keep"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn entities_of(reply: &Value, take: &str) -> Vec<Entity> {
    // The NER ear's own reading stands behind what it found; the regex
    // complement is a fallback the profile itself flags
    // (`entitiesAvailable` false when anno did not answer), and an
    // approximate entity does not claim the exactness a stat or a
    // timeline card asserts.
    let measured = reply["entitiesAvailable"].as_bool().unwrap_or(false);
    reply["entities"]
        .as_array()
        .map(|entities| {
            entities
                .iter()
                .filter_map(|entity| {
                    let text = entity["text"].as_str()?.trim();
                    if text.is_empty() {
                        return None;
                    }
                    let approximate =
                        !measured || entity["source"].as_str() == Some("regex-complement");
                    Some(Entity {
                        take: take.to_owned(),
                        kind: entity["entityType"]
                            .as_str()
                            .or_else(|| entity["kind"].as_str())
                            .unwrap_or_default()
                            .to_uppercase(),
                        text: text.to_owned(),
                        card: entity["card"].as_str().unwrap_or_default().to_owned(),
                        at: entity["start"].as_f64(),
                        approximate,
                        confidence: entity["confidence"].as_f64(),
                        value: entity["value"].as_f64(),
                        unit: entity["unit"]
                            .as_str()
                            .filter(|unit| !unit.trim().is_empty())
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn motifs_of(reply: &Value) -> Vec<Motif> {
    reply["motifs"]
        .as_array()
        .map(|motifs| {
            motifs
                .iter()
                .filter_map(|motif| {
                    Some(Motif {
                        phrase: motif["phrase"].as_str()?.to_owned(),
                        count: motif["count"].as_u64().unwrap_or(1) as usize,
                        line_indices: motif["lineIndices"]
                            .as_array()
                            .map(|rows| {
                                rows.iter()
                                    .filter_map(Value::as_u64)
                                    .map(|n| n as usize)
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn take_wide_of(reply: &Value) -> TakeRead {
    let hints = &reply["directorHints"];
    let whole = &reply["wholeTake"];
    TakeRead {
        integrated_lufs: whole
            .pointer("/loudness/integratedLufs")
            .and_then(Value::as_f64),
        noise_floor_dbfs: whole
            .pointer("/forensic/noiseFloorDbfs")
            .and_then(Value::as_f64),
        snr_db: whole.pointer("/forensic/snrDb").and_then(Value::as_f64),
        noise_type: whole
            .pointer("/forensic/noiseType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        spectral_flatness: whole
            .pointer("/forensic/spectralFlatness")
            .and_then(Value::as_f64),
        spectral_centroid_hz: whole
            .pointer("/forensic/spectralCentroidHz")
            .and_then(Value::as_f64),
        flux_onsets: whole
            .pointer("/forensic/fluxOnsetsSec")
            .and_then(Value::as_array)
            .map(|onsets| onsets.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default(),
        crest: whole.pointer("/forensic/crest").and_then(Value::as_f64),
        dynamic_range_db: whole
            .pointer("/forensic/dynamicRangeDb")
            .and_then(Value::as_f64),
        voicing_rate: whole
            .pointer("/forensic/voicingRate")
            .and_then(Value::as_f64),
        speech_ratio: whole["speechRatio"].as_f64(),
        cut_style: hints["cutStyle"].as_str().unwrap_or_default().to_owned(),
        cut_rule: hints["cutRule"].as_str().unwrap_or_default().to_owned(),
        font: hints
            .pointer("/language/font")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        music_genre: hints
            .pointer("/language/musicGenre")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        // The profile writes the bed's verdict as `bgm.recommendation`
        // (`add` / `skip` / `duck_only`); reading `decision` left it None
        // on every take, so the skip and duck_only rules never fired.
        bgm_decision: hints
            .pointer("/bgm/recommendation")
            .and_then(Value::as_str)
            .map(str::to_owned),
        bgm_reason: hints
            .pointer("/bgm/reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        dominant_period_sec: whole["dominantPeriodSec"].as_f64(),
        phrases: whole["phrases"]
            .as_array()
            .map(|phrases| {
                phrases
                    .iter()
                    .filter_map(|phrase| {
                        Some(PhraseSpan {
                            start: phrase["startSec"].as_f64()?,
                            end: phrase["endSec"].as_f64()?,
                            label: phrase["label"].as_str().unwrap_or_default().to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        rms_envelope: whole
            .pointer("/forensic/rmsEnvelope")
            .and_then(Value::as_array)
            .map(|points| points.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default(),
        pitch_contour_hz: whole
            .pointer("/pitchContourHz")
            .and_then(Value::as_array)
            .map(|points| points.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default(),
        flux_mean: whole.pointer("/forensic/fluxMean").and_then(Value::as_f64),
        mean_f0_hz: whole.pointer("/forensic/meanF0Hz").and_then(Value::as_f64),
        sample_peak_dbfs: whole
            .pointer("/forensic/samplePeakDbfs")
            .and_then(Value::as_f64),
        clipping_ratio: whole
            .pointer("/forensic/clippingRatio")
            .and_then(Value::as_f64),
        sibilance_ratio: whole
            .pointer("/forensic/sibilanceRatio")
            .and_then(Value::as_f64),
        hum_hz: whole.pointer("/forensic/humHz").and_then(Value::as_f64),
        dc_offset: whole.pointer("/forensic/dcOffset").and_then(Value::as_f64),
        spectral_rolloff_hz: whole
            .pointer("/forensic/spectralRolloffHz")
            .and_then(Value::as_f64),
        spectral_bandwidth_hz: whole
            .pointer("/forensic/spectralBandwidthHz")
            .and_then(Value::as_f64),
        onset_rate_per_min: whole
            .pointer("/forensic/onsetRatePerMin")
            .and_then(Value::as_f64),
        music_likelihood: whole
            .pointer("/forensic/musicLikelihood")
            .and_then(Value::as_f64),
        reverb_tail_sec: whole
            .pointer("/forensic/reverbTailSec")
            .and_then(Value::as_f64),
        loudness_range_lu: whole
            .pointer("/loudness/loudnessRangeLu")
            .and_then(Value::as_f64),
        true_peak_dbfs: whole
            .pointer("/loudness/truePeakDbfs")
            .and_then(Value::as_f64),
        // An absent speaker ear answers `utterances: 0` (the field is not
        // Option on the producer's side), and a bare zero would read as a
        // measured "not punchy" below - the ear's own `available` flag
        // gates the count, so an ear that never ran stays unmeasured.
        utterances: reply
            .pointer("/speaker/utterances")
            .and_then(Value::as_u64)
            .filter(|_| reply.pointer("/speaker/available").and_then(Value::as_bool) == Some(true)),
        mean_utterance_sec: reply
            .pointer("/speaker/meanUtteranceSec")
            .and_then(Value::as_f64),
        longest_silence_sec: reply
            .pointer("/speaker/longestSilenceSec")
            .and_then(Value::as_f64),
        speaker_seconds: reply
            .pointer("/speaker/speakers")
            .and_then(Value::as_array)
            .map(|speakers| {
                speakers
                    .iter()
                    .filter_map(|speaker| {
                        Some(SpeakerShare {
                            id: speaker["id"].as_u64()?,
                            speech_seconds: speaker["speechSeconds"].as_f64()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        rhythm_layers: whole["prosody"]
            .as_array()
            .map(|layers| {
                layers
                    .iter()
                    .filter_map(|layer| {
                        Some(RhythmLayer {
                            period_sec: layer["periodSec"].as_f64()?,
                            strength: layer["strength"].as_f64().unwrap_or(0.0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        // The profile writes a change as `{t, from, to}`; reading the
        // array as bare numbers left this empty on every take that had
        // diarization, so the seam that `speakerChanges` is meant to
        // drive never fired.
        speaker_changes: reply
            .pointer("/speaker/changes")
            .and_then(Value::as_array)
            .map(|changes| {
                changes
                    .iter()
                    .filter_map(|change| {
                        change
                            .get("t")
                            .and_then(Value::as_f64)
                            .or_else(|| change.as_f64())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        speaker_turns: reply
            .pointer("/speaker/turns")
            .and_then(Value::as_array)
            .map(|turns| {
                turns
                    .iter()
                    .filter_map(|turn| {
                        Some(SpeakerTurn {
                            start: turn["start"].as_f64()?,
                            end: turn["end"].as_f64()?,
                            speaker: turn["speaker"].as_u64().unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        questions: hints["questions"]
            .as_array()
            .map(|questions| {
                questions
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|index| index as usize)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn confidence_of(reply: &Value) -> BTreeMap<String, f64> {
    reply
        .pointer("/signalConfidence/perSignal")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some((
                        row["signal"].as_str()?.to_owned(),
                        row["confidence"].as_f64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read_with_layers(layers: Vec<(f64, f64)>) -> VoiceRead {
        let mut read = VoiceRead::default();
        read.take_wide.rhythm_layers = layers
            .into_iter()
            .map(|(period_sec, strength)| RhythmLayer {
                period_sec,
                strength,
            })
            .collect();
        read
    }

    /// The strongest layer is the one a stage can trust: the micro period
    /// inside a phrase is often measured with less strength than the macro
    /// one the phrases sit on, and taking the first would size a seam on
    /// the weaker reading.
    #[test]
    fn the_pulse_strength_is_the_strongest_layer_s() {
        let read = read_with_layers(vec![(1.8, 0.2), (0.6, 0.7)]);
        assert_eq!(read.pulse_strength(), Some(0.7));
    }

    /// No layer measured is not the same answer as no pulse: a stage must
    /// be able to tell "the rhythm read found nothing" from "the rhythm
    /// read never ran".
    #[test]
    fn an_unmeasured_pulse_is_none_not_zero() {
        assert_eq!(read_with_layers(Vec::new()).pulse_strength(), None);
    }

    /// The profile's own JSON, read as the profile writes it. Two paths
    /// were wrong for months and nothing failed: `bgm.decision` is really
    /// `bgm.recommendation`, and `speaker.changes` holds `{t, from, to}`
    /// objects rather than bare numbers. Both readings came back empty on
    /// every real take, so the skip rule and the speaker seam were dead
    /// while their code looked exercised. This test builds the reply from
    /// the producer's actual shape, so a path that drifts fails here.
    #[test]
    fn the_profile_s_own_shape_is_what_is_read() {
        let reply = json!({
            "audio": { "path": "/t/a.wav", "durationSec": 12.0 },
            "entitiesAvailable": true,
            "wholeTake": {
                "loudness": { "integratedLufs": -16.5, "loudnessRangeLu": 8.0,
                              "truePeakDbfs": -0.4 },
                "prosody": [ { "periodSec": 0.6, "strength": 0.7 } ],
                "dominantPeriodSec": 0.6,
                "phrases": [ { "startSec": 3.0, "endSec": 6.2, "label": "p", "averageEnergy": 0.1 } ],
                "pitchContourHz": [110.0, 0.0, 220.0],
                "forensic": {
                    "noiseFloorDbfs": -38.0, "snrDb": 9.5, "noiseType": "hiss",
                    "spectralFlatness": 0.42, "fluxOnsetsSec": [1.0, 3.25],
                    "samplePeakDbfs": -3.2, "clippingRatio": 0.0,
                    "sibilanceRatio": 0.07, "humHz": null, "fluxMean": 0.05,
                    "meanF0Hz": 140.0, "onsetRatePerMin": 40.0,
                    "musicLikelihood": 0.1, "reverbTailSec": null,
                    "rmsEnvelope": [0.1, 0.5, 0.9, 0.2],
                },
                "speechRatio": 0.71,
            },
            "directorHints": {
                "bgm": { "recommendation": "skip", "reason": "already musical" },
            },
            "speaker": {
                // The ear's own availability flag, exactly as the Rust
                // ear writes it: an absent ear answers utterances 0, and
                // only `available` tells a measured count from none.
                "available": true,
                "method": "silero-vad + spectral clustering",
                "utterances": 7, "meanUtteranceSec": 1.6, "longestSilenceSec": 1.9,
                "speakers": [ { "id": 0, "speechSeconds": 8.4, "lineIndices": [0] },
                              { "id": 1, "speechSeconds": 2.1, "lineIndices": [1] } ],
                "changes": [
                    { "t": 4.5, "from": 0, "to": 1 },
                    { "t": 9.0, "from": 1, "to": 0 },
                ],
                "turns": [
                    { "start": 0.0, "end": 4.5, "speaker": 0 },
                    { "start": 4.5, "end": 9.0, "speaker": 1 },
                ],
            },
            "lines": [
                { "start": 0.0, "end": 4.0, "text": "a line",
                  "loudness": { "meanLufsM": -14.0 },
                  "prosody": { "pitchMeanHz": 175.0, "finalRise": true, "voicedRatio": 0.9 },
                  "forensic": { "hnrDb": 18.0, "jitter": 0.01, "shimmer": 0.02 } },
            ],
            "entities": [ { "text": "IBM", "entityType": "ORG", "card": "lower-third",
                            "start": 2.0, "source": "anno" } ],
        });
        let read = VoiceRead::of(&reply).expect("a read");
        assert_eq!(read.take_wide.bgm_decision.as_deref(), Some("skip"));
        assert_eq!(
            read.take_wide.speaker_changes,
            vec![4.5, 9.0],
            "the change times live under `t`"
        );
        assert_eq!(read.take_wide.speaker_turns.len(), 2);
        assert_eq!(read.take_wide.speaker_turns[1].speaker, 1);
        assert_eq!(read.take_wide.noise_type, "hiss");
        assert_eq!(read.take_wide.snr_db, Some(9.5));
        assert_eq!(read.take_wide.flux_onsets, vec![1.0, 3.25]);
        assert_eq!(read.onset_near(3.3, 0.1), Some(3.25));
        assert_eq!(read.take_wide.integrated_lufs, Some(-16.5));
        assert_eq!(read.pulse_strength(), Some(0.7));
        // The fifty-ear readings, at the paths the profile writes them.
        assert_eq!(read.take_wide.sample_peak_dbfs, Some(-3.2));
        assert_eq!(read.take_wide.clipping_ratio, Some(0.0));
        assert_eq!(read.take_wide.sibilance_ratio, Some(0.07));
        assert_eq!(read.take_wide.hum_hz, None);
        assert_eq!(read.take_wide.flux_mean, Some(0.05));
        assert_eq!(read.take_wide.mean_f0_hz, Some(140.0));
        assert_eq!(read.take_wide.onset_rate_per_min, Some(40.0));
        assert_eq!(read.take_wide.music_likelihood, Some(0.1));
        assert_eq!(read.take_wide.reverb_tail_sec, None);
        assert_eq!(read.take_wide.loudness_range_lu, Some(8.0));
        assert_eq!(read.take_wide.true_peak_dbfs, Some(-0.4));
        assert_eq!(read.take_wide.rms_envelope, vec![0.1, 0.5, 0.9, 0.2]);
        assert_eq!(read.take_wide.phrases.len(), 1);
        assert_eq!(read.take_wide.phrases[0].start, 3.0);
        assert_eq!(read.take_wide.utterances, Some(7));
        assert_eq!(read.take_wide.mean_utterance_sec, Some(1.6));
        assert_eq!(read.take_wide.longest_silence_sec, Some(1.9));
        assert_eq!(read.take_wide.speaker_seconds.len(), 2);
        assert_eq!(read.take_wide.speaker_seconds[1].speech_seconds, 2.1);
        // The per-line readings the new consumers spend.
        let line = &read.lines[0];
        assert_eq!(line.mean_lufs_m, Some(-14.0));
        // The producer's `relativeLu` is absent here, so the take's
        // integrated level completes the line's lift: -14 over -16.5.
        assert_eq!(read.line_lift(line), Some(2.5));
        assert_eq!(line.pitch_mean_hz, Some(175.0));
        assert_eq!(line.final_rise, Some(true));
        assert_eq!(line.voiced_ratio, Some(0.9));
        assert_eq!(line.hnr_db, Some(18.0));
        assert_eq!(line.jitter, Some(0.01));
        assert_eq!(read.clean_voice(line), Some(true));
        // The register lift: 175 Hz over the take's 140 Hz is an octave
        // shy of a fourth - two semitones and a bit.
        let lift = read.register_lift(line).expect("a lift");
        assert!((3.0..=4.0).contains(&lift), "lift {lift}");
        // The phrase seam answers at its own boundary.
        assert!(read.phrase_seam(3.05, 0.2));
        assert!(!read.phrase_seam(8.0, 0.2));
        // The local energy reads the envelope's own curve.
        assert!(read.local_energy(6.0) > Some(0.9));
        assert!(read.local_energy(0.0) < Some(0.2));
        // The gain ceiling: a take peaking at -0.4 dBTP takes at most
        // -0.6 dB however far the platform target sits above it.
        assert_eq!(read.safe_gain(9.0), -0.6);
        assert_eq!(read.safe_gain(-2.0), -2.0, "a cut is never capped");
        // An entity the NER measured is not approximate.
        assert!(!read.entities[0].approximate);
    }

    /// The regex complement is a fallback the profile flags, and the
    /// reader must not let it claim the exactness a stat card asserts:
    /// `entitiesAvailable: false` marks every entity approximate, however
    /// confident the complement's own pattern felt.
    #[test]
    fn a_complement_entity_is_approximate_whatever_it_found() {
        let reply = json!({
            "audio": { "path": "/t/a.wav", "durationSec": 12.0 },
            "entitiesAvailable": false,
            "lines": [],
            "entities": [ { "text": "$10,450", "entityType": "MONEY", "card": "stat",
                            "start": 2.0, "source": "regex-complement" } ],
        });
        let read = VoiceRead::of(&reply).expect("a read");
        assert!(read.entities[0].approximate);
    }

    /// The gain ceiling's other two rules: a clipped take is never raised
    /// at all, and a compressed take (loudness range under 4 LU) takes at
    /// most +6 dB of however much the platform target asked for.
    #[test]
    fn the_gain_ceiling_respects_the_clip_and_the_compression() {
        let mut read = VoiceRead::default();
        read.take_wide.clipping_ratio = Some(0.001);
        assert_eq!(read.safe_gain(4.0), 0.0, "a clipped take is not raised");
        read.take_wide.clipping_ratio = Some(0.0);
        read.take_wide.loudness_range_lu = Some(2.5);
        read.take_wide.sample_peak_dbfs = Some(-12.0);
        assert_eq!(
            read.safe_gain(9.0),
            6.0,
            "a compressed take takes at most +6 dB"
        );
    }

    /// The conversation reading: the shares, not the change points, say
    /// whether the take is a dialogue. A second speaker under a fifth of
    /// the speech is a clustering wobble, not a turn.
    #[test]
    fn the_shares_say_whether_the_take_is_a_dialogue() {
        let mut read = VoiceRead::default();
        assert_eq!(read.conversation(), None, "no shares measured is no answer");
        read.take_wide.speaker_seconds = vec![SpeakerShare {
            id: 0,
            speech_seconds: 9.0,
        }];
        assert_eq!(
            read.conversation(),
            Some(false),
            "one voice is not a dialogue"
        );
        read.take_wide.speaker_seconds = vec![
            SpeakerShare {
                id: 0,
                speech_seconds: 8.0,
            },
            SpeakerShare {
                id: 1,
                speech_seconds: 2.0,
            },
        ];
        assert_eq!(
            read.conversation(),
            Some(true),
            "a fifth of the speech is already a turn"
        );
        read.take_wide.speaker_seconds = vec![
            SpeakerShare {
                id: 0,
                speech_seconds: 8.0,
            },
            SpeakerShare {
                id: 1,
                speech_seconds: 4.0,
            },
        ];
        assert_eq!(
            read.conversation(),
            Some(true),
            "a third of the speech is a dialogue"
        );
    }

    /// The utterance rhythm's two spends: the burst reading that backs a
    /// fast cut style, and the breath-group length that sizes a seam's
    /// blend when the rhythm layers found no period to stand behind.
    #[test]
    fn the_vad_rhythm_answers_bursts_and_breath_groups() {
        let mut read = VoiceRead::default();
        assert_eq!(read.punchy_delivery(), None);
        assert_eq!(read.breath_period(), None);
        read.take_wide.utterances = Some(7);
        read.take_wide.mean_utterance_sec = Some(1.0);
        assert_eq!(
            read.punchy_delivery(),
            Some(true),
            "seven short bursts is punchy"
        );
        assert_eq!(read.breath_period(), Some(1.0));
        read.take_wide.mean_utterance_sec = Some(3.2);
        assert_eq!(
            read.punchy_delivery(),
            Some(false),
            "long utterances are not bursts"
        );
        read.take_wide.mean_utterance_sec = Some(0.1);
        assert_eq!(
            read.breath_period(),
            None,
            "a span that short is not a breath-group"
        );
        read.take_wide.mean_utterance_sec = Some(9.0);
        assert_eq!(
            read.breath_period(),
            None,
            "a span that long is not a breath-group"
        );
        read.take_wide.utterances = Some(2);
        read.take_wide.mean_utterance_sec = None;
        assert_eq!(
            read.punchy_delivery(),
            Some(false),
            "two utterances is not a burst"
        );
    }

    /// The flux's mean against the envelope it moved from: the same flux
    /// over a loud take is a shuffle, over a quiet one a jump - the
    /// reading is the ratio, not the raw number.
    #[test]
    fn the_flux_means_read_against_the_level_it_moved_from() {
        let mut read = VoiceRead::default();
        assert_eq!(read.moves_hard(), None, "no flux measured is no answer");
        read.take_wide.flux_mean = Some(0.05);
        assert_eq!(read.moves_hard(), None, "no envelope measured is no answer");
        read.take_wide.rms_envelope = vec![0.05; 10];
        // A mean RMS of 0.05 against a flux of 0.05 is a ratio of one.
        assert_eq!(read.moves_hard(), Some(true));
        read.take_wide.rms_envelope = vec![0.5; 10];
        // The same flux over a take ten times louder is a tenth of it.
        assert_eq!(read.moves_hard(), Some(false));
    }

    /// The repair shopping list's own additions: a DC offset names its
    /// blocker, and a muffled take (a rolloff and a band that never
    /// reached the presence band) asks for a lift rather than a denoise.
    #[test]
    fn the_repair_list_names_the_blocker_and_the_lift() {
        let mut read = VoiceRead::default();
        read.take_wide.noise_type = "hiss".to_owned();
        read.take_wide.dc_offset = Some(0.02);
        assert_eq!(
            read.noise_character().as_deref(),
            Some("hiss with a 2.0% DC offset: a DC blocker first, then the denoise")
        );
        read.take_wide.dc_offset = Some(0.0);
        read.take_wide.spectral_rolloff_hz = Some(3100.0);
        read.take_wide.spectral_bandwidth_hz = Some(900.0);
        assert_eq!(
            read.noise_character().as_deref(),
            Some("hiss (rolloff 3100 Hz, band 900 Hz: muffled, so a presence lift, not a denoise)")
        );
        read.take_wide.spectral_rolloff_hz = Some(5200.0);
        assert_ne!(
            read.noise_character().as_deref().unwrap_or_default(),
            "",
            "a take that reached the presence band keeps the plain character"
        );
        assert!(
            !read
                .noise_character()
                .unwrap_or_default()
                .contains("muffled")
        );
    }

    /// The voice-quality gate's third arm: a shimmer over 6% retires a
    /// line an HNR alone would have passed, and a reading the ear never
    /// made changes nothing.
    #[test]
    fn a_shaky_line_is_not_a_clean_one_even_with_a_good_hnr() {
        let read = VoiceRead::default();
        let steady = LineRead {
            hnr_db: Some(18.0),
            jitter: Some(0.01),
            shimmer: Some(0.02),
            ..LineRead::default()
        };
        assert_eq!(read.clean_voice(&steady), Some(true));
        let shaky = LineRead {
            hnr_db: Some(18.0),
            jitter: Some(0.01),
            shimmer: Some(0.09),
            ..LineRead::default()
        };
        assert_eq!(read.clean_voice(&shaky), Some(false));
        let unread = LineRead {
            hnr_db: Some(18.0),
            ..LineRead::default()
        };
        assert_eq!(
            read.clean_voice(&unread),
            Some(true),
            "an unmeasured shimmer is not a shaky one"
        );
    }

    /// The hook's own lift, read from the contour itself: a delivery
    /// that rose across the opening three seconds performed the hook
    /// whatever the level meter read, and the caption stage spends that
    /// rise as weight. A contour with too few voiced points in the hook
    /// is an unmeasured rise, not a flat one.
    #[test]
    fn the_hook_lift_reads_the_contour_s_own_rise() {
        // Twenty-four points over nine seconds: the first eight (t < 3.0)
        // are the hook's own. The hook opens on 110 Hz and closes an
        // octave up - twelve semitones, the loudest claim a rise can make.
        let mut contour = vec![110.0_f64; 4];
        contour.extend(vec![220.0; 4]);
        contour.extend(vec![0.0; 16]);
        let read = VoiceRead {
            duration: Some(9.0),
            take_wide: TakeRead {
                pitch_contour_hz: contour,
                ..TakeRead::default()
            },
            ..VoiceRead::default()
        };
        let lift = read.hook_lift().expect("a rising hook answers");
        assert!(
            (lift - 12.0).abs() < 0.01,
            "an octave is twelve semitones, not {lift}"
        );
        // A hook the contour barely voiced: three points is too few to
        // claim a rise, and `None` is the honest answer.
        let mut sparse_contour = vec![110.0_f64; 3];
        sparse_contour.extend(vec![0.0; 21]);
        let sparse = VoiceRead {
            duration: Some(9.0),
            take_wide: TakeRead {
                pitch_contour_hz: sparse_contour,
                ..TakeRead::default()
            },
            ..VoiceRead::default()
        };
        assert_eq!(sparse.hook_lift(), None, "an unvoiced hook is unmeasured");
    }

    /// An untimed line - start and end both absent, parsed as a zero
    /// span - claims no moment: before the guard, its zero span at
    /// second zero let it shadow the line that really holds the take's
    /// opening words.
    #[test]
    fn an_untimed_line_claims_no_moment() {
        let read = VoiceRead {
            lines: vec![
                LineRead {
                    text: "untimed".to_owned(),
                    ..LineRead::default()
                },
                LineRead {
                    text: "timed".to_owned(),
                    start: 0.0,
                    end: 2.0,
                    ..LineRead::default()
                },
            ],
            ..VoiceRead::default()
        };
        let line = read.line_at(1.0).expect("the timed line holds second one");
        assert_eq!(line.text, "timed");
        let opening = read
            .line_at(0.0)
            .expect("the opening second belongs to the timed line too");
        assert_eq!(opening.text, "timed");
    }

    /// A bare utterance count - an ear that answered the count and not
    /// the lengths - still answers the burst question, at twice the
    /// paired arm's bar: twelve utterances without a mean is machine-gun
    /// delivery, six is not.
    #[test]
    fn a_bare_utterance_count_answers_at_twice_the_bar() {
        let with = |utterances: Option<u64>| VoiceRead {
            take_wide: TakeRead {
                utterances,
                ..TakeRead::default()
            },
            ..VoiceRead::default()
        };
        assert_eq!(with(Some(12)).punchy_delivery(), Some(true));
        assert_eq!(with(Some(6)).punchy_delivery(), Some(false));
        assert_eq!(with(None).punchy_delivery(), None);
    }

    /// An absent speaker ear answers `utterances: 0` - the field is not
    /// Option on the producer's side - and only its own `available` flag
    /// tells a measured count from none. The parse gates on the flag, so
    /// a take the ear never read keeps its rhythm unmeasured rather
    /// than reading as a measured "not punchy".
    #[test]
    fn an_absent_speaker_ear_keeps_its_rhythm_unmeasured() {
        let reply = json!({
            "audio": { "path": "/t/a.wav", "durationSec": 9.0 },
            "speaker": {
                "available": false, "method": "", "reason": "no WAV in the spec",
                "utterances": 0, "meanUtteranceSec": null, "longestSilenceSec": null,
                "speech": [], "turns": [], "speakers": [], "changes": [],
                "lineSpeakers": [],
            },
        });
        let read = VoiceRead::of(&reply).expect("the reply is a reading");
        assert_eq!(read.take_wide.utterances, None);
        assert_eq!(read.punchy_delivery(), None);
        // The ear that answered keeps its count.
        let present = json!({
            "audio": { "path": "/t/a.wav", "durationSec": 9.0 },
            "speaker": {
                "available": true, "method": "silero-vad + spectral clustering",
                "utterances": 7, "meanUtteranceSec": 1.6, "longestSilenceSec": 1.9,
                "speech": [], "turns": [], "speakers": [], "changes": [],
                "lineSpeakers": [],
            },
        });
        let read = VoiceRead::of(&present).expect("the reply is a reading");
        assert_eq!(read.take_wide.utterances, Some(7));
    }

    /// A pause's class is a decision, not metadata: a scene turn earns a
    /// visible move and a dramatic beat a soft one, so the seam reads the
    /// class rather than only the keep flag.
    #[test]
    fn a_pause_carries_the_class_the_seam_spends() {
        let mut read = VoiceRead::default();
        read.pauses = vec![
            Pause {
                start: 2.0,
                end: 3.2,
                class: "scene_turn".to_owned(),
                keep: true,
            },
            Pause {
                start: 7.0,
                end: 8.0,
                class: "dramatic".to_owned(),
                keep: true,
            },
        ];
        assert_eq!(read.pause_class_at(2.05), Some("scene_turn"));
        assert_eq!(read.pause_class_at(7.0), Some("dramatic"));
        assert_eq!(read.pause_class_at(5.0), None);
        assert!(read.keeps_pause_at(2.0));
    }
}
