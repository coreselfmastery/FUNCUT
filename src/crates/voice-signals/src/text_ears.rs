// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The text ear: what a line names, built on `anno`.
//!
//! Every line's text goes through the offline pattern NER (names,
//! organisations, places, dates, money, and the rest of the label set).
//! The entities come back with character offsets; those are mapped onto
//! the line's measured words - so `"$10,450"` is not just a MONEY span
//! but the word at 12.4 s - and each entity carries the edit artefact it
//! suggests: a stat card, a lower-third, a timeline graphic, or a redact
//! flag when it is private information.
//!
//! Mapping is best-effort and says so: an entity whose span no word
//! claims (it sat over punctuation, or the transcript's text and words
//! drifted) is reported with `approximate: true` and the line's own span.

use serde::Serialize;

use crate::{Error, LineSpec};

/// Import the trait whose method reads the entities.
use anno::Model as _;

/// One entity, mapped onto the timeline.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EntityOut {
    /// The line the entity was found in (index into the spec's lines).
    pub line_index: usize,
    /// The entity's surface text.
    pub text: String,
    /// The NER label (PER, ORG, MONEY, DATE, ...).
    pub entity_type: String,
    /// The backend's own confidence.
    pub confidence: f64,
    /// Start in seconds, from the mapped words.
    pub start: Option<f64>,
    /// End in seconds, from the mapped words.
    pub end: Option<f64>,
    /// The first mapped word's index in the line, when mapping held.
    pub word_start: Option<usize>,
    /// One past the last mapped word's index, when mapping held.
    pub word_end: Option<usize>,
    /// Whether the time mapping is approximate (entity to line span).
    pub approximate: bool,
    /// The edit artefact this entity suggests.
    pub card: &'static str,
}

/// Read the entities of every line.
pub fn entities(lines: &[LineSpec]) -> Result<Vec<EntityOut>, Error> {
    let model = anno::StackedNER::default();
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let text = line.text.trim();
        if text.is_empty() {
            continue;
        }
        let found = model
            .extract_entities(text, None)
            .map_err(|e| Error::Analysis(format!("NER on line {index}: {e}")))?;
        for entity in found {
            let (word_start, word_end, approximate) =
                map_to_words(line, entity.start(), entity.end());
            let (start, end) = match (word_start, word_end) {
                (Some(ws), Some(we)) if we > ws => {
                    (Some(line.words[ws].start), Some(line.words[we - 1].end))
                }
                _ => (line.start, line.end),
            };
            out.push(EntityOut {
                line_index: index,
                text: entity.text.clone(),
                entity_type: entity.entity_type.to_string(),
                confidence: (entity.confidence * 10_000.0).round() / 10_000.0,
                start: start.map(round3),
                end: end.map(round3),
                word_start,
                word_end,
                approximate,
                card: card_for(&entity.entity_type.to_string()),
            });
        }
    }
    Ok(out)
}

/// Map a character span onto the line's words. Returns
/// `(first word, one-past-last word, approximate)`.
fn map_to_words(
    line: &LineSpec,
    start_char: usize,
    end_char: usize,
) -> (Option<usize>, Option<usize>, bool) {
    if line.words.is_empty() {
        return (None, None, line.start.is_some());
    }
    let spans = word_char_spans(line);
    let mut first: Option<usize> = None;
    let mut last: Option<usize> = None;
    for (i, (ws, we)) in spans.iter().enumerate() {
        // A word claims the entity when they overlap in characters.
        if *ws < end_char && *we > start_char {
            if first.is_none() {
                first = Some(i);
            }
            last = Some(i + 1);
        }
    }
    match (first, last) {
        (Some(f), Some(l)) => (Some(f), Some(l), false),
        _ => {
            // No word claimed the span: nearest word by character
            // distance, and flagged approximate.
            let mut best = 0_usize;
            let mut best_distance = usize::MAX;
            for (i, (ws, we)) in spans.iter().enumerate() {
                // Distance to the entity span: how far the word sits
                // before or after it, 0 when they overlap.
                let distance = ws
                    .checked_sub(end_char)
                    .or_else(|| start_char.checked_sub(*we))
                    .unwrap_or(0);
                if distance < best_distance {
                    best_distance = distance;
                    best = i;
                }
            }
            (Some(best), Some(best + 1), true)
        }
    }
}

