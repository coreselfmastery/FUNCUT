// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The silence in a recording, as spans of the file.
//!
//! What the voiceover panel's pause removal needs of a take: where the
//! words are not. This module finds those spans by streaming the file's
//! decoded audio - mono, sixteen kilohertz, which is plenty for an
//! energy decision and cheap to decode - and folding it into windows a
//! fiftieth of a second long, never holding more than the one window
//! under construction. An hour-long take costs the same memory as a
//! jingle, the same doctrine as [`crate::peaks`].
//!
//! A window is quiet when its loudest sample sits under [`THRESHOLD`],
//! and a *silence* is a run of quiet windows at least
//! [`MIN_SILENCE_SECONDS`] long: shorter gaps are the rhythm of speech
//! and stay. Each silence is padded by [`PAD_SECONDS`] a side, so a
//! word that begins on a breath is not clipped at its onset, and
//! overlapping pads merge.
//!
//! Two guards keep the answer honest. A take in which no window is loud
//! (a dead microphone, a muted input) is not a take of pauses and
//! yields nothing, as does one whose spans would leave under
//! [`MIN_KEPT_SECONDS`] of speech: both are retakes, not edits.

use std::path::Path;

use crate::error::Result;
use crate::samples::{AudioDecoder, AudioOptions, SampleFormat};

/// The rate silence is measured at.
const RATE: u32 = 16_000;

/// The window the stream is folded into, in seconds. Short enough that
/// a pause's edges land where the ear hears them; long enough that one
/// noisy sample in a quiet room says nothing.
const WINDOW_SECONDS: f64 = 0.02;

/// How loud a window's loudest sample may be and the window still count
/// as quiet: a fiftieth, about -40 dBFS. Speech on any microphone worth
/// recording with peaks several times over it; a room's noise floor sits
/// several times under.
const THRESHOLD: f32 = 0.01;

/// The shortest run of quiet windows that counts as a pause, in seconds.
/// Shorter gaps between words are left alone.
const MIN_SILENCE_SECONDS: f64 = 0.35;

/// The silence kept on each side of what is found, in seconds, so the
/// first and last sounds of a word are not clipped with the pause.
const PAD_SECONDS: f64 = 0.06;

/// The least speech a take must keep for its silences to be worth
/// cutting. A take that would lose almost everything is a retake.
const MIN_KEPT_SECONDS: f64 = 0.2;

/// One silence, `[from, to)` in seconds of the file.
pub type Span = (f64, f64);

/// The silence in `path`'s audio stream - `stream`'s, or the file's
/// first - as spans of the file, padded and merged; see the module docs
/// for what counts and the guards that keep it honest.
pub fn silences(path: &Path, stream: Option<usize>) -> Result<Vec<Span>> {
    let mut decoder = AudioDecoder::open(
        path,
        &AudioOptions {
            rate: RATE,
            channels: 1,
            format: SampleFormat::F32,
            stream,
            ..AudioOptions::default()
        },
    )?;
    let mut finder = Finder::new();
    while let Some(chunk) = decoder.next_f32()? {
        finder.fold(chunk.into_iter());
    }
    Ok(finder.finish())
}

/// A stream of samples folded into the quiet runs of a take. Pure past
/// its input, so the finding is testable without a file.
struct Finder {
    /// The window under construction.
    window: Vec<f32>,
    /// Samples folded so far.
    folded: usize,
    /// Where the run of quiet windows under way began, in seconds, or
    /// `None` while the windows are loud.
    quiet_since: Option<usize>,
    /// The quiet runs found so far, each `[from, to)` in samples.
    quiet: Vec<(usize, usize)>,
    /// Whether any window has been loud.
    any_loud: bool,
}

impl Finder {
    fn new() -> Self {
        Self {
            window: Vec::new(),
            folded: 0,
            quiet_since: None,
            quiet: Vec::new(),
            any_loud: false,
        }
    }

    /// Folds samples in; the windows are decided as they fill.
    fn fold(&mut self, samples: impl Iterator<Item = f32>) {
        let window = (WINDOW_SECONDS * f64::from(RATE)).round() as usize;
        for sample in samples {
            self.window.push(sample);
            if self.window.len() == window {
                self.window_done();
            }
        }
    }

    /// Decides the window under construction and clears it.
    fn window_done(&mut self) {
        let window = std::mem::take(&mut self.window);
        let from = self.folded;
        self.folded += window.len();
        let loud = window
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
            > THRESHOLD;
        if loud {
            self.any_loud = true;
            if let Some(since) = self.quiet_since.take() {
                let length = (self.folded - since) as f64 / f64::from(RATE);
                if length >= MIN_SILENCE_SECONDS {
                    self.quiet.push((since, from));
                }
            }
        } else {
            self.quiet_since.get_or_insert(from);
        }
    }

