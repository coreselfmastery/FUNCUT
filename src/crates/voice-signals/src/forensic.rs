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
//! the dynamics (crest factor, dynamic range) and the RMS envelope, plus
//! the safety and identity readings: the sample peak, the clipping
//! ratio, the DC offset, the mains hum, the sibilance share, the spectral
//! rolloff and bandwidth, the onset rate, a music likelihood and an
//! estimated reverb tail.
//!
//! The noise floor is the BGM decision: a dry voiceover sits on a floor
//! tens of dB down and wants music under it; a take that already carries
//! music does not, and adding more only muds the mix. The clipping ratio
//! and the sample peak are the gain ceiling: a take that is already at
//! full scale must not be pushed harder. The hum and the sibilance share
//! are the repair shopping list, and the music likelihood is the honest
//! second opinion the BGM decision can cross-check.

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
    /// The take's own sample rate: the consumer that reads formants
    /// needs it to know the band it measured - 8 kHz phone audio cannot
    /// carry formants above 4 kHz, and the reading's confidence should
    /// say so.
    pub sample_rate_hz: u32,
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
    /// The loudest single sample, in dBFS. The ceiling a gain stage must
    /// respect before the true-peak measurement (which lives beside the
    /// loudness read) says anything about overs.
    pub sample_peak_dbfs: f32,
    /// The share of samples at or over 99.9% of full scale: the clipping
    /// ratio. Anything over zero on a voiceover is a recording fault the
    /// repair stage should know about and the mix must not amplify.
    pub clipping_ratio: f32,
    /// The take's mean sample value: a DC offset the encoder would carry
    /// as a waste of headroom and a click on every edit boundary.
    pub dc_offset: f32,
    /// The mains frequency the floor hums at (50 or 60 Hz, or a harmonic),
    /// when a Goertzel pass finds one standing well over its guard bands.
    /// `None` when the floor carries no stationary tone: the honest
    /// absence rather than a guessed frequency.
    pub hum_hz: Option<f32>,
    /// The share of the take's energy that sits in the 4-8 kHz sibilance
    /// band, 0..1: the harshness reading a de-esser or a gentle low-pass
    /// would act on, and the reason a bright take should not simply be
    /// turned up.
    pub sibilance_ratio: f32,
    /// The frequency below which 85% of the take's energy sits, in Hz: the
    /// bandwidth ceiling. A voiceover lives under 8 kHz; a take whose
    /// rolloff reaches the Nyquist carries hiss or cymbals, not speech.
    pub spectral_rolloff_hz: f32,
    /// The spectrum's spread around its own centroid, in Hz: how wide the
    /// timbre is. A narrow bandwidth is a pure tone or a clean voice; a
    /// wide one is a mix, a room, or noise.
    pub spectral_bandwidth_hz: f32,
    /// The spectral-flux onset rate, in onsets per minute: how dense the
    /// take's transients are, the honest input to a fast-or-held cut style
    /// that the onset count alone does not give (it does not know the
    /// take's length).
    pub onset_rate_per_min: f32,
    /// A 0-1 heuristic for how music-like the take is: onset regularity
    /// and onset density MULTIPLIED (a take needs both - an average would
    /// let either carry the number), with a flatness window as a pure-tone
    /// veto. Speech with a bed reads high; a dry read reads low. It is a
    /// cross-check for the BGM decision, not a classifier - the number
    /// says what it is built on.
    pub music_likelihood: f32,
    /// The estimated reverberation tail, in seconds: the median time the
    /// energy takes to fall 10 dB after an onset, scaled to a 60 dB decay.
    /// `None` when too few onsets measured a decay to say anything.
    pub reverb_tail_sec: Option<f32>,
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
/// noise: PitchTracker refuses a slice under two max-lags (2 x 2048
/// samples) and frames at 4096 points, so the floor is 4200 samples in
/// the take's own rate - 0.26 s at 16 kHz, less than a tenth of that at
/// 48 kHz. A fixed seconds floor handed 8 and 16 kHz takes slices the
/// tracker refuses outright.
fn min_voice_slice(rate: f32) -> f64 {
    4200.0 / f64::from(rate.max(1.0))
}

