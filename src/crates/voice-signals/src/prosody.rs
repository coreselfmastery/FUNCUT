// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The prosody ear: how the delivery rises, falls, stresses and rolls,
//! built on `spectral-prosody`.
//!
//! Per line this reads the pitch contour (autocorrelation, 70-400 Hz),
//! its range in semitones, its slope and whether the line *ends rising* -
//! the acoustic cue of a question - plus the energy envelope and the words
//! its peaks land on, which is where the caption's karaoke highlight and
//! the SFX peak belong. Over the whole take it builds the prosody graph
//! (one node per 64 ms frame: time, RMS energy, pitch, brightness) and
//! reads the rhythm layers and phrase boundaries out of the graph
//! Laplacian's spectrum.

use serde::Serialize;
use spectral_prosody::feature::{ProsodyFeature, estimate_pitch};
use spectral_prosody::phrase::PhraseSegmenter;
use spectral_prosody::prosody::{ProsodyGraph, ProsodyNode};
use spectral_prosody::rhythm::RhythmExtractor;

use crate::{LineSpec, Take};

/// Twelve over the natural log of two: the Hz-to-semitones conversion
/// factor.
const SEMITONES_PER_OCTAVE_PER_LN_HZ: f64 = 12.0 / std::f64::consts::LN_2;

/// The frame every contour is computed over, in samples. 1024 at 16 kHz is
/// 64 ms: long enough for a 70 Hz period (143 ms is two periods of the
/// lowest voice), short enough that a syllable is more than one frame.
const FRAME: usize = 1024;

/// Hard ceiling on prosody-graph nodes so an hour-long take cannot ask the
/// kNN builder for billions of distance computations; longer takes are
/// analysed on evenly thinned frames (one node covers proportionally more
/// time, the rhythm layers survive, the per-line contours are unaffected).
const MAX_NODES: usize = 3600;

/// The prosody reading of one line.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LineProsody {
    /// Mean voiced pitch in Hz.
    pub pitch_mean_hz: Option<f64>,
    /// Lowest voiced pitch in Hz.
    pub pitch_min_hz: Option<f64>,
    /// Highest voiced pitch in Hz.
    pub pitch_max_hz: Option<f64>,
    /// The voiced range in semitones.
    pub pitch_range_semitones: Option<f64>,
    /// The contour's slope in semitones per second (positive = rising).
    pub pitch_slope_semitones_per_sec: Option<f64>,
    /// Whether the last voiced quarter sits at least 1.5 semitones over
    /// the line's middle: the question cue.
    pub final_rise: bool,
    /// The share of frames the pitch tracker found a voice in.
    pub voiced_ratio: f64,
    /// Mean RMS energy, as dBFS.
    pub energy_mean_dbfs: f64,
    /// The words the energy peaks land on: where the emphasis is.
    pub emphasis_words: Vec<EmphasisWord>,
}

/// One stressed word, located by an energy peak.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EmphasisWord {
    /// The peak's time in seconds.
    pub t: f64,
    /// How many standard deviations the peak sits over the line's mean
    /// energy.
    pub strength: f64,
    /// The word's index in the line's own word list, when the peak lands
    /// inside one.
    pub word_index: Option<usize>,
    /// The word itself, when the peak lands inside one.
    pub word_text: Option<String>,
}

/// One rhythm layer of the take.
#[derive(Serialize, Clone, Debug)]
pub struct RhythmLayerOut {
    /// The layer's eigenvalue in the graph Laplacian's spectrum.
    pub eigenvalue: f64,
    /// The layer's period in seconds (macro rhythm at the top).
    pub period_sec: f64,
    /// The layer's relative strength.
    pub strength: f64,
}

/// One phrase boundary the Fiedler vector found.
#[derive(Serialize, Clone, Debug)]
pub struct PhraseOut {
    /// The phrase's start in seconds.
    pub start_sec: f64,
    /// The phrase's end in seconds.
    pub end_sec: f64,
    /// The segmenter's own label.
    pub label: String,
    /// The phrase's average energy.
    pub average_energy: f64,
}

