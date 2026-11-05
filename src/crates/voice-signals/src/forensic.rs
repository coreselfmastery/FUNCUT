// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The forensic ear: what the recording itself is made of, built on
//! `oximedia-audio-analysis`.
//!
//! Per line it reads the formants (F1-F4, LPC) and the voice quality
//! (F0, jitter, shimmer, harmonics-to-noise) - the fingerprint that tells
//! two TTS regenerations of the same line apart. Over the whole take it
//! reads the noise profile (type, floor, level), the signal-to-noise
//! ratio, the spectral flux (a second opinion on emphasis and onsets),
//! the dynamics (crest factor, dynamic range) and the RMS envelope.
//!
//! The noise floor is the BGM decision: a dry voiceover sits on a floor
//! tens of dB down and wants music under it; a take that already carries
//! music does not, and adding more only muds the mix.

use serde::Serialize;

use crate::{Error, LineSpec, Take};

/// The forensic reading of one line.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LineForensic {
    /// The formant frequencies F1-F4 in Hz, when the line is long enough
    /// and voiced enough for LPC to mean anything.
    pub formants_hz: Option<Vec<f32>>,
    /// The line's fundamental, from the voice analyzer.
    pub f0_hz: Option<f32>,
    /// Pitch-period variation (0-1): a hoarse or synthetic read is high.
    pub jitter: Option<f32>,
    /// Amplitude variation (0-1).
    pub shimmer: Option<f32>,
    /// Harmonics-to-noise ratio in dB: the voice's cleanliness.
    pub hnr_db: Option<f32>,
}

/// The whole take's forensic reading.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WholeForensic {
    /// The signal-to-noise ratio in dB.
    pub snr_db: f32,
    /// The noise floor as dBFS (20 log10 of the profiler's floor).
    pub noise_floor_dbfs: f32,
    /// The profiler's noise type label.
    pub noise_type: String,
    /// The noise level (RMS, linear).
    pub noise_level: f32,
    /// The noise's spectral flatness (0-1; 1 is white).
    pub spectral_flatness: f32,
    /// The spectrum's centroid in Hz: the take's brightness.
    pub spectral_centroid_hz: f32,
    /// The mean spectral flux: how fast the timbre moves.
    pub flux_mean: f32,
    /// The peak spectral flux.
    pub flux_peak: f32,
    /// The onset times spectral flux found, in seconds.
    pub flux_onsets_sec: Vec<f32>,
    /// The crest factor (peak-to-RMS).
    pub crest: f32,
    /// The dynamic range in dB.
    pub dynamic_range_db: f32,
    /// The mean F0 the YIN tracker found over voiced frames.
    pub mean_f0_hz: f32,
    /// The share of voiced frames.
    pub voicing_rate: f32,
    /// The RMS envelope, at most 256 points: the shape the BGM duck
    /// follows.
    pub rms_envelope: Vec<f32>,
}

/// The forensic ear's whole output.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ForensicOut {
    /// Per-line readings, index-aligned with the spec's lines.
    pub lines: Vec<Option<LineForensic>>,
    /// The whole take's reading.
    pub whole: Option<WholeForensic>,
    /// Why the reading is absent, when it is.
    pub note: Option<String>,
}

impl ForensicOut {
    /// An absent reading with the reason it could not be taken.
    pub fn absent(reason: &str) -> Self {
        Self {
            lines: Vec::new(),
            whole: None,
            note: Some(reason.to_owned()),
        }
    }
}

/// The shortest slice the voice analyzer accepts without producing
/// noise: two FFT frames of the default 2048-point config.
const MIN_VOICE_SLICE: f64 = 0.25;

/// Read the forensics of a take.
pub fn analyse(
    take: &Take,
    timed: &[(usize, &LineSpec)],
    max_line_secs: f64,
) -> Result<ForensicOut, Error> {
    let rate = take.sample_rate as f32;
    let mut by_index = std::collections::HashMap::new();
    for (index, line) in timed {
        let start = line.start.unwrap_or(0.0);
        let end = line.end.unwrap_or(0.0);
        let span = end - start;
        if span < MIN_VOICE_SLICE || span > max_line_secs {
            continue;
        }
        by_index.insert(*index, line_forensic(take.slice(start, end), rate));
    }
    let line_count = timed.iter().map(|(index, _)| *index + 1).max().unwrap_or(0);
    let lines = (0..line_count).map(|i| by_index.get(&i).cloned()).collect();
    let whole = whole_forensic(take)?;
    Ok(ForensicOut {
        lines,
        whole: Some(whole),
        note: None,
    })
}