/// Read the forensics of a take.
pub fn analyse(
    take: &Take,
    timed: &[(usize, &LineSpec)],
    max_line_secs: f64,
) -> Result<ForensicOut, Error> {
    let rate = take.sample_rate as f32;
    let min_slice = min_voice_slice(rate);
    let mut by_index = std::collections::HashMap::new();
    for (index, line) in timed {
        let start = line.start.unwrap_or(0.0);
        let end = line.end.unwrap_or(0.0);
        let span = end - start;
        if span < min_slice || span > max_line_secs {
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
        // Max-hold decimation: a nearest-sample pick let a short transient
        // between two kept samples vanish, and the envelope is the shape the
        // BGM duck follows - the transient is exactly the shape it must not
        // miss. Each bucket keeps its loudest frame.
        let step = envelope.len() as f32 / 256.0;
        envelope = (0..256)
            .map(|i| {
                let from = (i as f32 * step) as usize;
                let to = (((i + 1) as f32 * step) as usize).min(envelope.len());
                envelope[from..to.max(from + 1)]
                    .iter()
                    .copied()
                    .fold(f32::MIN, f32::max)
            })
            .collect();
    }
    // The safety and identity readings: peak, clipping, DC offset.
    let mut peak = 0.0_f32;
    let mut clipped = 0_usize;
    let mut sum = 0.0_f64;
    for sample in samples.iter() {
        let magnitude = sample.abs();
        peak = peak.max(magnitude);
        if magnitude >= 0.999 {
            clipped += 1;
        }
        sum += f64::from(*sample);
    }
    let sample_peak_dbfs = 20.0 * (peak.max(1.0e-9)).log10();
    let clipping_ratio = clipped as f32 / samples.len() as f32;
    let dc_offset = (sum / samples.len() as f64) as f32;
    // The averaged power spectrum the band readings share.
    let duration = samples.len() as f64 / f64::from(rate);
    let spectrum = averaged_power_spectrum(samples, rate);
    let (sibilance_ratio, rolloff_hz, bandwidth_hz) = band_readings(&spectrum, rate);
    let hum_hz = hum_tone(samples, rate);
    let onset_rate = if duration > 0.0 {
        flux.onsets.len() as f32 / duration as f32 * 60.0
    } else {
        0.0
    };
    let music_likelihood = music_likelihood(&flux.onsets, duration, profile.spectral_flatness);
    let reverb_tail = reverb_tail_sec(samples, rate, &flux.onsets);
    Ok(WholeForensic {
        sample_rate_hz: take.sample_rate,
        snr_db: round3(snr),
        noise_floor_dbfs: round3(20.0 * (profile.noise_floor + 1.0e-12).log10()),
        // A Debug dump of the profiler's enum is not a label an API can
        // keep: a renamed variant would silently change the value. The
        // lowercase form is the stable contract.
        noise_type: format!("{:?}", profile.noise_type).to_lowercase(),
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
        sample_peak_dbfs: round3(sample_peak_dbfs),
        clipping_ratio: round6(clipping_ratio),
        dc_offset: round6(dc_offset),
        hum_hz: hum_hz.map(round3),
        sibilance_ratio: round3(sibilance_ratio),
        spectral_rolloff_hz: round3(rolloff_hz),
        spectral_bandwidth_hz: round3(bandwidth_hz),
        onset_rate_per_min: round3(onset_rate),
        music_likelihood: round3(music_likelihood),
        reverb_tail_sec: reverb_tail.map(round3),
    })
}

/// The frame every band reading is computed over, in samples: 1024 at
/// 16 kHz is 64 ms, the same frame the flux detector hops over.
const BAND_FRAME: usize = 1024;

/// The ceiling on frames the averaged spectrum pays for: an hour-long
/// take at 16 kHz is ~56k frames; the average changes nothing after a few
/// hundred, so the pass thins evenly and keeps its place in the take.
const MAX_BAND_FRAMES: usize = 400;

/// The averaged power spectrum over Hann-windowed frames: one half-length
/// magnitude-squared vector, `spectrum[i]` the energy at `i * rate /
/// BAND_FRAME` Hz. Frames are thinned evenly over the take so a long take
/// costs the same as a short one and no region is left unmeasured.
fn averaged_power_spectrum(samples: &[f32], rate: f32) -> Vec<f32> {
    if samples.len() < BAND_FRAME || rate <= 0.0 {
        return Vec::new();
    }
    let total = samples.len().div_ceil(BAND_FRAME);
    let thin = if total > MAX_BAND_FRAMES {
        total.div_ceil(MAX_BAND_FRAMES)
    } else {
        1
    };
    let bins = BAND_FRAME / 2;
    let mut spectrum = vec![0.0_f64; bins];
    let mut frames = 0_usize;
    let mut offset = 0_usize;
    while offset + BAND_FRAME <= samples.len() {
        let frame = &samples[offset..offset + BAND_FRAME];
        // The Hann window keeps the frame's edges from smearing the
        // spectrum the way a rectangular cut would.
        let windowed: Vec<f64> = frame
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / BAND_FRAME as f64).cos();
                w * f64::from(*s)
            })
            .collect();
        let mut real = windowed;
        let mut imag = vec![0.0_f64; BAND_FRAME];
        fft_in_place(&mut real, &mut imag);
        for i in 0..bins {
            spectrum[i] += real[i] * real[i] + imag[i] * imag[i];
        }
        frames += 1;
        offset += BAND_FRAME * thin;
    }
    if frames == 0 {
        return Vec::new();
    }
    spectrum
        .into_iter()
        .map(|energy| (energy / frames as f64) as f32)
        .collect()
}

