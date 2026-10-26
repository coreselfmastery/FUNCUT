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
    pub fn speech_generate(
        &mut self,
        project: &str,
        text: &str,
        voice: i32,
        speed: Option<f64>,
        model: Option<String>,
    ) -> Result<Started, ApiError> {
        self.session(project)?;
        self.writable(project)?;
        // Refused here, before a job is booked, so a typo is one round
        // trip rather than a job that starts and fails.
        if concat_speech::tts::voice_family(voice).is_none() {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!("unknown voice {voice} - the Settings > Speech ids, Kokoro 0-52"),
            ));
        }
        if speed.is_some_and(|value| !value.is_finite()) {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                "speed must be a finite number",
            ));
        }
        let request = concat_speech::tts::SpeakRequest {
            model_id: model.unwrap_or_else(|| DEFAULT_SPEECH.to_owned()),
            voice,
            reference: None,
            text: text.to_owned(),
            speed: speed.unwrap_or(1.0) as f32,
            pauses: None,
            steps: None,
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