/// The whole take's prosody.
#[derive(Serialize, Clone, Debug)]
pub struct ProsodyOut {
    /// Per-line readings, index-aligned with the spec's lines (the entry
    /// is `None` for a line with no usable timing).
    pub lines: Vec<Option<LineProsody>>,
    /// The rhythm layers, strongest first.
    pub rhythm_layers: Vec<RhythmLayerOut>,
    /// The strongest layer's period in seconds: the take's tempo.
    pub dominant_period_sec: Option<f64>,
    /// The phrase boundaries.
    pub phrases: Vec<PhraseOut>,
    /// Why the reading is absent, when it is.
    pub note: Option<String>,
}

impl ProsodyOut {
    /// An absent reading with the reason it could not be taken.
    pub fn absent(reason: &str) -> Self {
        Self {
            lines: Vec::new(),
            rhythm_layers: Vec::new(),
            dominant_period_sec: None,
            phrases: Vec::new(),
            note: Some(reason.to_owned()),
        }
    }
}

/// Read the prosody of a take: per-line contours and emphasis, plus the
/// whole take's rhythm and phrases.
pub fn analyse(take: &Take, timed: &[(usize, &LineSpec)]) -> ProsodyOut {
    let rate = f64::from(take.sample_rate);
    let frame_secs = FRAME as f64 / rate;
    // `timed` is the filtered subset of the spec's lines that carry a
    // usable span; readings land at their own line index so the caller's
    // alignment survives an untimed line in the middle.
    let mut by_index = std::collections::HashMap::new();
    for (index, line) in timed {
        let start = line.start.unwrap_or(0.0);
        let end = line.end.unwrap_or(0.0);
        let reading = if end - start >= 2.0 * frame_secs {
            Some(line_prosody(take.slice(start, end), rate, start, line))
        } else {
            None
        };
        by_index.insert(*index, reading);
    }
    let line_count = timed.iter().map(|(index, _)| *index + 1).max().unwrap_or(0);
    let lines = (0..line_count)
        .map(|i| by_index.get(&i).cloned().flatten())
        .collect();
    let (rhythm_layers, dominant, phrases) = whole_take(take, rate);
    ProsodyOut {
        lines,
        rhythm_layers,
        dominant_period_sec: dominant,
        phrases,
        note: None,
    }
}

/// The reading of one line's slice.
fn line_prosody(samples: &[f32], rate: f64, offset: f64, line: &LineSpec) -> LineProsody {
    let f64_samples: Vec<f64> = samples.iter().map(|s| f64::from(*s)).collect();
    let pitch = estimate_pitch(&f64_samples, rate, FRAME, 70.0, 400.0);
    let energy = ProsodyFeature::from_energy(&f64_samples, rate, FRAME);
    let voiced: Vec<(f64, f64)> = pitch
        .values
        .iter()
        .enumerate()
        .filter(|(_, hz)| **hz >= 70.0 && **hz <= 400.0)
        .map(|(i, hz)| (i as f64 * frame_time(rate), *hz))
        .collect();
    let pitch_mean = mean(voiced.iter().map(|(_, hz)| *hz));
    let pitch_min = voiced.iter().map(|(_, hz)| *hz).fold(None::<f64>, |a, h| {
        Some(match a {
            Some(lo) if lo < h => lo,
            _ => h,
        })
    });
    let pitch_max = voiced.iter().map(|(_, hz)| *hz).fold(None::<f64>, |a, h| {
        Some(match a {
            Some(hi) if hi > h => hi,
            _ => h,
        })
    });
    let range_semitones = match (pitch_min, pitch_max) {
        (Some(lo), Some(hi)) if hi > lo => Some(SEMITONES_PER_OCTAVE_PER_LN_HZ * (hi / lo).ln()),
        _ => None,
    };
    let slope = slope_semitones(&voiced);
    let final_rise = final_rise(&voiced);
    let voiced_ratio = if pitch.values.is_empty() {
        0.0
    } else {
        voiced.len() as f64 / pitch.values.len() as f64
    };
    let energy_mean = energy.mean();
    let energy_dbfs = 20.0 * (energy_mean + 1.0e-12).log10();
    let peaks = energy_peaks(
        &energy.values,
        frame_time(rate),
        energy_mean,
        energy.std_dev(),
    );
    let emphasis_words = peaks
        .into_iter()
        .map(|(t, strength)| {
            let at = t + offset;
            let hit = word_at(line, at);
            let (word_index, word_text) = match hit {
                Some((i, w)) => (Some(i), Some(w)),
                None => (None, None),
            };
            EmphasisWord {
                t: (at * 1000.0).round() / 1000.0,
                strength: (strength * 100.0).round() / 100.0,
                word_index,
                word_text,
            }
        })
        .collect();
    LineProsody {
        pitch_mean_hz: round_opt(pitch_mean),
        pitch_min_hz: round_opt(pitch_min),
        pitch_max_hz: round_opt(pitch_max),
        pitch_range_semitones: round_opt(range_semitones),
        pitch_slope_semitones_per_sec: round_opt(slope),
        final_rise,
        voiced_ratio: (voiced_ratio * 1000.0).round() / 1000.0,
        energy_mean_dbfs: (energy_dbfs * 100.0).round() / 100.0,
        emphasis_words,
    }
}