/// An in-place iterative radix-2 FFT. The length must be a power of two;
/// the transform is the textbook Cooley-Tukey butterfly with a
/// bit-reversal permutation, kept here so the band readings need no FFT
/// dependency for what is one 1024-point pass every frame.
fn fft_in_place(real: &mut [f64], imag: &mut [f64]) {
    let n = real.len();
    if n <= 1 || !n.is_power_of_two() {
        return;
    }
    // Bit-reversal permutation.
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        if j > i {
            real.swap(i, j);
            imag.swap(i, j);
        }
    }
    // The butterflies: width 2, 4, ..., n.
    let mut width = 2;
    while width <= n {
        let step = std::f64::consts::TAU / width as f64;
        for block in (0..n).step_by(width) {
            for k in 0..width / 2 {
                let angle = step * k as f64;
                let (wr, wi) = (angle.cos(), -angle.sin());
                let i0 = block + k;
                let i1 = block + k + width / 2;
                let tr = wr * real[i1] - wi * imag[i1];
                let ti = wr * imag[i1] + wi * real[i1];
                real[i1] = real[i0] - tr;
                imag[i1] = imag[i0] - ti;
                real[i0] += tr;
                imag[i0] += ti;
            }
        }
        width *= 2;
    }
}

/// The band readings one averaged spectrum answers: the sibilance share
/// (the 4-8 kHz band over the whole), the 85% rolloff frequency and the
/// bandwidth (the spread around the spectrum's own centroid). An empty
/// spectrum - a take shorter than one frame - answers zeros, and so does
/// a digitally silent one: a zero spectrum would otherwise report a
/// Nyquist rolloff, the brightest reading for the quietest take.
fn band_readings(spectrum: &[f32], rate: f32) -> (f32, f32, f32) {
    if spectrum.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let hz_per_bin = rate / BAND_FRAME as f32;
    let summed: f64 = spectrum.iter().map(|p| f64::from(*p)).sum::<f64>();
    if summed <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let total = summed.max(1.0e-12);
    // The sibilance band: 4-8 kHz, where the /s/ and /sh/ sounds live.
    let sibilant: f64 = spectrum
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            let hz = *i as f32 * hz_per_bin;
            (4000.0..=8000.0).contains(&hz)
        })
        .map(|(_, p)| f64::from(*p))
        .sum();
    // The rolloff: the first bin where the running sum holds 85% of the
    // energy, in Hz - the percentile the catalogue, the Rust reader's
    // doc and the muffled-take rule all name.
    let mut running = 0.0_f64;
    let mut rolloff_bin = spectrum.len() - 1;
    for (i, power) in spectrum.iter().enumerate() {
        running += f64::from(*power);
        if running >= 0.85 * total {
            rolloff_bin = i;
            break;
        }
    }
    // The centroid and the bandwidth around it, from the same spectrum so
    // the two readings agree about which energy they mean.
    let centroid: f64 = spectrum
        .iter()
        .enumerate()
        .map(|(i, p)| (i as f64 * hz_per_bin as f64) * f64::from(*p))
        .sum::<f64>()
        / total;
    let spread: f64 = spectrum
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let hz = i as f64 * hz_per_bin as f64;
            let d = hz - centroid;
            d * d * f64::from(*p)
        })
        .sum::<f64>()
        / total;
    (
        (sibilant / total) as f32,
        (rolloff_bin as f32 + 1.0) * hz_per_bin,
        spread.sqrt() as f32,
    )
}

