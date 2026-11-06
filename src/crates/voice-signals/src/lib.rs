// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The director's ears: the signal extractor behind the `voice_signals`
//! sense command.
//!
//! The editor already knows *what* was said (the transcript) and *how it
//! felt* (the emotion read). This crate answers everything the voice itself
//! can tell an editor that words cannot:
//!
//! - **Prosody** — the pitch contour, the energy envelope and the rhythm of
//!   the delivery, per line and over the whole take
//!   ([`prosody`](crate::prosody), built on `spectral-prosody`).
//! - **Forensics** — formants, noise floor, SNR, spectral flux, dynamics and
//!   voice quality ([`forensic`](crate::forensic), built on
//!   `oximedia-audio-analysis`).
//! - **Speakers** — who spoke when, from an energy VAD and a voice-print
//!   clustering ([`speaker`](crate::speaker), built on `polyvoice`).
//! - **Entities** — the names, numbers, dates and money a line names, mapped
//!   back to word timings ([`text_ears`](crate::text_ears), built on `anno`).
//!
//! The interface mirrors the Python sense scripts: one JSON spec in, one
//! JSON object out, no files written except by `selftest`. The Python side
//! (`sense/voice_signals.py`) decodes any media to a 16-bit mono WAV with
//! ffmpeg, measures the loudness-side signals (LUFS per line, pause
//! classification, speech rate) that live better beside the transcript, and
//! merges the two halves into one reading for the director.
//!
//! Every extractor degrades honestly: a line too short for formants reports
//! `null` formants rather than a guess, a take with one voiced window
//! reports a single speaker rather than a clustering, and the `sources`
//! object in the output always names the library that produced each family
//! of signals.

pub mod forensic;
pub mod prosody;
pub mod speaker;
pub mod text_ears;

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Everything that can go wrong in one extraction.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The WAV file named by the spec could not be opened or decoded.
    #[error("could not read the WAV: {0}")]
    Wav(String),
    /// The spec itself is inconsistent (a missing file, an empty line list).
    #[error("{0}")]
    Spec(String),
    /// One of the analysis libraries refused the audio.
    #[error("analysis failed: {0}")]
    Analysis(String),
    /// The selftest could not create its scratch directory.
    #[error("selftest scratch: {0}")]
    Scratch(String),
}

/// One measured word, as the transcript job reports it.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WordSpec {
    /// Seconds from the take's start.
    pub start: f64,
    /// Seconds from the take's start.
    pub end: f64,
    /// The word itself.
    pub text: String,
}

/// One transcript line: its span, its text and its measured words.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LineSpec {
    /// Seconds from the take's start; `None` on a text-only line, which
    /// still gets its entities read but no audio-derived signals.
    pub start: Option<f64>,
    /// Seconds from the take's start.
    pub end: Option<f64>,
    /// What was said.
    pub text: String,
    /// The same words, measured one by one. May be empty when only the
    /// line-level timings exist; entity time mapping then falls back to the
    /// line's own span.
    #[serde(default)]
    pub words: Vec<WordSpec>,
}

/// The voice-activity configuration, overridable from the spec.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VadSpec {
    /// The energy threshold in dB below which a frame is silence
    /// (ffmpeg's `silencedetect` convention: -35 default).
    #[serde(default = "default_vad_threshold_db")]
    pub threshold_db: f32,
    /// VAD frame size in samples (512 default: 32 ms at 16 kHz).
    #[serde(default = "default_vad_frame")]
    pub frame_size: usize,
    /// Frames of probability at or above this count as speech (0.5).
    #[serde(default = "default_vad_probability")]
    pub probability: f32,
    /// Speech shorter than this many seconds is dropped (0.12 default).
    #[serde(default = "default_vad_min_speech")]
    pub min_speech_secs: f64,
    /// Silence shorter than this many seconds does not split speech
    /// (0.20 default).
    #[serde(default = "default_vad_min_silence")]
    pub min_silence_secs: f64,
}

fn default_vad_threshold_db() -> f32 {
    -35.0
}
fn default_vad_frame() -> usize {
    512
}
fn default_vad_probability() -> f32 {
    0.5
}
fn default_vad_min_speech() -> f64 {
    0.12
}
fn default_vad_min_silence() -> f64 {
    0.20
}