/// The frame duration in seconds.
fn frame_time(rate: f64) -> f64 {
    FRAME as f64 / rate
}

/// A frame's brightness, as a zero-crossing rate in crossings per sample:
/// a cheap stand-in for spectral centroid that keeps every prosody node
/// meaningful without a second FFT per frame.
fn brightness(frame: &[f64]) -> f64 {
    if frame.len() < 2 {
        return 0.0;
    }
    let mut crossings = 0_usize;
    for pair in frame.windows(2) {
        if (pair[0] < 0.0 && pair[1] >= 0.0) || (pair[0] >= 0.0 && pair[1] < 0.0) {
            crossings += 1;
        }
    }
    crossings as f64 / frame.len() as f64
}

/// The take's rhythm layers and phrases from the prosody graph.
fn whole_take(take: &Take, rate: f64) -> (Vec<RhythmLayerOut>, Option<f64>, Vec<PhraseOut>) {
    let f64_samples: Vec<f64> = take.samples.iter().map(|s| f64::from(*s)).collect();
    if f64_samples.len() < FRAME * 4 {
        return (Vec::new(), None, Vec::new());
    }
    let pitch = estimate_pitch(&f64_samples, rate, FRAME, 70.0, 400.0);
    let energy = ProsodyFeature::from_energy(&f64_samples, rate, FRAME);
    let frames: Vec<&[f64]> = f64_samples.chunks(FRAME).collect();
    let total = energy.values.len();
    let thin = if total > MAX_NODES {
        total.div_ceil(MAX_NODES)
    } else {
        1
    };
    let mut nodes = Vec::with_capacity(total / thin + 1);
    for i in 0..total {
        // Thin evenly but keep the frames' own time stamps, so the graph
        // still knows where in the take each node sits.
        if i % thin != 0 {
            continue;
        }
        let frame = frames.get(i).copied().unwrap_or(&[]);
        let hz = pitch.values.get(i).copied().unwrap_or(0.0);
        nodes.push(ProsodyNode::new(
            i as f64 * frame_time(rate),
            energy.values.get(i).copied().unwrap_or(0.0),
            if (70.0..=400.0).contains(&hz) {
                hz
            } else {
                0.0
            },
            frame_time(rate) * thin as f64,
            brightness(frame),
        ));
    }
    if nodes.len() < 8 {
        return (Vec::new(), None, Vec::new());
    }
    // Sigma: the median gap between kept nodes, so thinning a long take
    // widens the proximity kernel instead of shredding the graph.
    let mut gaps: Vec<f64> = nodes
        .windows(2)
        .map(|pair| pair[1].time - pair[0].time)
        .collect();
    gaps.sort_by(|a, b| a.total_cmp(b));
    let sigma = gaps.get(gaps.len() / 2).copied().unwrap_or(0.5).max(1.0e-3);
    let Some(graph) = ProsodyGraph::build_knn(nodes, 4, sigma).ok() else {
        return (Vec::new(), None, Vec::new());
    };
    let mut layers: Vec<RhythmLayerOut> = RhythmExtractor::new(6)
        .extract(&graph)
        .unwrap_or_default()
        .into_iter()
        .map(|layer| RhythmLayerOut {
            eigenvalue: round5(layer.eigenvalue),
            period_sec: round5(layer.period),
            strength: round5(layer.strength),
        })
        .collect();
    layers.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    let dominant = layers.first().map(|l| l.period_sec).filter(|p| *p > 0.0);
    let phrases = PhraseSegmenter::new(6)
        .segment(&graph)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|phrase| {
            let start = graph.nodes.get(phrase.start_idx).map(|n| n.time)?;
            let end = graph
                .nodes
                .get(phrase.end_idx.min(graph.nodes.len().saturating_sub(1)))
                .map(|n| n.time + frame_time(rate))?;
            Some(PhraseOut {
                start_sec: round5(start),
                end_sec: round5(end),
                label: phrase.label,
                average_energy: round5(phrase.average_energy),
            })
        })
        .collect();
    (layers, dominant, phrases)
}