/// One line's reading. `None` fields are honest absences: the voice
/// analyzer returns no voice characteristics for a slice it cannot find
/// a periodic signal in, and that is reported rather than guessed.
fn line_forensic(samples: &[f32], rate: f32) -> LineForensic {
    let analyzer = oximedia_audio_analysis::AudioAnalyzer::new(default_config());
    let Ok(result) = analyzer.analyze(samples, rate) else {
        return LineForensic {
            formants_hz: None,
            f0_hz: None,
            jitter: None,
            shimmer: None,
            hnr_db: None,
        };
    };
    let formants = if result.formants.formants.is_empty() {
        None
    } else {
        Some(result.formants.formants.clone())
    };
    let voice = result.voice.map(|v| {
        let f0 = if v.f0 > 0.0 { Some(v.f0) } else { None };
        (f0, v.formants, v.jitter, v.shimmer, v.hnr)
    });
    // Prefer the voice analyzer's own formants when it found a voice; the
    // bare formant result is the fallback for a voiced-but-quiet line.
    let (f0, formants, jitter, shimmer, hnr) = match voice {
        Some((f0, vf, jitter, shimmer, hnr)) => {
            let formants = if vf.is_empty() { formants } else { Some(vf) };
            (f0, formants, Some(jitter), Some(shimmer), Some(hnr))
        }
        None => (None, formants, None, None, None),
    };
    LineForensic {
        formants_hz: formants,
        f0_hz: f0,
        jitter,
        shimmer,
        hnr_db: hnr,
    }
}

/// The whole take's reading.
fn whole_forensic(take: &Take) -> Result<WholeForensic, Error> {
    let rate = take.sample_rate as f32;
    let samples = &take.samples;
    if samples.is_empty() {
        return Err(Error::Analysis("the take decodes to no samples".into()));
    }
    let analyzer = oximedia_audio_analysis::AudioAnalyzer::new(default_config());
    let result = analyzer
        .analyze(samples, rate)
        .map_err(|e| Error::Analysis(format!("whole-take analysis: {e}")))?;
    let profiler = oximedia_audio_analysis::noise::profile::NoiseProfiler::new(default_config());
    let profile = profiler
        .profile(samples, rate)
        .map_err(|e| Error::Analysis(format!("noise profile: {e}")))?;
    let snr = oximedia_audio_analysis::noise::snr::estimate_snr_db(samples);
    let flux = oximedia_audio_analysis::spectral_flux::SpectralFluxDetector::new(
        rate,
        1024,
        256,
        oximedia_audio_analysis::spectral_flux::FluxSensitivity::Medium,
    )
    .detect(samples);
    let mut envelope = result.dynamics.rms_over_time.clone();
    if envelope.len() > 256 {
        let step = envelope.len() as f32 / 256.0;
        envelope = (0..256)
            .map(|i| {
                let index = ((i as f32 + 0.5) * step) as usize;
                envelope[index.min(envelope.len() - 1)]
            })
            .collect();
    }
    Ok(WholeForensic {
        snr_db: round3(snr),
        noise_floor_dbfs: round3(20.0 * (profile.noise_floor + 1.0e-12).log10()),
        noise_type: format!("{:?}", profile.noise_type),
        noise_level: round3(profile.noise_level),
        spectral_flatness: round3(profile.spectral_flatness),
        spectral_centroid_hz: round3(result.spectral.centroid),
        flux_mean: round3(flux.mean_flux),
        flux_peak: round3(flux.peak_flux),
        flux_onsets_sec: flux.onsets.iter().map(|t| round3(*t)).collect(),
        crest: round3(result.dynamics.crest),
        dynamic_range_db: round3(result.dynamics.dynamic_range_db),
        mean_f0_hz: round3(result.pitch.mean_f0),
        voicing_rate: round3(result.pitch.voicing_rate),
        rms_envelope: envelope.iter().map(|v| round5(*v)).collect(),
    })
}

/// The analysis configuration every analyzer here shares.
fn default_config() -> oximedia_audio_analysis::AnalysisConfig {
    oximedia_audio_analysis::AnalysisConfig::default()
}

/// Round to three decimals.
fn round3(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}

/// Round to five decimals.
fn round5(value: f32) -> f32 {
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
    fn a_tone_has_a_low_noise_floor_and_a_clean_hnr_path() {
        let take = take_with(200.0, 2.0, 16000);
        let whole = whole_forensic(&take).expect("a two-second tone analyses");
        assert!(whole.snr_db > 5.0, "snr {}", whole.snr_db);
        assert!(
            whole.noise_floor_dbfs < -20.0,
            "floor {}",
            whole.noise_floor_dbfs
        );
        assert!(!whole.rms_envelope.is_empty());
        assert!(whole.crest > 0.5, "crest {}", whole.crest);
    }

    #[test]
    fn a_line_slice_gets_its_formants() {
        let take = take_with(150.0, 1.0, 16000);
        let reading = line_forensic(&take.samples[..16000.min(take.samples.len())], 16000.0);
        assert!(
            reading.formants_hz.as_ref().is_some_and(|f| !f.is_empty()),
            "formants {reading:?}"
        );
    }

    #[test]
    fn an_empty_take_is_an_analysis_error_not_a_crash() {
        let take = Take {
            samples: Vec::new(),
            sample_rate: 16000,
            channels: 1,
        };
        assert!(whole_forensic(&take).is_err());
    }

    #[test]
    fn the_envelope_is_capped_at_256_points() {
        let take = take_with(200.0, 30.0, 16000);
        let whole = whole_forensic(&take).expect("analyses");
        assert!(
            whole.rms_envelope.len() <= 256,
            "{}",
            whole.rms_envelope.len()
        );
        assert!(
            whole.rms_envelope.len() >= 64,
            "{}",
            whole.rms_envelope.len()
        );
    }
}
