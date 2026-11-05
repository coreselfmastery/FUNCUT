// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The speaker ear: who spoke when, built on `polyvoice`.
//!
//! The VAD is polyvoice's `EnergyVad`; the diarization clusters oximedia
//! voice-prints (F0, formants, brightness, quality) with polyvoice's
//! agglomerative cosine clustering. No model files, no network: a
//! voiceprint-heuristic diarization, honest about being one - the output's
//! `method` field says exactly what ran, and `available` is false (with a
//! reason) when a take is too short or too unvoiced to cluster.
//!
//! A speaker change is a scene change: the director cuts to a different
//! angle, colour or lower-third on the `changes` list.

use serde::Serialize;

use crate::{Error, Spec, Take};

/// The VAD's `process` lives on this trait.
use polyvoice::vad::VoiceActivityDetector as _;

/// One stretch of detected speech.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    /// Start in seconds.
    pub start: f64,
    /// End in seconds.
    pub end: f64,
}

/// One speaker turn: a continuous stretch of one speaker.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// Start in seconds.
    pub start: f64,
    /// End in seconds.
    pub end: f64,
    /// The speaker's label (0-based, in order of first appearance).
    pub speaker: u32,
}

/// One change of speaker, at the moment it happens.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// When the change lands, in seconds.
    pub t: f64,
    /// The speaker before the change.
    pub from: u32,
    /// The speaker after the change.
    pub to: u32,
}

/// One speaker found in the take.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerEntry {
    /// The speaker's label.
    pub id: u32,
    /// How much of the take this speaker holds, in seconds.
    pub speech_seconds: f64,
    /// The indices of the transcript lines this speaker holds.
    pub line_indices: Vec<usize>,
}

/// The speaker ear's whole output.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerOutAll {
    /// Whether a diarization was possible at all.
    pub available: bool,
    /// What ran, when something did.
    pub method: String,
    /// Why nothing ran, when nothing did.
    pub reason: Option<String>,
    /// The detected speech spans.
    pub speech: Vec<Span>,
    /// The speaker turns, in time order.
    pub turns: Vec<Turn>,
    /// The speakers, in order of first appearance.
    pub speakers: Vec<SpeakerEntry>,
    /// The changes of speaker, in time order.
    pub changes: Vec<Change>,
    /// The speaker of each line of the transcript (by midpoint overlap);
    /// -1 when a line has no timing or no speech under it.
    pub line_speakers: Vec<i64>,
}

/// The whole-output type the crate's `Output` carries.
impl SpeakerOutAll {
    /// An absent reading with the reason it could not be taken.
    pub fn absent(reason: &str) -> Self {
        Self {
            available: false,
            method: String::new(),
            reason: Some(reason.to_owned()),
            speech: Vec::new(),
            turns: Vec::new(),
            speakers: Vec::new(),
            changes: Vec::new(),
            line_speakers: Vec::new(),
        }
    }
}

/// The length of one diarization window: long enough for a stable
/// voice-print, short enough that a turn is not averaged away.
const WINDOW_SECS: f64 = 2.0;

/// The shortest window worth a voice-print.
const MIN_WINDOW_SECS: f64 = 0.8;