impl Default for VadSpec {
    fn default() -> Self {
        Self {
            threshold_db: default_vad_threshold_db(),
            frame_size: default_vad_frame(),
            probability: default_vad_probability(),
            min_speech_secs: default_vad_min_speech(),
            min_silence_secs: default_vad_min_silence(),
        }
    }
}

/// The whole extraction request, as the Python bridge writes it.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    /// A 16-bit PCM mono WAV of the take (the bridge's ffmpeg handoff).
    /// `None` on a text-only pass: entities are still extracted, audio
    /// signals are not.
    pub wav: Option<String>,
    /// The transcript's lines with their measured words.
    pub lines: Vec<LineSpec>,
    /// The ceiling on distinct speakers (4 default: an interview plus a
    /// host).
    #[serde(default = "default_max_speakers")]
    pub max_speakers: usize,
    /// The voice-activity configuration.
    #[serde(default)]
    pub vad: VadSpec,
    /// The cosine-similarity threshold at which two voice-print windows
    /// may be clustered into one speaker (0.60 default; lower is more
    /// eager to merge).
    #[serde(default = "default_speaker_threshold")]
    pub speaker_threshold: f32,
    /// The longest line, in seconds, still worth a per-line voice analysis
    /// (30 default; longer spans are analysed whole and per-line fields
    /// are left null rather than paying minutes of CPU).
    #[serde(default = "default_max_line_secs")]
    pub max_line_secs: f64,
}

fn default_max_speakers() -> usize {
    4
}
fn default_speaker_threshold() -> f32 {
    0.60
}
fn default_max_line_secs() -> f64 {
    30.0
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            wav: None,
            lines: Vec::new(),
            max_speakers: default_max_speakers(),
            vad: VadSpec::default(),
            speaker_threshold: default_speaker_threshold(),
            max_line_secs: default_max_line_secs(),
        }
    }
}

/// The whole extraction, ready to serialise.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// Always present; the honest extraction note.
    pub ok: bool,
    /// The decoded take's facts, or `None` on a text-only pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioFacts>,
    /// The prosody ear.
    pub prosody: prosody::ProsodyOut,
    /// The forensic ear.
    pub forensic: forensic::ForensicOut,
    /// The speaker ear.
    pub speaker: speaker::SpeakerOutAll,
    /// The text ear.
    pub entities: Vec<text_ears::EntityOut>,
    /// Which library produced each family of signals.
    pub sources: serde_json::Value,
}

/// What the decoded take itself says.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AudioFacts {
    /// The take's length in seconds.
    pub duration_sec: f64,
    /// The WAV's sample rate in Hz.
    pub sample_rate: u32,
    /// The channel count after mix-down (1 expected; the bridge writes
    /// mono, the number is reported so a surprise is visible).
    pub channels: u32,
}

/// One decoded take: mono f32 samples at the file's own rate.
pub struct Take {
    /// Samples in `[-1, 1]`, channels mixed down.
    pub samples: Vec<f32>,
    /// The file's sample rate in Hz.
    pub sample_rate: u32,
    /// The channel count the file carried.
    pub channels: u32,
}

impl Take {
    /// The take's length in seconds.
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }

    /// The samples inside `[start, end)` seconds, clamped to the take.
    pub fn slice(&self, start: f64, end: f64) -> &[f32] {
        let len = self.samples.len();
        let from = (start.max(0.0) * f64::from(self.sample_rate)) as usize;
        let to = ((end.max(start.max(0.0))) * f64::from(self.sample_rate)) as usize;
        let from = from.min(len);
        let to = to.min(len).max(from);
        &self.samples[from..to]
    }
}

/// Decode a WAV file: any bit depth `hound` reads, any channel count
/// (mixed down by averaging), any sample rate (kept as found; every
/// downstream frame is computed from the real rate).
pub fn decode_wav(path: &Path) -> Result<Take, Error> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| Error::Wav(format!("{}: {e}", path.display())))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    match spec.sample_format {
        hound::SampleFormat::Float => {
            let raw: Vec<f32> = reader.samples::<f32>().filter_map(Result::ok).collect();
            Ok(Take {
                samples: mix_down(raw, channels),
                sample_rate: spec.sample_rate,
                channels: u32::from(spec.channels),
            })
        }
        hound::SampleFormat::Int => {
            let max = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            let raw: Vec<f32> = reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| (s as f32) / max)
                .collect();
            Ok(Take {
                samples: mix_down(raw, channels),
                sample_rate: spec.sample_rate,
                channels: u32::from(spec.channels),
            })
        }
    }
}