/// Each word's character span in the line's text, found by walking the
/// text and matching each word's own characters in order. When a word
/// cannot be found at its position (the transcript's text and words
/// drifted), its span is interpolated from its neighbours.
fn word_char_spans(line: &LineSpec) -> Vec<(usize, usize)> {
    let text = line.text.as_str();
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::with_capacity(line.words.len());
    let mut cursor = 0_usize;
    for word in &line.words {
        let target: Vec<char> = word.text.chars().collect();
        if target.is_empty() {
            spans.push((cursor, cursor));
            continue;
        }
        let mut found: Option<usize> = None;
        let mut probe = cursor;
        while probe + target.len() <= chars.len() {
            if chars[probe..probe + target.len()] == target[..] {
                found = Some(probe);
                break;
            }
            probe += 1;
        }
        match found {
            Some(at) => {
                spans.push((at, at + target.len()));
                cursor = at + target.len();
            }
            None => {
                // Interpolate: the word sits proportionally where its
                // remaining words do not.
                let at = cursor.min(chars.len().saturating_sub(1));
                spans.push((at, at));
            }
        }
    }
    spans
}

/// The edit artefact an entity type suggests.
fn card_for(entity_type: &str) -> &'static str {
    match entity_type {
        "MONEY" | "CARDINAL" | "QUANTITY" | "PERCENT" => "stat",
        "DATE" | "TIME" => "timeline",
        "EMAIL" | "PHONE" | "SSN" | "ADDRESS" | "IP" | "PII" => "redact",
        "PER" | "ORG" | "LOC" | "GPE" | "PRODUCT" | "EVENT" | "WORK_OF_ART" | "NORP" | "FAC"
        | "LAW" | "LANGUAGE" => "lower-third",
        _ => "mention",
    }
}

/// Round to three decimals.
fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WordSpec;

    fn line(text: &str, words: Vec<WordSpec>) -> LineSpec {
        LineSpec {
            start: Some(1.0),
            end: Some(3.0),
            text: text.into(),
            words,
        }
    }

    #[test]
    fn money_and_organisations_are_found_and_timed() {
        let lines = vec![line(
            "Lynn Conway worked at IBM and earned $10,450 in 2026.",
            vec![
                WordSpec {
                    start: 1.0,
                    end: 1.3,
                    text: "Lynn".into(),
                },
                WordSpec {
                    start: 1.3,
                    end: 1.6,
                    text: "Conway".into(),
                },
                WordSpec {
                    start: 1.6,
                    end: 1.9,
                    text: "worked".into(),
                },
                WordSpec {
                    start: 1.9,
                    end: 2.1,
                    text: "at".into(),
                },
                WordSpec {
                    start: 2.1,
                    end: 2.4,
                    text: "IBM".into(),
                },
                WordSpec {
                    start: 2.4,
                    end: 2.7,
                    text: "and".into(),
                },
                WordSpec {
                    start: 2.7,
                    end: 3.0,
                    text: "earned".into(),
                },
                WordSpec {
                    start: 3.0,
                    end: 3.4,
                    text: "$10,450".into(),
                },
            ],
        )];
        let found = entities(&lines).expect("ner runs");
        let money = found
            .iter()
            .find(|e| e.entity_type == "MONEY")
            .expect("money found");
        assert_eq!(money.text, "$10,450");
        assert!(!money.approximate, "word mapping held");
        assert_eq!(money.word_start, Some(7));
        assert_eq!(money.card, "stat");
        let person = found
            .iter()
            .find(|e| e.entity_type == "PER")
            .expect("person found");
        assert_eq!(person.card, "lower-third");
        assert_eq!(person.start, Some(1.0));
    }

    #[test]
    fn a_full_date_is_found_and_approximate_mapping_survives() {
        let lines = vec![line(
            "It shipped on March 4, 2026 to acclaim.",
            vec![
                WordSpec {
                    start: 1.0,
                    end: 1.2,
                    text: "It".into(),
                },
                WordSpec {
                    start: 1.2,
                    end: 1.5,
                    text: "shipped".into(),
                },
            ],
        )];
        let found = entities(&lines).expect("ner runs");
        let date = found
            .iter()
            .find(|e| e.entity_type == "DATE")
            .expect("a written-out date is a DATE");
        assert_eq!(date.text, "March 4, 2026");
        assert_eq!(date.card, "timeline");
        // The date's words are not in the word list, so the mapping is
        // approximate onto the nearest word - and must still carry a time.
        assert!(date.approximate, "mapping onto the nearest word");
        assert!(date.start.is_some(), "an approximate time is still a time");
    }

    #[test]
    fn empty_lines_are_skipped_silently() {
        let lines = vec![line("   ", vec![])];
        assert!(entities(&lines).expect("ner runs").is_empty());
    }

    #[test]
    fn the_card_map_covers_the_director_artefacts() {
        assert_eq!(card_for("MONEY"), "stat");
        assert_eq!(card_for("DATE"), "timeline");
        assert_eq!(card_for("PER"), "lower-third");
        assert_eq!(card_for("EMAIL"), "redact");
        assert_eq!(card_for("MYSTERY"), "mention");
    }
}