/// Read the speakers of a take.
pub fn analyse(take: &Take, spec: &Spec) -> Result<SpeakerOutAll, Error> {
    let rate = take.sample_rate;
    let frame = spec.vad.frame_size.max(1);
    let mut vad = polyvoice::vad::EnergyVad::new(spec.vad.threshold_db, rate, frame);
    let mut probs = Vec::with_capacity(take.samples.len() / frame);
    // The VAD's frame contract: whole frames only, the trailing partial
    // frame is dropped (it is at most 31 ms of a 16 kHz take).
    for chunk in take.samples.chunks(frame) {
        if chunk.len() == frame {
            let frames = vad
                .process(chunk)
                .map_err(|e| Error::Analysis(format!("vad: {e}")))?;
            probs.extend(frames);
        }
    }
    let frame_secs = frame as f64 / f64::from(rate);
    let speech = speech_spans(
        &probs,
        frame_secs,
        spec.vad.probability,
        spec.vad.min_speech_secs,
        spec.vad.min_silence_secs,
    );
    let line_count = spec.lines.len();
    if speech.is_empty() {
        return Ok(SpeakerOutAll {
            available: false,
            method: "energy-vad".into(),
            reason: Some("no speech detected under the VAD threshold".into()),
            speech,
            turns: Vec::new(),
            speakers: Vec::new(),
            changes: Vec::new(),
            line_speakers: vec![-1; line_count],
        });
    }
    let windows = speech_windows(&speech);
    // One window cannot be clustered and does not need to be: the whole
    // take is one speaker by Occam.
    if windows.len() < 2 {
        let turns = vec![Turn {
            start: speech.first().map(|s| s.start).unwrap_or(0.0),
            end: speech.last().map(|s| s.end).unwrap_or(0.0),
            speaker: 0,
        }];
        return Ok(finish(spec, speech, turns, &windows));
    }
    let rate_f = rate as f32;
    let mut prints: Vec<(Span, Vec<f32>)> = Vec::with_capacity(windows.len());
    for window in &windows {
        let samples = take.slice(window.start, window.end);
        if let Some(print) = voiceprint(samples, rate_f) {
            prints.push((*window, print));
        }
    }
    if prints.len() < 2 {
        let turns = vec![Turn {
            start: speech[0].start,
            end: speech[speech.len() - 1].end,
            speaker: 0,
        }];
        return Ok(finish(spec, speech, turns, &windows));
    }
    // Z-score each feature across the take before clustering: a feature
    // that never varies (formants on a monotone take) carries no speaker
    // information and must not outvote one that does. This is the classic
    // speaker-clustering whitening step, and it is what makes two voices
    // of the same room separate.
    let embeddings = standardise(&prints.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>());
    // Windows whose every feature sat on the take's mean carry no signal
    // at all; they join whichever turn surrounds them rather than
    // founding a cluster of one.
    let mut voiced_windows = Vec::with_capacity(prints.len());
    let mut kept_embeddings = Vec::with_capacity(prints.len());
    for (i, (window, _)) in prints.iter().enumerate() {
        if let Some(embedding) = &embeddings[i] {
            voiced_windows.push((*window, kept_embeddings.len()));
            kept_embeddings.push(embedding.clone());
        }
    }
    if voiced_windows.len() < 2 {
        let turns = vec![Turn {
            start: speech[0].start,
            end: speech[speech.len() - 1].end,
            speaker: 0,
        }];
        return Ok(finish(spec, speech, turns, &windows));
    }
    let raw = polyvoice::ahc::agglomerative_cluster_max_clusters(
        &kept_embeddings,
        spec.speaker_threshold,
        spec.max_speakers.max(1),
    );
    // Relabel in order of first appearance so speaker 0 is the first
    // voice heard, whatever the clustering numbered internally.
    let mut remap = std::collections::HashMap::new();
    let mut next = 0_u32;
    let labels: Vec<u32> = raw
        .iter()
        .map(|l| {
            *remap.entry(*l).or_insert_with(|| {
                let id = next;
                next += 1;
                id
            })
        })
        .collect();
    let mut turns: Vec<Turn> = Vec::new();
    for (window, embed_index) in &voiced_windows {
        let speaker = labels[*embed_index];
        match turns.last_mut() {
            // Same speaker inside 0.6 s: the same turn, gaps and all.
            Some(last) if last.speaker == speaker && window.start - last.end < 0.6 => {
                last.end = window.end;
            }
            _ => turns.push(Turn {
                start: window.start,
                end: window.end,
                speaker,
            }),
        }
    }
    Ok(finish(spec, speech, turns, &windows))
}

/// Convert per-frame speech probabilities into speech spans. A span ends
/// where its last speech frame ended - not where the closing silence was
/// finally declared - so a two-frame blip is not silently grown past the
/// minimum-speech filter by the very silence that closed it.
fn speech_spans(
    probs: &[f32],
    frame_secs: f64,
    threshold: f32,
    min_speech: f64,
    min_silence: f64,
) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut start: Option<f64> = None;
    let mut last_speech_end = 0.0_f64;
    let mut quiet_run = 0_usize;
    for (i, p) in probs.iter().enumerate() {
        let t = i as f64 * frame_secs;
        if *p >= threshold {
            if start.is_none() {
                start = Some(t);
            }
            last_speech_end = t + frame_secs;
            quiet_run = 0;
        } else if start.is_some() {
            quiet_run += 1;
            // A short quiet stretch is a breath inside speech, not a
            // boundary; only a real silence closes the span.
            if quiet_run as f64 * frame_secs >= min_silence {
                if let Some(s) = start.take() {
                    spans.push(Span {
                        start: s,
                        end: last_speech_end,
                    });
                }
                quiet_run = 0;
            }
        }
    }
    if let Some(s) = start {
        spans.push(Span {
            start: s,
            end: last_speech_end,
        });
    }
    spans
        .into_iter()
        .filter(|span| span.end - span.start >= min_speech)
        .collect()
}