/// The energy peaks of a line: frames more than one standard deviation
/// over the mean that also beat both neighbours.
fn energy_peaks(values: &[f64], step: f64, mean_energy: f64, std: f64) -> Vec<(f64, f64)> {
    let mut peaks = Vec::new();
    if std <= 1.0e-9 || values.len() < 3 {
        return peaks;
    }
    for i in 1..values.len() - 1 {
        let v = values[i];
        if v > mean_energy + std && v >= values[i - 1] && v > values[i + 1] {
            // A peak already found within 120 ms is the same stress: keep
            // the taller one.
            let t = i as f64 * step;
            if let Some(last) = peaks.last_mut() {
                if t - last.0 < 0.12 && v > values[(last.0 / step) as usize] {
                    *last = (t, (v - mean_energy) / std);
                }
                if t - last.0 < 0.12 {
                    continue;
                }
            }
            peaks.push((t, (v - mean_energy) / std));
        }
    }
    peaks
}

/// The word whose span contains `t`, if any.
fn word_at(line: &LineSpec, t: f64) -> Option<(usize, String)> {
    line.words
        .iter()
        .position(|w| t >= w.start && t <= w.end)
        .map(|i| (i, line.words[i].text.clone()))
}

/// The mean of an iterator of values, `None` when empty.
fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut count = 0_usize;
    let mut sum = 0.0;
    for value in values {
        sum += value;
        count += 1;
    }
    if count == 0 {
        None
    } else {
        Some(sum / count as f64)
    }
}

/// A least-squares slope over (time, Hz), converted to semitones per
/// second so a "rising delivery" is a number a director can compare
/// against a rule.
fn slope_semitones(voiced: &[(f64, f64)]) -> Option<f64> {
    if voiced.len() < 3 {
        return None;
    }
    let n = voiced.len() as f64;
    let t_mean = voiced.iter().map(|(t, _)| *t).sum::<f64>() / n;
    let h_mean = voiced.iter().map(|(_, h)| *h).sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (t, h) in voiced {
        num += (t - t_mean) * (h - h_mean);
        den += (t - t_mean) * (t - t_mean);
    }
    if den <= 1.0e-12 || h_mean <= 0.0 {
        return None;
    }
    // d(semitone)/dt = dHz/dt * 12 / (Hz * ln 2)
    Some((num / den) * SEMITONES_PER_OCTAVE_PER_LN_HZ / h_mean)
}

/// Whether the line's last voiced quarter sits at least 1.5 semitones
/// over its middle half: the terminal rise that marks a question.
fn final_rise(voiced: &[(f64, f64)]) -> bool {
    if voiced.len() < 4 {
        return false;
    }
    let last = &voiced[(voiced.len() * 3) / 4..];
    let middle = &voiced[voiced.len() / 4..(voiced.len() * 3) / 4];
    let (Some(last_mean), Some(middle_mean)) = (
        mean(last.iter().map(|(_, h)| *h)),
        mean(middle.iter().map(|(_, h)| *h)),
    ) else {
        return false;
    };
    if last_mean <= 0.0 || middle_mean <= 0.0 {
        return false;
    }
    SEMITONES_PER_OCTAVE_PER_LN_HZ * (last_mean / middle_mean).ln() >= 1.5
}

