// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Captions: a transcript turned into word-timed caption comps.
//!
//! This is the piece no external motion engine can do for FUNCUT, because
//! FUNCUT is the thing holding the transcript. A recognised transcript is a
//! list of words with times; a kinetic caption is a comp whose text node
//! carries `words: [(text, start, end)]`. The whole job between them is
//! **line breaking**: a caption line that runs too long is unreadable, one
//! that breaks mid-clause reads wrong, and one that is on screen for four
//! seconds is not a caption any more.
//!
//! The rules here are the ones broadcast captioning settled on, with the
//! timings measured against the words themselves:
//!
//! - a line ends at a **sentence end**, at `words_per_line` words, at
//!   `max_line` seconds, or at a silence longer than `gap`;
//! - a line starts `lead` before its first word and holds `tail` after its
//!   last, so the words sit inside the line rather than at its edges;
//! - the comp's own times are **relative to the line**, so the same comp
//!   placed at a different second keeps its motion.
//!
//! ```no_run
//! # fn main() {
//! use concat_motion::captions::{self, Captions, Word};
//!
//! let words = vec![
//!     Word::new("Every", 0.20, 0.55),
//!     Word::new("cut", 0.55, 0.85),
//!     Word::new("is", 0.85, 1.00),
//! ];
//! let reel = captions::sequence(&words, &Captions::default(), 1920, 1080, 30.0);
//! assert_eq!(reel.scenes.len(), 1);
//! # }
//! ```

use crate::authoring::Op;
use crate::comp::{Comp, NodeKind};
use crate::presets;
use crate::sequence::{Scene, Sequence};

/// One recognised word and when it is spoken.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    /// The word, as the recogniser wrote it.
    pub text: String,
    /// When it starts, in seconds from the start of the recording.
    pub start: f64,
    /// When it ends.
    pub end: f64,
}

impl Word {
    /// A word with its times.
    pub fn new(text: impl Into<String>, start: f64, end: f64) -> Word {
        Word {
            text: text.into(),
            start,
            end: end.max(start),
        }
    }

    /// Read words from `(text, start, end)` triples, which is the shape a
    /// transcript arrives in.
    pub fn from_triples(triples: &[(String, f64, f64)]) -> Vec<Word> {
        triples
            .iter()
            .map(|(text, start, end)| Word::new(text.clone(), *start, *end))
            .collect()
    }
}

/// How to break a transcript into caption lines.
#[derive(Clone, Debug)]
pub struct Captions {
    /// The preset the lines are made of.
    pub comp: String,
    /// The text node inside it that carries the word timing.
    pub node: String,
    /// Most words on one line. Six reads on a phone; four is comfortable.
    pub words_per_line: usize,
    /// Longest a line may stay on screen, in seconds.
    pub max_line: f64,
    /// Silence that ends a line, in seconds.
    pub gap: f64,
    /// How long before its first word a line appears.
    pub lead: f64,
    /// How long after its last word it holds.
    pub tail: f64,
    /// Shortest a line may be, so a single clipped word is still readable.
    pub min_line: f64,
}

impl Default for Captions {
    fn default() -> Self {
        Captions {
            comp: "funcut.caption.kinetic".into(),
            node: "caption".into(),
            words_per_line: 5,
            max_line: 3.6,
            gap: 0.6,
            lead: 0.08,
            tail: 0.28,
            min_line: 0.9,
        }
    }
}

/// One caption line, laid out and ready to place.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// The words, with times **relative to the line's own start**.
    pub words: Vec<Word>,
    /// Where the line starts on the timeline.
    pub at: f64,
    /// How long it holds.
    pub hold: f64,
    /// The line as one string, for a preview or a log.
    pub text: String,
}

impl Line {
    /// The words as `(text, start, end)` with `None` for the last word, which
    /// holds to the end of the comp.
    fn ops(&self, node: &str) -> Op {
        let last = self.words.len().saturating_sub(1);
        Op::SetWords {
            node: node.into(),
            words: self
                .words
                .iter()
                .enumerate()
                .map(|(index, word)| {
                    (
                        word.text.clone(),
                        word.start,
                        if index == last { None } else { Some(word.end) },
                    )
                })
                .collect(),
        }
    }
}