/// The Goertzel power at one exact frequency, over the whole take: the
/// narrowband detector the FFT's 15.6 Hz bins are too coarse to be for
/// a 50 Hz hum.
fn goertzel_power(samples: &[f32], rate: f32, hz: f32) -> f64 {
    if samples.is_empty() || rate <= 0.0 {
        return 0.0;
    }
    let k = (std::f64::consts::TAU * f64::from(hz)) / f64::from(rate);
    let coeff = 2.0 * k.cos();
    let mut s1 = 0.0_f64;
    let mut s2 = 0.0_f64;
    for sample in samples {
        let s0 = f64::from(*sample) + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2) / samples.len() as f64
}

/// The mains hum, when one stands over its guard bands: 50 and 60 Hz and
/// their first harmonics, each compared against the power just off the
/// candidate. A stationary tone 14 dB over its guards is a hum; anything
/// quieter is the floor's own texture and stays `None`. A harmonic alone
/// is not a hum: a monotone voice fundamental sitting on 100 or 120 Hz
/// would otherwise read as mains, so the 50/60 fundamental must have
/// passed its own guard before any harmonic is spent.
fn hum_tone(samples: &[f32], rate: f32) -> Option<f32> {
    const FUNDAMENTALS: &[(f32, f32, f32)] = &[(50.0, 40.0, 70.0), (60.0, 40.0, 70.0)];
    const HARMONICS: &[(f32, f32, f32, f32)] =
        &[(100.0, 90.0, 130.0, 50.0), (120.0, 90.0, 130.0, 60.0)];
    const RATIO: f64 = 25.0;
    let mut best: Option<(f32, f64)> = None;
    for (hz, low, high) in FUNDAMENTALS {
        let power = goertzel_power(samples, rate, *hz);
        let guard = goertzel_power(samples, rate, *low).max(goertzel_power(samples, rate, *high));
        if guard <= 0.0 || power / guard < RATIO {
            continue;
        }
        match best {
            // The fundamental wins over a harmonic at the same strength:
            // the hum is where the energy starts, not where it repeats.
            Some((_, best_power)) if power <= best_power => {}
            _ => best = Some((*hz, power)),
        }
    }
    // A harmonic is only spent beside its own fundamental: the 100 Hz
    // arm needs the 50 Hz one to have passed, the 120 Hz arm the 60 Hz
    // one, and the fundamental always wins at equal strength anyway.
    let fundamental_passed = |fundamental: f32| {
        FUNDAMENTALS.iter().any(|(fund, low, high)| {
            if *fund != fundamental {
                return false;
            }
            let guard =
                goertzel_power(samples, rate, *low).max(goertzel_power(samples, rate, *high));
            guard > 0.0 && goertzel_power(samples, rate, *fund) / guard >= RATIO
        })
    };
    for (hz, low, high, fundamental) in HARMONICS {
        if !fundamental_passed(*fundamental) {
            continue;
        }
        let power = goertzel_power(samples, rate, *hz);
        let guard = goertzel_power(samples, rate, *low).max(goertzel_power(samples, rate, *high));
        if guard <= 0.0 || power / guard < RATIO {
            continue;
        }
        match best {
            Some((_, best_power)) if power <= best_power => {}
            _ => best = Some((*hz, power)),
        }
    }
    best.map(|(hz, _)| hz)
}