/// Split speech spans into diarization windows.
fn speech_windows(speech: &[Span]) -> Vec<Span> {
    let mut windows = Vec::new();
    for span in speech {
        let mut cursor = span.start;
        while span.end - cursor >= MIN_WINDOW_SECS {
            let end = (cursor + WINDOW_SECS).min(span.end);
            windows.push(Span { start: cursor, end });
            cursor = end;
        }
        // A tail too short for its own window joins the previous one when
        // there is one, and is dropped when there is not.
        if span.end - cursor > 0.0 {
            match windows.last_mut() {
                Some(last) if last.end - cursor < 1.0 => last.end = span.end,
                _ => {}
            }
        }
    }
    windows
}

/// Standardise feature rows across the take, then L2-normalise each row.
///
/// A feature only survives standardisation when it moves: a column that
/// never varies carries no speaker information, and - the subtler trap - a
/// column whose variation is a thousandth of its own scale (flatness
/// wobbling between 0.0737 and 0.0739 on a synthetic take) is measurement
/// noise that z-scoring would amplify to full scale and let outvote the
/// real signal. So a column must vary by at least 5% of its own mean
/// before it counts. Rows that end up with no signal at all come back
/// `None`.
fn standardise(rows: &[Vec<f32>]) -> Vec<Option<Vec<f32>>> {
    if rows.is_empty() {
        return Vec::new();
    }
    let width = rows[0].len();
    let mut out = Vec::with_capacity(rows.len());
    let mut means = vec![0.0_f32; width];
    let mut stds = vec![0.0_f32; width];
    let mut usable = vec![false; width];
    for c in 0..width {
        let column: Vec<f32> = rows.iter().map(|r| r[c]).collect();
        let n = column.len() as f32;
        let mean = column.iter().sum::<f32>() / n;
        let variance = column.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
        let std = variance.sqrt();
        means[c] = mean;
        stds[c] = std;
        // The coefficient-of-variation floor: real speaker differences
        // move a feature by tens of percent of itself; measurement noise
        // moves it by fractions of a percent.
        let cv = std / mean.abs().max(1.0e-9);
        usable[c] = std > 1.0e-9 && cv >= 0.05;
    }
    for row in rows {
        let mut standardised: Vec<f32> = (0..width)
            .map(|c| {
                if usable[c] {
                    (row[c] - means[c]) / stds[c]
                } else {
                    0.0
                }
            })
            .collect();
        let norm = standardised.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm <= 1.0e-9 {
            out.push(None);
        } else {
            for value in &mut standardised {
                *value /= norm;
            }
            out.push(Some(standardised));
        }
    }
    out
}

