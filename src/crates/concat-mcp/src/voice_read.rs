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
    /// The line's level against the take's integrated level, in LU.
    pub relative_lu: Option<f64>,
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
    /// The share of the take that is speech, 0..1.
    pub speech_ratio: Option<f64>,
    /// Whether the take already carries music, as the music gate measured.
    pub music_present: Option<bool>,
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
    /// The lines whose punctuation asks a question, by index.
    pub questions: Vec<usize>,
    /// The take's tempo: the strongest rhythm layer's period, in seconds.
    /// The cut stage's own grid - a cut lands on a multiple of this.
    pub dominant_period_sec: Option<f64>,
    /// The rhythm layers the prosody read found, strongest first: the
    /// macro period the phrases sit on and the micro period inside them.
    pub rhythm_layers: Vec<RhythmLayer>,
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

    /// The line whose span holds a file second, if any does.
    pub fn line_at(&self, moment: f64) -> Option<&LineRead> {
        self.lines
            .iter()
            .find(|line| line.start <= moment + 0.05 && line.end > moment - 0.05)
    }

    /// Whether the pause opening at `start` is one the profile says to
    /// keep: a dramatic hold or a scene turn, whatever the silence
    /// detector's duration would have suggested.
    pub fn keeps_pause_at(&self, start: f64) -> bool {
        self.pauses
            .iter()
            .any(|pause| (pause.start - start).abs() < 0.2 && pause.keep)
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
                        strength: word["strength"].as_f64().unwrap_or(0.5),
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
        relative_lu: row.pointer("/loudness/relativeLu").and_then(Value::as_f64),
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
        speech_ratio: whole["speechRatio"].as_f64(),
        music_present: reply["musicPresent"].as_bool(),
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
        bgm_decision: hints
            .pointer("/bgm/decision")
            .and_then(Value::as_str)
            .map(str::to_owned),
        bgm_reason: hints
            .pointer("/bgm/reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        dominant_period_sec: whole["dominantPeriodSec"].as_f64(),
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
        speaker_changes: reply
            .pointer("/speaker/changes")
            .and_then(Value::as_array)
            .map(|changes| changes.iter().filter_map(Value::as_f64).collect())
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
}