/// A time to the millisecond, so a document stays readable and a timeline that
/// counts decimal places still accepts it.
fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Break a transcript into caption lines.
///
/// Words are taken in the order given and grouped as the rules above allow;
/// a word whose own span is longer than `max_line` keeps its own line and the
/// line is allowed to run long, because cutting a word in half is worse than
/// holding it.
pub fn lay_out(words: &[Word], caps: &Captions) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut group: Vec<Word> = Vec::new();

    let flush = |group: &mut Vec<Word>, lines: &mut Vec<Line>| {
        if group.is_empty() {
            return;
        }
        let first = group[0].start;
        let last = group[group.len() - 1].end;
        let at = round((first - caps.lead).max(0.0));
        let hold = round(((last + caps.tail) - at).max(caps.min_line));
        let relative: Vec<Word> = group
            .iter()
            .map(|word| Word {
                text: word.text.clone(),
                start: round((word.start - at).max(0.0)),
                end: round((word.end - at).max(0.0)),
            })
            .collect();
        let text = group
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(Line {
            words: relative,
            at,
            hold,
            text,
        });
        group.clear();
    };

    for word in words {
        if let Some(previous) = group.last() {
            let silence = word.start - previous.end;
            let span = word.end - group[0].start;
            let ends_sentence = previous
                .text
                .chars()
                .last()
                .is_some_and(|ch| matches!(ch, '.' | '!' | '?' | '…' | ':' | ';'));
            if silence >= caps.gap || span > caps.max_line || (ends_sentence && !group.is_empty()) {
                flush(&mut group, &mut lines);
            }
        }
        group.push(word.clone());
        if group.len() >= caps.words_per_line {
            flush(&mut group, &mut lines);
        }
    }
    flush(&mut group, &mut lines);

    // Two lines that would overlap are pushed apart rather than stacked: a
    // caption is never on screen twice. A pushed line keeps two things.
    // Its words are re-anchored to where the line landed - they keep their
    // spoken times, and the ones already spoken are lit the moment it
    // appears - because left relative to where the line *would* have
    // started, every word of a pushed line would fire one push late. And
    // its hold shrinks to end where it naturally ends, so the delay the
    // push handed it is not handed on: with the hold left at its natural
    // length each line ends one push later than the one before, and line
    // by line the captions drift ever further behind the speech they are
    // timed to. A line pushed past its natural end - speech the screen
    // could not keep up with - still holds `min_line`, because a caption
    // that flashes is not a caption either.
    for index in 1..lines.len() {
        let previous_end = lines[index - 1].at + lines[index - 1].hold;
        if lines[index].at < previous_end {
            let natural_at = lines[index].at;
            let natural_end = natural_at + lines[index].hold;
            let push = previous_end - natural_at;
            let line = &mut lines[index];
            line.at = previous_end;
            line.hold = round((natural_end - previous_end).max(caps.min_line));
            // Words fully spoken before a pushed line could appear still
            // light the moment the line does - in speaking order, never
            // overlapping the word before them, each with a hair of a span
            // - because an end at or before a start, or a start before the
            // previous word's end, is a comp the engine refuses outright
            // (E411, a word "ends at 0s, not after it starts at 0s").
            let mut floor = 0.0f64;
            for word in &mut line.words {
                word.start = round((word.start - push).max(floor));
                word.end = round((word.end - push).max(word.start + 0.02));
                floor = word.end;
            }
        }
    }
    lines
}

/// Lay a transcript out as a sequence of caption comps.
///
/// Each line is the same preset with its words set: one comp per line, placed
/// where the words are spoken, ready to be rendered as one file or placed on a
/// project's timeline. `source_dir` is left empty - a caption comp carries no
/// assets - so a caller that wants plates underneath can stack them.
pub fn sequence(words: &[Word], caps: &Captions, width: u32, height: u32, fps: f64) -> Sequence {
    let mut reel = Sequence::new(width, height, fps);
    let mut comp = preset(&caps.comp);
    // A caption comp is authored on one canvas and played on another: the
    // kinetic preset is drawn for 1920x1080, and a reel is usually
    // 1080x1920. The words are re-anchored to the frame itself - centred
    // across it, in the lower third a thumb reads from, sized and wrapped
    // to the frame's own width - because the authored absolute position
    // (960, 900) lands near the right edge of a portrait frame and the
    // words clip: the "wrong text placement" every vertical video carried.
    let fit = f64::from(width) / f64::from(comp.width);
    for node in &mut comp.nodes {
        if node.id != caps.node {
            continue;
        }
        if let NodeKind::Text(text) = &mut node.kind {
            text.size = (text.size * fit).round().max(12.0);
            if let Some(max_width) = text.max_width.as_mut() {
                *max_width = f64::from(width) * 0.88;
            } else {
                text.max_width = Some(f64::from(width) * 0.88);
            }
        }
        node.transform.position = Some(crate::comp::Point::new(
            f64::from(width) * 0.5,
            f64::from(height) * 0.75,
        ));
    }
    for line in lay_out(words, caps) {
        let scene = Scene::new(comp.clone())
            .starting_at(line.at)
            .holding(line.hold)
            .with_ops(vec![line.ops(&caps.node)]);
        reel.push(scene);
    }
    reel
}