/// One window's voice-print: the features that separate voices, scaled to
/// roughly unit range so no single feature dominates before the
/// standardisation.
///
/// The F0 reads through a three-step ladder: the voice analyzer's own,
/// the pitch tracker's, and finally the prosody ear's autocorrelation -
/// which locks onto tones and clean synthesis the specialised voice
/// trackers decline. Formants fall back from the voice analyzer to the
/// bare formant result; quality (jitter, shimmer) is zeroed when the
/// voice analyzer declines. The window is only refused when nothing
/// found a voice at all and no formants came back.
fn voiceprint(samples: &[f32], rate: f32) -> Option<Vec<f32>> {
    let analyzer = oximedia_audio_analysis::AudioAnalyzer::new(
        oximedia_audio_analysis::AnalysisConfig::default(),
    );
    let result = analyzer.analyze(samples, rate).ok()?;
    let mut f0 = result.voice.as_ref().map(|voice| voice.f0).unwrap_or(0.0);
    if f0 <= 0.0 {
        f0 = result.pitch.mean_f0;
    }
    if f0 <= 0.0 {
        let f64_samples: Vec<f64> = samples.iter().map(|s| f64::from(*s)).collect();
        let pitch = spectral_prosody::feature::estimate_pitch(
            &f64_samples,
            f64::from(rate),
            1024,
            70.0,
            400.0,
        );
        let voiced: Vec<f64> = pitch
            .values
            .iter()
            .filter(|hz| **hz >= 70.0 && **hz <= 400.0)
            .copied()
            .collect();
        if !voiced.is_empty() {
            f0 = (voiced.iter().sum::<f64>() / voiced.len() as f64) as f32;
        }
    }
    let (formants, jitter, shimmer) = match result.voice {
        Some(voice) => {
            let formants = if voice.formants.is_empty() {
                result.formants.formants.clone()
            } else {
                voice.formants
            };
            (formants, voice.jitter, voice.shimmer)
        }
        None => (result.formants.formants.clone(), 0.0, 0.0),
    };
    if f0 <= 0.0 && formants.is_empty() {
        return None;
    }
    let f = |i: usize| formants.get(i).copied().unwrap_or(0.0);
    Some(vec![
        f0 / 200.0,
        f(0) / 500.0,
        f(1) / 1500.0,
        f(2) / 2500.0,
        result.spectral.centroid / 2000.0,
        result.spectral.flatness,
        jitter * 10.0,
        shimmer * 10.0,
    ])
}

