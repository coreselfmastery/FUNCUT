// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Speech on the API, behind the `speech` feature: what was said, and what
//! to say.
//!
//! Both run as jobs, like an export: the method names the job and the work
//! happens on a thread of its own, reporting through
//! [`Event::AnalysisDone`]/[`Event::AnalysisFailed`] like every other job -
//! the result (a transcript's segments, a spoken WAV's path) rides in the
//! done event's `output` and then in the job status's.
//!
//! The models are the window's own, downloaded through Settings, and the
//! methods say which when one is missing rather than failing silently. A
//! transcription or a voice already running is refused by the speech
//! crate's one-at-a-time gates, and a job stopped by name ends as cancelled
//! at its next word - a voice in flight is not interrupted mid-sentence,
//! which is the same courtesy the window extends.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::message::{ApiError, ErrorCode, Event, JobStatus, ModelsState, Started};
use crate::{Api, AppDirs, EventSink};

/// The whisper model a transcription gets when the caller says nothing:
/// the fast English one. `small` is the multilingual choice.
const DEFAULT_TRANSCRIBER: &str = "base.en";
/// The voice model a generation gets when the caller says nothing: the one
/// that fits a 4 GB laptop.
const DEFAULT_SPEECH: &str = "kokoro-int8-multi-lang-v1_0";