/// The comp a caption line is made of.
fn preset(id: &str) -> Comp {
    presets::build(id).unwrap_or_else(|| {
        presets::build("funcut.caption.kinetic").expect("the caption preset is part of the library")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(texts: &[(&str, f64, f64)]) -> Vec<Word> {
        texts
            .iter()
            .map(|(text, start, end)| Word::new(*text, *start, *end))
            .collect()
    }

    #[test]
    fn a_long_run_is_broken_into_lines_of_the_right_length() {
        let transcript = words(&[
            ("one", 0.0, 0.3),
            ("two", 0.32, 0.6),
            ("three", 0.62, 0.9),
            ("four", 0.92, 1.2),
            ("five", 1.22, 1.5),
            ("six", 1.52, 1.8),
            ("seven", 1.82, 2.1),
        ]);
        let caps = Captions::default();
        let lines = lay_out(&transcript, &caps);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "one two three four five");
        assert_eq!(lines[1].text, "six seven");
        assert!(
            lines
                .iter()
                .all(|line| line.words.len() <= caps.words_per_line)
        );
    }

    #[test]
    fn a_sentence_end_breaks_the_line_early() {
        let transcript = words(&[("Ready.", 0.0, 0.4), ("Set", 0.45, 0.7), ("go.", 0.75, 1.0)]);
        let lines = lay_out(&transcript, &Captions::default());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Ready.");
        assert_eq!(lines[1].text, "Set go.");
    }

    #[test]
    fn a_silence_breaks_the_line() {
        let transcript = words(&[("before", 0.0, 0.4), ("after", 2.0, 2.4)]);
        let lines = lay_out(&transcript, &Captions::default());
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn the_times_inside_a_line_are_relative_to_where_it_is_placed() {
        let transcript = words(&[("at", 10.0, 10.3), ("ten", 10.3, 10.6)]);
        let lines = lay_out(&transcript, &Captions::default());
        assert_eq!(lines.len(), 1);
        // The line starts a hair before its first word, so the first word is
        // not at zero and the comp's own clock stays honest.
        assert!((lines[0].at - 9.92).abs() < 1e-9, "{}", lines[0].at);
        assert!((lines[0].words[0].start - 0.08).abs() < 1e-9);
        assert!((lines[0].hold - (10.6 + 0.28 - 9.92)).abs() < 1e-9);
    }

    #[test]
    fn lines_never_overlap_and_nothing_is_shorter_than_it_can_be_read() {
        let transcript = words(&[
            ("a", 0.0, 0.05),
            ("b", 0.06, 0.1),
            ("c", 0.11, 0.15),
            ("d", 0.16, 0.2),
            ("e", 0.21, 0.25),
            ("f", 0.26, 0.3),
        ]);
        let caps = Captions {
            words_per_line: 2,
            min_line: 0.9,
            ..Captions::default()
        };
        let lines = lay_out(&transcript, &caps);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.hold >= 0.9));
        for pair in lines.windows(2) {
            assert!(
                pair[1].at >= pair[0].at + pair[0].hold - 1e-9,
                "{:?} overlaps {:?}",
                pair[1],
                pair[0]
            );
        }
    }

    #[test]
    fn the_last_word_of_a_line_holds_to_the_end_of_its_comp() {
        let transcript = words(&[("last", 1.0, 1.4)]);
        let lines = lay_out(&transcript, &Captions::default());
        match lines[0].ops("caption") {
            Op::SetWords { words, .. } => {
                assert_eq!(words.len(), 1);
                assert_eq!(words[0].2, None);
            }
            other => panic!("expected word timing, got {other:?}"),
        }
    }

    #[test]
    fn a_transcript_becomes_comps_that_can_be_rendered_without_editing() {
        let transcript = words(&[
            ("Make", 0.0, 0.4),
            ("it", 0.4, 0.7),
            ("move.", 0.7, 1.1),
            ("Make", 1.3, 1.7),
            ("it", 1.7, 2.0),
            ("yours.", 2.0, 2.5),
        ]);
        let reel = sequence(&transcript, &Captions::default(), 1920, 1080, 30.0);
        assert_eq!(reel.scenes.len(), 2);
        assert!((reel.scenes[0].at.unwrap_or(-1.0) - 0.0).abs() < 1e-9);
        assert!(
            reel.scenes[1].at.unwrap_or(0.0) >= 1.2,
            "the second line follows the first"
        );
        // Every scene carries the word operation and nothing else.
        assert!(reel.scenes.iter().all(|scene| scene.ops.len() == 1));
    }

    #[test]
    fn an_empty_transcript_is_an_empty_sequence_and_not_a_panic() {
        let reel = sequence(&[], &Captions::default(), 1920, 1080, 30.0);
        assert!(reel.scenes.is_empty());
        assert_eq!(reel.duration(), 0.0);
    }

    #[test]
    fn a_word_longer_than_a_line_keeps_its_own_line_and_is_not_cut() {
        let transcript = words(&[("extraordinarily", 0.0, 5.0), ("short", 5.1, 5.4)]);
        let caps = Captions::default();
        let lines = lay_out(&transcript, &caps);
        assert_eq!(lines[0].words.len(), 1);
        assert!(lines[0].hold >= 5.0, "the long word is held, not clipped");
    }

    /// Continuous speech pushes line after line (each line's tail and the
    /// next's lead overlap), and the pushes must not become a drift: a
    /// pushed line ends where it naturally ends and its words keep their
    /// spoken times, so the most a caption ever trails its first word is
    /// the one tail-and-lead the screen owes the line before it - not one
    /// more per line, which line by line would leave the speech behind.
    #[test]
    fn pushed_lines_stay_timed_to_their_speech() {
        // Twelve words of continuous speech, three a second.
        let mut transcript = Vec::new();
        let mut t = 0.0;
        for _ in 0..12 {
            transcript.push(("word", t, t + 0.30));
            t += 0.32;
        }
        let transcript = words(&transcript);
        let caps = Captions::default();
        let lines = lay_out(&transcript, &caps);
        assert!(lines.len() >= 3, "the transcript is broken into lines");

        let ceiling = caps.tail + caps.lead;
        let mut spoken = 0.0;
        let mut lags: Vec<f64> = Vec::new();
        for line in &lines {
            // Where the line's first word is spoken, and where the placed
            // comp will actually fire it.
            let fires = line.at + line.words[0].start;
            let lag = fires - spoken;
            assert!(
                lag <= ceiling + 1e-9,
                "a word fires {lag:.2}s behind its speech, past one tail and lead"
            );
            lags.push(lag);
            spoken += 0.32 * line.words.len() as f64;
        }
        // The first push owes the line before it a tail and a lead; every
        // push after that owes no more than the first did. A lag that grew
        // line over line would be the drift.
        assert!(
            lags[lags.len() - 1] <= lags[1] + 1e-9,
            "the lag grows line over line ({:?}): drift",
            lags
        );
        // And no line ever starts before the one before it ends.
        for pair in lines.windows(2) {
            assert!(pair[1].at >= pair[0].at + pair[0].hold - 1e-9);
        }
    }

    /// The words a pushed line has already missed are lit when it appears,
    /// not replayed from its own start.
    #[test]
    fn words_already_spoken_before_a_pushed_line_are_lit_on_arrival() {
        let transcript = words(&[
            ("a", 0.0, 0.05),
            ("b", 0.06, 0.1),
            ("c", 0.11, 0.15),
            ("d", 0.16, 0.2),
        ]);
        let caps = Captions {
            words_per_line: 2,
            min_line: 0.9,
            ..Captions::default()
        };
        let lines = lay_out(&transcript, &caps);
        // The second line is pushed to where the first ends, which is past
        // both its words: they arrive already said.
        let second = &lines[1];
        assert!(second.at > 0.2, "the line is pushed past its words");
        // They arrive already said: each lights on arrival, in speaking
        // order, the second a hair after the first ends - never
        // overlapping, never an end at or before a start, either of which
        // is a comp the engine refuses outright.
        assert_eq!(second.words[0].start, 0.0);
        assert!(second.words[1].start >= second.words[0].end);
        assert!(second.words[1].end > second.words[1].start);
    }

    #[test]
    fn an_unknown_caption_preset_falls_back_rather_than_panicking() {
        let caps = Captions {
            comp: "funcut.nope".into(),
            ..Captions::default()
        };
        let reel = sequence(&words(&[("hi", 0.0, 0.3)]), &caps, 1280, 720, 30.0);
        assert_eq!(reel.scenes.len(), 1);
        assert_eq!(reel.scenes[0].comp.id, "funcut.caption.kinetic");
    }
}