/// How music-like the take is, 0..1. A beat is onsets that are both dense
/// and regular, so the core is the product of the two - a take needs both,
/// not an average of either. Two honest corrections follow. Onsets closer
/// than 50 ms are the same transient counted twice, so they merge before
/// the intervals are measured. And a near-pure spectrum (flatness under
/// 0.02) is a single spectral line, whose "onsets" are the flux detector's
/// own artefact on a tone that never changes - the reading is vetoed to
/// 15% rather than reported as a metronome. The number is a cross-check
/// for the BGM decision, and its basis is what it is: a heuristic that
/// says so about itself.
fn music_likelihood(onsets: &[f32], duration: f64, flatness: f32) -> f32 {
    // Merge the double-fires: a second onset inside 50 ms is the same
    // transient the detector answered twice, not a second beat.
    let mut merged: Vec<f32> = Vec::with_capacity(onsets.len());
    for onset in onsets {
        match merged.last() {
            Some(last) if *onset - *last < 0.05 => {}
            _ => merged.push(*onset),
        }
    }
    // Fewer than eight transients is not a pulse, whatever their spacing.
    if merged.len() < 8 || duration <= 0.0 {
        return 0.0;
    }
    // Onset regularity: the coefficient of variation of the inter-onset
    // intervals, inverted - a perfectly metronomic take reads 1.
    let intervals: Vec<f64> = merged
        .windows(2)
        .map(|pair| f64::from(pair[1] - pair[0]).max(1.0e-3))
        .collect();
    let mean = intervals.iter().sum::<f64>() / intervals.len() as f64;
    let variance = intervals
        .iter()
        .map(|i| (i - mean) * (i - mean))
        .sum::<f64>()
        / intervals.len() as f64;
    let cv = variance.sqrt() / mean;
    let regularity = 1.0 - cv.min(1.0);
    // Onset density: full density is two onsets a second and the floor
    // is one every 2.5 s - speech fires its transients in bursts, and a
    // beat needs a rate a listener would tap to.
    let per_second = merged.len() as f64 / duration;
    let density = ((per_second - 0.4) / 1.6).clamp(0.0, 1.0);
    let mut pulse = regularity * density;
    // The pure-tone veto: one spectral line has no beat, and the onsets
    // it produced were the detector answering a tone that never changed.
    if f64::from(flatness) < 0.02 {
        pulse *= 0.15;
    }
    pulse.clamp(0.0, 1.0) as f32
}

/// The estimated reverb tail: after each of the first onsets, the time the
/// energy takes to fall 10 dB from its post-onset peak, scaled by six to a
/// 60 dB decay (the linear-decay assumption RT60 rests on), medianed over
/// the onsets that produced a reading. `None` when fewer than three
/// onsets could be measured - one decay is an anecdote, not a room.
fn reverb_tail_sec(samples: &[f32], rate: f32, onsets: &[f32]) -> Option<f32> {
    const WINDOW: usize = 1024;
    const HOP: usize = 512;
    if samples.len() < WINDOW || rate <= 0.0 {
        return None;
    }
    let frame_secs = HOP as f64 / f64::from(rate);
    // Overlapping windows, HOP apart: non-overlapping `chunks(HOP)` can
    // never yield a WINDOW-sized slice, so this loop used to measure
    // nothing at all and every `reverbTailSec` was null - the room
    // reading the mix's duck and the auto edit's bed level lean on never
    // existed.
    let frame_db: Vec<f64> = samples
        .windows(WINDOW)
        .step_by(HOP)
        .map(|chunk| {
            let mean_square = chunk
                .iter()
                .map(|s| f64::from(*s) * f64::from(*s))
                .sum::<f64>()
                / chunk.len() as f64;
            10.0 * (mean_square + 1.0e-12).log10()
        })
        .collect();
    if frame_db.is_empty() {
        return None;
    }
    let mut decays: Vec<f64> = Vec::new();
    for onset in onsets.iter().take(50) {
        let start = (*onset as f64 / frame_secs) as usize;
        if start >= frame_db.len() {
            continue;
        }
        // The peak the onset rises to, inside the first 150 ms.
        let peak_end = (start + (0.15 / frame_secs) as usize + 1).min(frame_db.len());
        let Some((peak_i, peak_db)) = frame_db[start..peak_end]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, db)| (i + start, *db))
        else {
            continue;
        };
        // The first frame 10 dB under the peak, inside 2.5 s.
        let fall_end = (peak_i + (2.5 / frame_secs) as usize + 1).min(frame_db.len());
        let fell = frame_db[peak_i..fall_end]
            .iter()
            .position(|db| *db <= peak_db - 10.0);
        if let Some(frames) = fell {
            let decay_10db = frames as f64 * frame_secs;
            // A fall faster than 20 ms is the onset's own cut-off, not a
            // room; slower than 2 s is the take's dynamics, not a tail.
            if (0.02..=2.0).contains(&decay_10db) {
                decays.push(decay_10db);
            }
        }
    }
    if decays.len() < 3 {
        return None;
    }
    decays.sort_by(|a, b| a.total_cmp(b));
    let median = decays[decays.len() / 2];
    Some((median * 6.0) as f32)
}