/// Average interleaved channels down to one mono stream.
fn mix_down(raw: Vec<f32>, channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return raw;
    }
    raw.chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Run the whole extraction: decode (unless text-only), then each ear in
/// turn, then the sources note.
pub fn extract(spec: &Spec) -> Result<Output, Error> {
    if spec.lines.is_empty() {
        return Err(Error::Spec(
            "no lines: the spec needs the transcript's lines to read".into(),
        ));
    }
    let mut audio_facts = None;
    let (prosody, forensic, speaker) = if let Some(wav) = spec.wav.as_deref() {
        let path = Path::new(wav);
        if !path.is_file() {
            return Err(Error::Spec(format!("no such WAV file: {wav}")));
        }
        let take = decode_wav(path)?;
        audio_facts = Some(AudioFacts {
            duration_sec: take.duration(),
            sample_rate: take.sample_rate,
            channels: take.channels,
        });
        let timed: Vec<(usize, &LineSpec)> = spec
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                line.start.is_some_and(|s| s >= 0.0)
                    && line.end.is_some_and(|e| e > line.start.unwrap_or(0.0))
            })
            .collect();
        let prosody = prosody::analyse(&take, &timed);
        let forensic = forensic::analyse(&take, &timed, spec.max_line_secs)?;
        let speaker = speaker::analyse(&take, spec)?;
        (prosody, forensic, speaker)
    } else {
        (
            prosody::ProsodyOut::absent("no WAV in the spec"),
            forensic::ForensicOut::absent("no WAV in the spec"),
            speaker::SpeakerOutAll::absent("no WAV in the spec"),
        )
    };
    let entities = text_ears::entities(&spec.lines)?;
    Ok(Output {
        ok: true,
        audio: audio_facts,
        prosody,
        forensic,
        speaker,
        entities,
        sources: serde_json::json!({
            "prosody": "spectral-prosody 0.1.1 (autocorrelation pitch, RMS energy, Laplacian rhythm layers, Fiedler phrases)",
            "forensic": "oximedia-audio-analysis 0.2.1 (LPC formants, noise profile, YIN pitch, spectral flux, dynamics, voice quality)",
            "speaker": "polyvoice 0.17.0 (EnergyVad, agglomerative cosine clustering over oximedia voice-prints)",
            "entities": "anno 0.14.0 (offline pattern NER: PER/ORG/LOC/DATE/MONEY and friends)",
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_down_stereo_averages_the_channels() {
        let mixed = mix_down(vec![0.5, -0.5, 1.0, 0.0], 2);
        assert_eq!(mixed, vec![0.0, 0.5]);
    }

    #[test]
    fn slicing_clamps_and_never_inverts() {
        let take = Take {
            samples: vec![0.1; 160],
            sample_rate: 16,
            channels: 1,
        };
        assert_eq!(take.slice(-5.0, 1.0).len(), 16);
        assert_eq!(take.slice(9.0, 99.0).len(), 16);
        assert_eq!(take.slice(3.0, 3.0).len(), 0);
    }

    #[test]
    fn an_empty_line_list_is_a_spec_error_not_a_crash() {
        let err = extract(&Spec::default()).unwrap_err();
        assert!(err.to_string().contains("no lines"), "{err}");
    }

    #[test]
    fn a_missing_wav_is_a_spec_error_naming_the_file() {
        let spec = Spec {
            wav: Some("/nonexistent/vo.wav".into()),
            lines: vec![LineSpec {
                start: Some(0.0),
                end: Some(1.0),
                text: "hello".into(),
                words: Vec::new(),
            }],
            ..Spec::default()
        };
        let err = extract(&spec).unwrap_err();
        assert!(err.to_string().contains("vo.wav"), "{err}");
    }
}