/// Assemble the finished reading: speakers, changes and the line map.
fn finish(spec: &Spec, speech: Vec<Span>, turns: Vec<Turn>, windows: &[Span]) -> SpeakerOutAll {
    let mut speakers: Vec<SpeakerEntry> = Vec::new();
    for turn in &turns {
        let secs = turn.end - turn.start;
        match speakers.iter_mut().find(|s| s.id == turn.speaker) {
            Some(found) => found.speech_seconds += secs,
            None => speakers.push(SpeakerEntry {
                id: turn.speaker,
                speech_seconds: secs,
                line_indices: Vec::new(),
            }),
        }
    }
    let mut changes = Vec::new();
    for pair in turns.windows(2) {
        if pair[0].speaker != pair[1].speaker {
            changes.push(Change {
                t: pair[1].start,
                from: pair[0].speaker,
                to: pair[1].speaker,
            });
        }
    }
    let line_speakers: Vec<i64> = spec
        .lines
        .iter()
        .map(|line| {
            let (Some(start), Some(end)) = (line.start, line.end) else {
                return -1;
            };
            // The line belongs to the speaker whose turn holds most of
            // its middle; a line is spoken, not straddled.
            let mid = (start + end) / 2.0;
            turns
                .iter()
                .filter(|t| mid >= t.start && mid <= t.end)
                .map(|t| t.speaker as i64)
                .next()
                .unwrap_or(-1)
        })
        .collect();
    for (index, speaker) in line_speakers.iter().enumerate() {
        if let Some(found) = speakers
            .iter_mut()
            .find(|s| s.id as i64 == *speaker)
            .filter(|_| *speaker >= 0)
        {
            found.line_indices.push(index);
        }
    }
    for speaker in &mut speakers {
        speaker.speech_seconds = (speaker.speech_seconds * 1000.0).round() / 1000.0;
    }
    SpeakerOutAll {
        available: true,
        method: format!(
            "polyvoice EnergyVad + oximedia voice-print + polyvoice AHC (cosine {:.2}, {} windows)",
            spec.speaker_threshold,
            windows.len()
        ),
        reason: None,
        speech,
        turns,
        speakers,
        changes,
        line_speakers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probs(pattern: &[(f32, usize)]) -> Vec<f32> {
        // (probability, frames) runs, laid end to end.
        let mut out = Vec::new();
        for (p, n) in pattern {
            out.extend(std::iter::repeat_n(*p, *n));
        }
        out
    }

    #[test]
    fn speech_spans_merge_breaths_and_drop_blips() {
        // 20 frames on, 3 off (a breath), 20 on, then silence.
        let spans = speech_spans(
            &probs(&[(1.0, 20), (0.0, 3), (1.0, 20)]),
            0.032,
            0.5,
            0.12,
            0.2,
        );
        assert_eq!(spans.len(), 1, "{spans:?}");
        // 2 frames of "speech" is under the 0.12 s minimum.
        let spans = speech_spans(
            &probs(&[(1.0, 2), (0.0, 40), (1.0, 20)]),
            0.032,
            0.5,
            0.12,
            0.2,
        );
        assert_eq!(spans.len(), 1, "{spans:?}");
    }

    #[test]
    fn windows_split_long_speech_and_keep_short_whole() {
        let speech = vec![Span {
            start: 0.0,
            end: 5.0,
        }];
        let windows = speech_windows(&speech);
        assert_eq!(windows.len(), 3, "{windows:?}");
        assert!((windows[0].end - windows[0].start - 2.0).abs() < 1.0e-9);
        let short = vec![Span {
            start: 0.0,
            end: 1.2,
        }];
        assert_eq!(speech_windows(&short).len(), 1);
    }

    #[test]
    fn standardisation_kills_constant_features_and_keeps_the_rest() {
        // Three windows, two voices: f0 separates them, formants are
        // identical (and must not outvote the f0).
        let rows = vec![
            vec![0.55, 1.0, 2.0],
            vec![0.56, 1.0, 2.0],
            vec![1.20, 1.0, 2.0],
        ];
        let out = standardise(&rows);
        let a = out[0].clone().expect("window 0 has signal");
        let b = out[1].clone().expect("window 1 has signal");
        let c = out[2].clone().expect("window 2 has signal");
        // Constant columns standardise to exactly zero.
        assert_eq!(a[1], 0.0);
        assert_eq!(a[2], 0.0);
        // The two same-voice windows are identical; the other voice is
        // their mirror.
        assert!((a[0] - b[0]).abs() < 1.0e-6);
        assert!(a[0] * c[0] < 0.0, "opposite sides of the mean");
        // Two identical rows on every feature carry no signal at all.
        let flat = standardise(&[vec![1.0, 2.0], vec![1.0, 2.0]]);
        assert!(flat.iter().all(|row| row.is_none()));
    }

    /// Synthesise a voice-like tone: a fundamental with decaying harmonics
    /// plus a deterministic whisper of noise, the shape LPC and the pitch
    /// trackers are built for. (A pure sine is the adversarial case - one
    /// spectral line leaves LPC ill-conditioned, and a single frame of VAD
    /// boundary jitter swings the poles - so the test uses the shape real
    /// voices have.)
    fn voice_like(freq: f64, secs: f64, rate: u32, seed: u64) -> Vec<f32> {
        let n = (secs * f64::from(rate)) as usize;
        let mut noise = seed;
        (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let mut sample = 0.0_f64;
                for harmonic in 1..=6_u32 {
                    let h = f64::from(harmonic);
                    sample += (h * freq * std::f64::consts::TAU * t).sin() / h.sqrt();
                }
                // A deterministic LCG whisper, ~40 dB down.
                noise = noise
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let dither = ((noise >> 33) as f64 / 2_147_483_648.0_f64) - 0.5;
                (sample * 0.15 + dither * 0.005) as f32
            })
            .collect()
    }

    #[test]
    fn two_voice_like_tones_are_two_speakers() {
        // Two voices: 110 Hz for 4 s, then 240 Hz for 4 s, a real pause
        // between them, both loud enough for the VAD.
        let rate = 16000_u32;
        let mut samples = voice_like(110.0, 4.0, rate, 7);
        samples.extend(std::iter::repeat_n(0.0_f32, rate as usize));
        samples.extend(voice_like(240.0, 4.0, rate, 99));
        let take = Take {
            samples,
            sample_rate: rate,
            channels: 1,
        };
        let spec = Spec::default();
        let out = analyse(&take, &spec).expect("diarization runs");
        assert!(out.available, "{out:?}");
        assert!(out.speakers.len() >= 2, "speakers {:?}", out.speakers);
        assert_eq!(out.changes.len(), 1, "{:?}", out.changes);
    }

    #[test]
    fn a_silent_take_reports_why() {
        let take = Take {
            samples: vec![0.0; 16000],
            sample_rate: 16000,
            channels: 1,
        };
        let out = analyse(&take, &Spec::default()).expect("runs");
        assert!(!out.available);
        assert!(out.reason.is_some());
    }
}