/// The analysis configuration every analyzer here shares.
fn default_config() -> oximedia_audio_analysis::AnalysisConfig {
    // A window the YIN tracker can actually search. PitchTracker::new
    // fixes max_lag at 2048 samples and frames the take at
    // `fft_size.min(4096)`, so the default 2048-point config hands the
    // estimator a frame exactly max_lag long: its difference loop
    // `for j in 0..(len - max_lag)` is an empty range, the CMND stays
    // 1.0, every frame reads unvoiced, and the voice characteristics
    // (f0, jitter, shimmer, HNR) were null on every take the ear ever
    // read - the whole-take meanF0Hz and voicingRate were always 0.0,
    // and every decision downstream of them never fired. 4096 points
    // give the loop half a window of overlap; the lag range still
    // covers 7.8 Hz (16 kHz) to 23 Hz (48 kHz), far under any voice.
    oximedia_audio_analysis::AnalysisConfig {
        fft_size: 4096,
        ..Default::default()
    }
}

/// Round to three decimals.
fn round3(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}

/// Round to five decimals.
fn round5(value: f32) -> f32 {
    (value * 100_000.0).round() / 100_000.0
}

/// Round to six decimals: the clipping ratio and the DC offset are small
/// numbers where three decimals would round a real reading to zero.
fn round6(value: f32) -> f32 {
    (value * 1_000_000.0).round() / 1_000_000.0
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
    fn a_voice_like_slice_actually_reads_its_voice() {
        // The ear's dead window: with the default 2048-point config the
        // YIN tracker's difference loop was an empty range, every frame
        // read unvoiced, and f0/hnr/jitter/shimmer were null on every
        // take the ear ever read - while absences were honest nulls no
        // test failed. This is the pin: a periodic, voice-like slice
        // MUST answer its f0 (and the quality trio a voiced frame
        // carries), or the ear has died silently again.
        let take = take_with(140.0, 1.5, 16000);
        let reading = line_forensic(&take.samples[..24000.min(take.samples.len())], 16000.0);
        assert!(
            reading.f0_hz.is_some_and(|f0| (f0 - 140.0).abs() < 3.0),
            "f0 {reading:?}"
        );
        assert!(reading.hnr_db.is_some(), "hnr {reading:?}");
        assert!(reading.jitter.is_some(), "jitter {reading:?}");
        assert!(reading.shimmer.is_some(), "shimmer {reading:?}");
        let whole = whole_forensic(&take).expect("a whole tone analyses");
        assert!(
            whole.mean_f0_hz > 100.0 && whole.voicing_rate > 0.5,
            "mean f0 {} voicing {}",
            whole.mean_f0_hz,
            whole.voicing_rate
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

    #[test]
    fn a_quiet_tone_reads_its_own_sample_peak_and_no_clipping() {
        let take = take_with(200.0, 2.0, 16000); // amplitude 0.4
        let whole = whole_forensic(&take).expect("analyses");
        let expected = 20.0 * 0.4_f32.log10();
        assert!(
            (whole.sample_peak_dbfs - expected).abs() < 0.2,
            "peak {} vs {expected}",
            whole.sample_peak_dbfs
        );
        assert_eq!(whole.clipping_ratio, 0.0);
        assert!(whole.dc_offset.abs() < 0.01, "dc {}", whole.dc_offset);
    }

    #[test]
    fn a_clipped_and_biased_take_says_so() {
        // A sine driven 20% over full scale: every crest is flat-topped,
        // plus a 0.05 DC bias so the offset reading has something to find.
        let rate = 16000_u32;
        let n = 32000_usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let raw = (200.0 * std::f64::consts::TAU * t).sin() * 1.2;
                (raw.clamp(-1.0, 1.0) + 0.05) as f32
            })
            .collect();
        let take = Take {
            samples,
            sample_rate: rate,
            channels: 1,
        };
        let whole = whole_forensic(&take).expect("analyses");
        assert!(
            whole.clipping_ratio > 0.0,
            "clipped {}",
            whole.clipping_ratio
        );
        assert!(
            (whole.dc_offset - 0.05).abs() < 0.01,
            "dc {}",
            whole.dc_offset
        );
        assert!(
            whole.sample_peak_dbfs > -0.5,
            "peak {}",
            whole.sample_peak_dbfs
        );
    }

    #[test]
    fn a_take_with_mains_hum_names_the_frequency() {
        // A 50 Hz hum 30 dB over a 200 Hz tone: the Goertzel pass must
        // find 50, not 60 and not the tone itself.
        let rate = 16000_u32;
        let n = 48000_usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let tone = (200.0 * std::f64::consts::TAU * t).sin() * 0.1;
                let hum = (50.0 * std::f64::consts::TAU * t).sin() * 0.01;
                (tone + hum) as f32
            })
            .collect();
        let take = Take {
            samples,
            sample_rate: rate,
            channels: 1,
        };
        assert_eq!(hum_tone(&take.samples, 16000.0), Some(50.0));
    }

    #[test]
    fn the_band_readings_separate_a_tone_from_broadband_noise() {
        let rate = 16000_u32;
        // A 1 kHz tone: energy in one bin.
        let tone = take_with(1000.0, 2.0, rate);
        // Deterministic broadband noise: energy across every bin.
        let mut seed = 42_u64;
        let n = 32000_usize;
        let noise: Vec<f32> = (0..n)
            .map(|_| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (((seed >> 33) as f64 / 2_147_483_648.0) - 0.5) as f32
            })
            .collect();
        let noise_take = Take {
            samples: noise,
            sample_rate: rate,
            channels: 1,
        };
        let tone_spectrum = averaged_power_spectrum(&tone.samples, 16000.0);
        let noise_spectrum = averaged_power_spectrum(&noise_take.samples, 16000.0);
        let (tone_sibilance, tone_rolloff, tone_bandwidth) = band_readings(&tone_spectrum, 16000.0);
        let (noise_sibilance, noise_rolloff, _) = band_readings(&noise_spectrum, 16000.0);
        // A 1 kHz tone keeps its sibilance share near zero and its rolloff
        // near its own frequency; noise spreads both.
        assert!(tone_sibilance < 0.05, "tone sibilance {tone_sibilance}");
        assert!(noise_sibilance > 0.2, "noise sibilance {noise_sibilance}");
        assert!(tone_rolloff < 2000.0, "tone rolloff {tone_rolloff}");
        assert!(noise_rolloff > 6000.0, "noise rolloff {noise_rolloff}");
        assert!(tone_bandwidth < 500.0, "tone bandwidth {tone_bandwidth}");
    }

    #[test]
    fn the_fft_matches_a_direct_dft_on_a_small_input() {
        // 8 points, checked against the naive O(n^2) sum: the fast path
        // is only right if it agrees with the slow one.
        let real: Vec<f64> = vec![0.5, -0.3, 0.8, 0.1, -0.6, 0.4, 0.2, -0.7];
        let mut fast_real = real.clone();
        let mut fast_imag = vec![0.0; real.len()];
        fft_in_place(&mut fast_real, &mut fast_imag);
        for k in 0..real.len() {
            let mut slow_real = 0.0;
            let mut slow_imag = 0.0;
            for (t, x) in real.iter().enumerate() {
                let angle = -std::f64::consts::TAU * k as f64 * t as f64 / real.len() as f64;
                slow_real += x * angle.cos();
                slow_imag += x * angle.sin();
            }
            assert!(
                (fast_real[k] - slow_real).abs() < 1.0e-9,
                "bin {k}: {} vs {slow_real}",
                fast_real[k]
            );
            assert!(
                (fast_imag[k] - slow_imag).abs() < 1.0e-9,
                "bin {k}: {} vs {slow_imag}",
                fast_imag[k]
            );
        }
    }

    #[test]
    fn a_steady_tone_is_not_music_and_carries_no_reverb_tail() {
        let take = take_with(200.0, 4.0, 16000);
        let whole = whole_forensic(&take).expect("analyses");
        // A steady tone: no onsets, so no regularity, no density, and a
        // flatness far under the music window.
        assert!(
            whole.music_likelihood < 0.2,
            "likelihood {}",
            whole.music_likelihood
        );
        // The tone's onsets (if any) decay instantly, under the 20 ms
        // floor, so the tail reads absent rather than a room.
        assert!(
            whole.reverb_tail_sec.is_none_or(|secs| secs < 0.5),
            "tail {:?}",
            whole.reverb_tail_sec
        );
        // The noise label is the stable lowercase form, whatever the
        // profiler's enum names its variants.
        assert_eq!(whole.noise_type, whole.noise_type.to_lowercase());
    }

    #[test]
    fn a_decaying_take_measures_its_reverb_tail() {
        // Five bursts, each a tone that decays exponentially (tau 60 ms):
        // the energy falls 10 dB in about 70 ms, so the RT60 the frames
        // read should sit near 0.4 s. The old frame builder kept no
        // frames at all - `chunks(512)` never yields a 1024-sample
        // window - so the tail was always `None`; this test pins the
        // revival against a synthetic room.
        let rate = 16000_u32;
        let mut samples: Vec<f32> = Vec::with_capacity(16000 * 6);
        for i in 0..16000 * 6 {
            let t = i as f64 / f64::from(rate);
            let last_burst = t.floor().min(4.0);
            let age = t - last_burst;
            let decay = (-age / 0.06).exp();
            let sample = 0.5 * decay * (500.0 * TAU * t).sin();
            samples.push(sample as f32);
        }
        let take = Take {
            samples,
            sample_rate: rate,
            channels: 1,
        };
        let whole = whole_forensic(&take).expect("a decaying take analyses");
        let tail = whole
            .reverb_tail_sec
            .expect("a decaying take measures its tail");
        assert!(
            (0.15..=1.2).contains(&tail),
            "the tail {tail} should sit near the 0.4 s the decay implies"
        );
    }

    #[test]
    fn a_metronomic_take_reads_as_music_likely() {
        // A click every 0.5 s over broadband-ish noise: regular, dense
        // onsets with a mid flatness - the shape the heuristic exists to
        // catch, so a take that already carries a beat is not bedded again.
        let rate = 16000_u32;
        let mut seed = 7_u64;
        let mut next_click = 0_usize;
        let mut samples: Vec<f32> = Vec::with_capacity(16000 * 10);
        for i in 0..16000 * 10 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let noise = ((seed >> 33) as f64 / 2_147_483_648.0 - 0.5) * 0.05;
            let t = i as f64 / f64::from(rate);
            let click = if i >= next_click && i < next_click + 400 {
                (1000.0 * std::f64::consts::TAU * t).sin()
                    * 0.5
                    * (1.0 - (i - next_click) as f64 / 400.0)
            } else {
                0.0
            };
            if i == next_click + 400 {
                next_click += 8000;
            }
            samples.push((noise + click) as f32);
        }
        let take = Take {
            samples,
            sample_rate: rate,
            channels: 1,
        };
        let whole = whole_forensic(&take).expect("analyses");
        assert!(
            whole.onset_rate_per_min > 90.0,
            "rate {}",
            whole.onset_rate_per_min
        );
        assert!(
            whole.music_likelihood > 0.5,
            "likelihood {}",
            whole.music_likelihood
        );
    }
}