    /// The spans of the take folded so far: the quiet runs padded,
    /// merged and clamped, or nothing when a guard says the take is not
    /// one to cut. See the module docs.
    fn finish(mut self) -> Vec<Span> {
        // A run still under way at the end of the file is the pause the
        // take trailed off on.
        if let Some(since) = self.quiet_since {
            let length = (self.folded - since) as f64 / f64::from(RATE);
            if length >= MIN_SILENCE_SECONDS {
                self.quiet.push((since, self.folded));
            }
        }
        if !self.any_loud {
            return Vec::new();
        }
        let total = self.folded;
        let at = |samples: usize| samples as f64 / f64::from(RATE);
        let pad = (PAD_SECONDS * f64::from(RATE)).round() as usize;
        let mut spans: Vec<Span> = self
            .quiet
            .iter()
            .map(|&(from, to)| {
                (
                    (at(from.saturating_sub(pad))).max(0.0),
                    at((to + pad).min(total)),
                )
            })
            .collect();
        spans.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut merged: Vec<Span> = Vec::new();
        for (from, to) in spans {
            match merged.last_mut() {
                // Padded neighbours that touch were one pause, or two a
                // breath apart: one span either way.
                Some((_, end)) if from <= *end => *end = (*end).max(to),
                _ => merged.push((from, to)),
            }
        }
        // The guards' second half: what the spans leave must be speech
        // worth the name, or the take is a retake rather than an edit.
        let cut: f64 = merged.iter().map(|(from, to)| to - from).sum();
        if at(total) - cut < MIN_KEPT_SECONDS {
            return Vec::new();
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Folds `shapes` through the finder: each is `(from, to, level)`,
    /// the level every sample between `from` and `to` seconds holds, the
    /// last one's `to` naming the take's length.
    fn find(shapes: &[(f64, f64, f32)]) -> Vec<Span> {
        let seconds = shapes.last().map(|&(to, _, _)| to).unwrap_or(0.0);
        let mut finder = Finder::new();
        let value = |at: f64| {
            shapes
                .iter()
                .find(|&&(from, to, _)| at >= from && at < to)
                .map(|&(_, _, level)| level)
                .unwrap_or(0.0)
        };
        let count = (seconds * f64::from(RATE)).round() as usize;
        let samples: Vec<f32> = (0..count)
            .map(|index| value(index as f64 / f64::from(RATE)))
            .collect();
        finder.fold(samples.into_iter());
        finder.finish()
    }

    /// Two words with a pause between them, and pauses at each end: the
    /// spans are the pauses, padded, and nothing else.
    #[test]
    fn pauses_between_words_are_found_and_padded() {
        // A word, a half-second pause, a word, a full second pause, a
        // word, and a half-second pause to finish.
        let spans = find(&[
            (0.0, 1.0, 0.5),
            (1.5, 2.5, 0.5),
            (3.5, 4.5, 0.5),
            (5.0, 0.0, 0.0),
        ]);
        let near = |found: &[Span], wanted: &[(f64, f64)]| {
            assert_eq!(found.len(), wanted.len(), "{found:?}");
            for ((from, to), (wants_from, wants_to)) in found.iter().zip(wanted) {
                assert!((from - wants_from).abs() < 0.03, "{from} vs {wants_from}");
                assert!((to - wants_to).abs() < 0.03, "{to} vs {wants_to}");
            }
        };
        near(&spans, &[(0.94, 1.56), (2.44, 3.56), (4.44, 5.0)]);
    }

    /// A gap shorter than the least pause is the rhythm of speech, and
    /// stays.
    #[test]
    fn a_short_gap_between_words_stays() {
        let spans = find(&[(0.0, 1.0, 0.5), (1.2, 2.0, 0.5)]);
        assert!(spans.is_empty(), "{spans:?}");
    }

    /// A take with nothing loud in it is not a take of pauses.
    #[test]
    fn a_silent_take_yields_nothing() {
        assert!(find(&[(0.0, 5.0, 0.001)]).is_empty());
        assert!(find(&[(0.0, 5.0, 0.0)]).is_empty());
    }

    /// A take whose pauses would leave a word's worth of speech asks for
    /// a retake, not an edit.
    #[test]
    fn a_take_of_almost_only_silence_yields_nothing() {
        // Ten seconds with one twentieth of a second of speech in them.
        let spans = find(&[(0.0, 0.05, 0.5), (10.0, 0.0, 0.0)]);
        assert!(spans.is_empty(), "{spans:?}");
    }

    /// Padded neighbours that touch merge into one pause.
    #[test]
    fn pauses_a_breath_apart_merge() {
        // A word, a pause, a fiftieth-of-a-second blip - a breath, a
        // mouth noise - and another pause to the end: after padding the
        // two pauses are one.
        let spans = find(&[(0.0, 1.0, 0.5), (1.5, 1.58, 0.5), (3.0, 0.0, 0.0)]);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!((spans[0].0 - 0.94).abs() < 0.03, "{spans:?}");
        assert!((spans[0].1 - 3.0).abs() < 0.03, "{spans:?}");
    }

    /// The spans of a real file, by way of the decoder: the same answer
    /// the pure finder gives, through FFmpeg and back.
    #[test]
    fn a_files_silence_is_its_pure_finding() {
        // A second of tone, half a second of silence, a second of tone:
        // 16-bit mono PCM at the measuring rate, in a plain WAV.
        let rate = RATE;
        let mut samples: Vec<i16> = Vec::new();
        let mut push = |seconds: f64, level: f32| {
            for step in 0..(seconds * f64::from(rate)).round() as usize {
                let phase = step as f32 * 2.0 * std::f32::consts::PI * 220.0 / rate as f32;
                samples.push((level * phase.sin() * i16::MAX as f32) as i16);
            }
        };
        push(1.0, 0.5);
        push(0.5, 0.0);
        push(1.0, 0.5);
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        let data = (samples.len() * 2) as u32;
        wav.extend_from_slice(&(36 + data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data.to_le_bytes());
        for sample in &samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        let path = std::env::temp_dir().join("concat-silence-test.wav");
        std::fs::write(&path, &wav).expect("writes the wav");
        let found = silences(&path, None).expect("reads it back");
        let _ = std::fs::remove_file(&path);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!((found[0].0 - 0.94).abs() < 0.03, "{found:?}");
        assert!((found[0].1 - 1.56).abs() < 0.03, "{found:?}");
    }
}