/// Round an optional value to five decimals for a stable output.
fn round_opt(value: Option<f64>) -> Option<f64> {
    value.map(round5)
}

/// Round to five decimals.
fn round5(value: f64) -> f64 {
    (value * 100_000.0).round() / 100_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn take_with(freq: f64, secs: f64, rate: u32) -> Take {
        let n = (secs * f64::from(rate)) as usize;
        Take {
            samples: (0..n)
                .map(|i| ((freq * TAU * i as f64) / f64::from(rate)).sin() as f32 * 0.4)
                .collect(),
            sample_rate: rate,
            channels: 1,
        }
    }

    #[test]
    fn a_steady_tone_reads_its_own_pitch() {
        let take = take_with(200.0, 1.0, 16000);
        let line = LineSpec {
            start: Some(0.0),
            end: Some(1.0),
            text: "a tone".into(),
            words: vec![crate::WordSpec {
                start: 0.0,
                end: 1.0,
                text: "a tone".into(),
            }],
        };
        let reading = line_prosody(&take.samples, f64::from(take.sample_rate), 0.0, &line);
        let mean = reading.pitch_mean_hz.expect("a voiced tone has pitch");
        assert!((190.0..=210.0).contains(&mean), "pitch {mean}");
        assert!(reading.voiced_ratio > 0.9, "ratio {}", reading.voiced_ratio);
    }

    #[test]
    fn a_rising_tone_has_a_positive_slope_and_a_terminal_rise() {
        let rate = 16000.0;
        let n = 32000;
        let samples: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / rate;
                let hz = 120.0 + 100.0 * t;
                (hz * TAU * t).sin() * 0.4
            })
            .collect();
        let voiced: Vec<(f64, f64)> = samples
            .chunks(FRAME)
            .enumerate()
            .map(|(i, _frame)| {
                let hz = 120.0 + 100.0 * (i as f64 * FRAME as f64 / rate + 0.032);
                (i as f64 * FRAME as f64 / rate, hz)
            })
            .collect();
        let slope = slope_semitones(&voiced).expect("a chirp has a slope");
        assert!(slope > 5.0, "slope {slope}");
        assert!(final_rise(&voiced), "a chirp ends high");
    }

    #[test]
    fn brightness_counts_crossings() {
        let flat = vec![0.5; 64];
        assert_eq!(brightness(&flat), 0.0);
        let alternating: Vec<f64> = (0..64).map(|i| f64::from((i % 2) - 1) * 0.5).collect();
        assert!(brightness(&alternating) > 0.9);
    }

    #[test]
    fn peaks_sit_on_local_maxima_above_one_sigma() {
        let mut values = vec![0.1; 200];
        values[50] = 0.9;
        values[120] = 0.8;
        let mean_energy = 0.1;
        let std = 0.1;
        let peaks = energy_peaks(&values, 0.064, mean_energy, std);
        assert_eq!(peaks.len(), 2, "{peaks:?}");
        assert!((peaks[0].0 - 50.0 * 0.064).abs() < 1.0e-9);
    }

    #[test]
    fn the_whole_take_graph_answers_a_tone() {
        let take = take_with(180.0, 4.0, 16000);
        let (layers, dominant, phrases) = whole_take(&take, f64::from(take.sample_rate));
        assert!(!layers.is_empty(), "a four-second tone has rhythm layers");
        assert!(dominant.unwrap_or(0.0) > 0.0, "a dominant period exists");
        assert!(!phrases.is_empty(), "phrases come back");
    }

    #[test]
    fn an_untimed_line_gets_no_reading_but_breaks_nothing() {
        let take = take_with(200.0, 0.5, 16000);
        let out = analyse(&take, &[]);
        assert!(out.lines.is_empty());
        assert!(out.note.is_none());
    }
}