impl Api {
    /// [`crate::Request::SpeechTranscribe`]: the words of a recording with
    /// their times, on a job of its own.
    pub fn speech_transcribe(
        &mut self,
        path: &str,
        model: Option<String>,
        language: Option<String>,
    ) -> Result<Started, ApiError> {
        if !std::path::Path::new(path).is_file() {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no such file: {path}"),
            ));
        }
        let summary = concat_host::media::probe(path).map_err(ApiError::failed)?;
        let Some(duration) = summary.duration.filter(|seconds| *seconds > 0.0) else {
            return Err(ApiError::new(
                ErrorCode::Refused,
                format!("{path} states no duration to transcribe"),
            ));
        };
        let request = concat_speech::transcribe::TranscribeRequest {
            path: path.to_owned(),
            audio_stream: None,
            source_start: 0.0,
            window: duration,
            model_id: model.unwrap_or_else(|| DEFAULT_TRANSCRIBER.to_owned()),
        };
        self.begin_speech(path, Work::Transcribe { request, language })
    }

    /// [`crate::Request::SpeechGenerate`]: a WAV of the words, in the
    /// project's own audio folder, on a job of its own.
    #[allow(clippy::too_many_arguments)]
    pub fn speech_generate(
        &mut self,
        project: &str,
        text: &str,
        voice: i32,
        speed: Option<f64>,
        model: Option<String>,
        delivery: Option<&str>,
        pauses: Option<f64>,
        steps: Option<i32>,
    ) -> Result<Started, ApiError> {
        self.session(project)?;
        self.writable(project)?;
        // Refused here, before a job is booked, so a typo is one round
        // trip rather than a job that starts and fails.
        if concat_speech::tts::voice_family(voice).is_none() {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!(
                    "unknown voice {voice} - a Kokoro voice is numbered 0-52; \
                     the `models_list` reply says which voice models are on disk"
                ),
            ));
        }
        if speed.is_some_and(|value| !value.is_finite() || !(0.5..=2.0).contains(&value)) {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                "speed must be a finite number from 0.5 to 2",
            ));
        }
        if pauses.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                "pauses must be a finite number from 0 to 1",
            ));
        }
        // 16 is the engine's own ceiling (its diffusion steps clamp there),
        // so anything above was accepted and then silently halved - an
        // error at the door names the real dial instead.
        if steps.is_some_and(|value| !(1..=16).contains(&value)) {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                "steps must be between 1 and 16",
            ));
        }
        let (delivery_speed, delivery_pauses) = match delivery {
            Some(name) => delivery_preset(name).ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Invalid,
                    format!(
                        "unknown delivery {name:?}; choose from neutral, conversational, \
                         urgent, excited, calm, awed, somber, authoritative"
                    ),
                )
            })?,
            None => (1.0_f32, None),
        };
        let request = concat_speech::tts::SpeakRequest {
            model_id: model.unwrap_or_else(|| DEFAULT_SPEECH.to_owned()),
            voice,
            reference: None,
            text: text.to_owned(),
            speed: speed.map_or(delivery_speed, |value| value as f32),
            pauses: pauses.map(|value| value as f32).or(delivery_pauses),
            steps,
            project: project.to_owned(),
        };
        self.begin_speech(project, Work::Generate { request })
    }

    /// [`crate::Request::ModelsList`]: every voice and transcriber model
    /// this build knows, with which are on disk.
    pub fn models_list(&mut self) -> Result<ModelsState, ApiError> {
        let tts = concat_speech::Speech::status(&self.dirs).map_err(ApiError::failed)?;
        let transcriber =
            concat_speech::Transcriber::status(&self.dirs).map_err(ApiError::failed)?;
        Ok(ModelsState {
            tts: tts.models,
            transcriber: transcriber.models,
        })
    }

    /// [`crate::Request::ModelsDownload`]: one model from Concat's mirror,
    /// on a job of its own - the download a headless caller had no way to
    /// start before, which the errors used to point at a Settings window
    /// that is not there without the window.
    pub fn models_download(&mut self, kind: &str, id: &str) -> Result<Started, ApiError> {
        let known = match kind {
            "tts" => concat_speech::Speech::status(&self.dirs)
                .map_err(ApiError::failed)?
                .models
                .iter()
                .any(|model| model.id == id),
            "whisper" => concat_speech::Transcriber::status(&self.dirs)
                .map_err(ApiError::failed)?
                .models
                .iter()
                .any(|model| model.id == id),
            other => {
                return Err(ApiError::new(
                    ErrorCode::Invalid,
                    format!("unknown model kind {other:?} - tts or whisper"),
                ));
            }
        };
        if !known {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("unknown {kind} model {id:?} - the ids are in models.list"),
            ));
        }
        self.begin_speech(
            id,
            Work::Download {
                kind: kind.to_owned(),
                id: id.to_owned(),
            },
        )
    }

    /// One speech job: named and booked like an export, run on its own
    /// thread, ending in the same events an analysis does.
    fn begin_speech(&mut self, path: &str, work: Work) -> Result<Started, ApiError> {
        let started = Started {
            job: self.jobs.mint(),
            path: path.to_owned(),
            output: String::new(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.book.start(
            JobStatus::running(&started.job, &started.path, &started.output),
            Arc::clone(&cancel),
        );
        let job = SpeechJob {
            name: started.job.clone(),
            path: started.path.clone(),
            work,
            events: Arc::clone(&self.events),
            cancel,
            dirs: AppDirs {
                config: self.dirs.config.clone(),
                data: self.dirs.data.clone(),
            },
            transcriber: Arc::clone(&self.transcriber),
            speech: Arc::clone(&self.speech),
        };
        let book = Arc::clone(&self.book);
        let name = started.job.clone();
        let thread = std::thread::Builder::new()
            .name(format!("speech {}", started.job))
            .spawn(move || job.run())
            .map_err(|error| {
                book.abandon(&name);
                ApiError::failed(format!("could not start the job: {error}"))
            })?;
        self.jobs.threads.push((started.job.clone(), thread));
        Ok(started)
    }
}

/// What one speech job is.
enum Work {
    /// Hear a recording.
    Transcribe {
        /// What to hear and with what model.
        request: concat_speech::transcribe::TranscribeRequest,
        /// The language hint, when the caller gave one.
        language: Option<String>,
    },
    /// Say the words.
    Generate {
        /// What to say, with which voice and where the WAV goes.
        request: concat_speech::tts::SpeakRequest,
    },
    /// Bring one model down from the mirror: the headless download.
    Download {
        /// Which engine the model serves: "tts" or "whisper".
        kind: String,
        /// The model's id.
        id: String,
    },
}

/// One speech job on its thread.
struct SpeechJob {
    /// The job's name, as every event about it carries.
    name: String,
    /// What the job works on: the recording, or the project.
    path: String,
    /// The work itself.
    work: Work,
    /// Where its ending goes.
    events: EventSink,
    /// How it is stopped, at the next word at the soonest.
    cancel: Arc<AtomicBool>,
    /// Where the models live.
    dirs: AppDirs,
    /// The ear.
    transcriber: Arc<concat_speech::Transcriber>,
    /// The voice.
    speech: Arc<concat_speech::Speech>,
}

impl SpeechJob {
    /// The whole job: work, then say how it went.
    fn run(self) {
        let done = match &self.work {
            Work::Transcribe { request, language } => self
                .transcriber
                .transcribe_with_language(&self.dirs, request, language.as_deref(), |_| {})
                .map(|segments| {
                    serde_json::json!({
                        "segments": segments,
                        "language": language,
                    })
                    .to_string()
                }),
            Work::Generate { request } => {
                self.speech
                    .speak(&self.dirs, request, |_| {})
                    .and_then(|result| {
                        serde_json::to_string(&result).map_err(|error| error.to_string())
                    })
            }
            Work::Download { kind, id } => {
                let downloaded = if kind == "tts" {
                    self.speech.download_model(&self.dirs, id, |_| {})
                } else {
                    self.transcriber.download_model(&self.dirs, id, |_| {})
                };
                downloaded.map(|()| {
                    serde_json::json!({
                        "kind": kind,
                        "id": id,
                        "downloaded": true,
                    })
                    .to_string()
                })
            }
        };
        let event = if self.cancel.load(Ordering::Relaxed) {
            Event::AnalysisFailed {
                job: self.name,
                path: self.path,
                error: ApiError::new(ErrorCode::Cancelled, "the job was stopped"),
            }
        } else {
            match done {
                Ok(output) => Event::AnalysisDone {
                    job: self.name,
                    path: self.path,
                    output,
                },
                Err(message) => Event::AnalysisFailed {
                    job: self.name,
                    path: self.path,
                    error: ApiError::failed(message),
                },
            }
        };
        (self.events)(event);
    }
}

/// The delivery presets: a speaking rate and a sentence-breath scale.
/// These are the two dials the shipped TTS families actually honour, so a
/// director can name a feeling without pretending the engine has an
/// emotion embedding. Explicit `speed` / `pauses` always win over these.
///
/// Rate is a multiplier on the voice's natural pace; breath is sherpa's
/// 0..1 silence scale (0.2 is the engine's own).
fn delivery_preset(name: &str) -> Option<(f32, Option<f32>)> {
    let name = name.trim().to_ascii_lowercase();
    Some(match name.as_str() {
        "neutral" => (1.0, None),
        "conversational" => (1.04, Some(0.22)),
        "urgent" => (1.14, Some(0.05)),
        "excited" => (1.12, Some(0.12)),
        "calm" => (0.92, Some(0.42)),
        "awed" => (0.95, Some(0.35)),
        "somber" => (0.88, Some(0.5)),
        "authoritative" => (0.96, Some(0.28)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::delivery_preset;

    #[test]
    fn delivery_names_a_pacing_not_a_fake_emotion() {
        assert_eq!(delivery_preset("neutral"), Some((1.0, None)));
        let (urgent_speed, urgent_breath) = delivery_preset("URGENT").expect("urgent");
        let (calm_speed, calm_breath) = delivery_preset(" calm ").expect("calm");
        assert!(urgent_speed > 1.1 && calm_speed < 1.0);
        assert!(urgent_breath.expect("urgent breath") < calm_breath.expect("calm breath"));
        assert_eq!(delivery_preset("melodramatic"), None);
    }

    #[test]
    fn every_delivery_stays_in_the_engine_range() {
        for name in [
            "neutral",
            "conversational",
            "urgent",
            "excited",
            "calm",
            "awed",
            "somber",
            "authoritative",
        ] {
            let (speed, breath) = delivery_preset(name).unwrap_or_else(|| panic!("{name}"));
            assert!((0.7..=1.3).contains(&speed), "{name} speed {speed}");
            if let Some(p) = breath {
                assert!((0.0..=1.0).contains(&p), "{name} breath {p}");
            }
        }
    }
}
