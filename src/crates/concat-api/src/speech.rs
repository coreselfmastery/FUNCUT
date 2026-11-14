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

/// A transcript with its times rounded to the millisecond, the shape the
/// answer keeps and the digits it does not: whisper's token timestamps
/// carry sixteen digits of f64, and a 25-minute voiceover's words at
/// ~24 characters of fraction each were tens of thousands of tokens of
/// precision no frame can use - a 30 fps frame is 33 ms wide, and 1 ms
/// is thirty times finer than that. Every consumer of the transcript -
/// the silence cuts, the captions, the SFX peaks - works to frame
/// tolerances; the rounding changes none of them and roughly halves
/// what a long video's transcript costs an agent's context.
fn rounded(
    segments: Vec<concat_speech::transcribe::Segment>,
) -> Vec<concat_speech::transcribe::Segment> {
    let millisecond = |time: f64| (time * 1000.0).round() / 1000.0;
    segments
        .into_iter()
        .map(|mut segment| {
            segment.start = millisecond(segment.start);
            segment.end = millisecond(segment.end);
            for word in &mut segment.words {
                word.start = millisecond(word.start);
                word.end = millisecond(word.end);
            }
            segment
        })
        .collect()
}

/// The whisper model a transcription gets when the caller says nothing:
/// the fast English one. `small` is the multilingual choice.
const DEFAULT_TRANSCRIBER: &str = "base.en";
/// The voice model a generation gets when the caller says nothing: the one
/// that fits a 4 GB laptop.
const DEFAULT_SPEECH: &str = "kokoro-int8-multi-lang-v1_0";

/// The writing system a stretch of transcribed text is dominated by
/// (DEG-09). Script, not language: Latin cannot tell English from French,
/// so the mismatch flag below is raised only for what a script scan can
/// honestly catch - the cross-script cases, and an English-only model
/// handed non-Latin audio, which whisper reads as transliterated garbage
/// either way. Same-script mismatches stay out of scope, on purpose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Script {
    Latin,
    Cyrillic,
    Greek,
    Han,
    Japanese,
    Hangul,
    Arabic,
    Hebrew,
    Devanagari,
    Bengali,
    Thai,
}

impl Script {
    /// The script one character is written in, when it is a letter.
    fn of(character: char) -> Option<Self> {
        Some(match character {
            '\u{0400}'..='\u{04FF}' => Script::Cyrillic,
            '\u{0370}'..='\u{03FF}' => Script::Greek,
            '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' => {
                Script::Han
            }
            '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' => Script::Japanese,
            '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7A3}' => {
                Script::Hangul
            }
            '\u{0600}'..='\u{06FF}'
            | '\u{0750}'..='\u{077F}'
            | '\u{FB50}'..='\u{FDFF}'
            | '\u{FE70}'..='\u{FEFF}' => Script::Arabic,
            '\u{0590}'..='\u{05FF}' => Script::Hebrew,
            '\u{0900}'..='\u{097F}' => Script::Devanagari,
            '\u{0980}'..='\u{09FF}' => Script::Bengali,
            '\u{0E00}'..='\u{0E7F}' => Script::Thai,
            'a'..='z' | 'A'..='Z' | '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}' => {
                Script::Latin
            }
            _ => return None,
        })
    }

    /// The name the reply carries.
    fn name(self) -> &'static str {
        match self {
            Script::Latin => "latin",
            Script::Cyrillic => "cyrillic",
            Script::Greek => "greek",
            Script::Han => "han",
            Script::Japanese => "japanese",
            Script::Hangul => "hangul",
            Script::Arabic => "arabic",
            Script::Hebrew => "hebrew",
            Script::Devanagari => "devanagari",
            Script::Bengali => "bengali",
            Script::Thai => "thai",
        }
    }

    /// Where this script sits in the count table.
    fn slot(self) -> usize {
        match self {
            Script::Latin => 0,
            Script::Cyrillic => 1,
            Script::Greek => 2,
            Script::Han => 3,
            Script::Japanese => 4,
            Script::Hangul => 5,
            Script::Arabic => 6,
            Script::Hebrew => 7,
            Script::Devanagari => 8,
            Script::Bengali => 9,
            Script::Thai => 10,
        }
    }
}

/// The script that outnumbers the rest in the text. Latin is the default
/// when nothing readable is there at all: an empty or punctuation-only
/// transcript is not a script disagreement.
fn dominant_script(text: &str) -> Script {
    let mut counts = [0_usize; 11];
    for character in text.chars() {
        if let Some(script) = Script::of(character) {
            counts[script.slot()] += 1;
        }
    }
    counts
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| **count)
        .map(|(slot, _)| match slot {
            1 => Script::Cyrillic,
            2 => Script::Greek,
            3 => Script::Han,
            4 => Script::Japanese,
            5 => Script::Hangul,
            6 => Script::Arabic,
            7 => Script::Hebrew,
            8 => Script::Devanagari,
            9 => Script::Bengali,
            10 => Script::Thai,
            _ => Script::Latin,
        })
        .unwrap_or(Script::Latin)
}

/// The script an ISO language code is written in, for the ones whose
/// script is not Latin. Anything unmapped is assumed Latin-script: the
/// check is a cross-script net, not a language identifier.
fn script_of_code(code: &str) -> Script {
    match code {
        "ru" | "uk" | "bg" | "be" | "sr" | "mk" | "kk" | "ky" | "tg" | "mn" => Script::Cyrillic,
        "el" => Script::Greek,
        "zh" => Script::Han,
        "ja" => Script::Japanese,
        "ko" => Script::Hangul,
        "ar" | "fa" | "ur" => Script::Arabic,
        "he" | "yi" => Script::Hebrew,
        "hi" | "mr" | "sa" | "ne" => Script::Devanagari,
        "bn" | "as" => Script::Bengali,
        "th" => Script::Thai,
        _ => Script::Latin,
    }
}

/// Whether the transcription's own script disagrees with what was asked
/// for: a language hint written in another script, or an English-only
/// model handed non-Latin audio (it cannot refuse; it transliterates).
/// Japanese and Han answer for each other - Japanese text is Han-dominant
/// whenever the kanji outnumber the kana.
fn language_mismatch(hint: Option<&str>, english_only_model: bool, script: Script) -> bool {
    if english_only_model && script != Script::Latin {
        return true;
    }
    let Some(hint) = hint else {
        return false;
    };
    let primary = hint
        .trim()
        .split(&['-', '_'][..])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if primary.is_empty() || primary == "auto" {
        return false;
    }
    match (script_of_code(&primary), script) {
        (Script::Japanese, Script::Han) | (Script::Han, Script::Japanese) => false,
        (expected, detected) => expected != detected,
    }
}

impl Api {
    /// [`crate::Request::SpeechTranscribe`]: the words of a recording with
    /// their times, on a job of its own.
    pub fn speech_transcribe(
        &mut self,
        path: &str,
        model: Option<String>,
        language: Option<String>,
        initial_prompt: Option<String>,
    ) -> Result<Started, ApiError> {
        if !std::path::Path::new(path).is_file() {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no such file: {path}"),
            ));
        }
        // The glossary whisper starts from: a prompt is the spelling the
        // whole run keeps, so one that is too long to be a glossary or
        // carries a byte whisper's setter panics on is refused at the
        // door, in one round trip, before a job is booked for it.
        if let Some(prompt) = initial_prompt.as_deref() {
            if prompt.chars().count() > 200 {
                return Err(ApiError::new(
                    ErrorCode::Invalid,
                    format!(
                        "initialPrompt is {} characters; the glossary is at most 200 - name the terms, do not write the script",
                        prompt.chars().count()
                    ),
                ));
            }
            if prompt.contains('\0') {
                return Err(ApiError::new(
                    ErrorCode::Invalid,
                    "initialPrompt must not contain NUL",
                ));
            }
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
        self.begin_speech(
            path,
            Work::Transcribe {
                request,
                language,
                prompt: initial_prompt,
            },
        )
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
            crate::job::Side::Own,
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
        /// The initial-prompt glossary, when the caller gave one: the
        /// proper nouns and jargon whisper starts its run knowing.
        prompt: Option<String>,
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
            Work::Transcribe {
                request,
                language,
                prompt,
            } => self
                .transcriber
                .transcribe_with_language(
                    &self.dirs,
                    request,
                    language.as_deref(),
                    prompt.as_deref(),
                    |_| {},
                )
                .map(|segments| {
                    // DEG-09: the reply says what ran, what script the
                    // words came back in, and whether that disagrees with
                    // the ask - a wrong-language transcript discovered at
                    // the captions is three phases too late.
                    let words: String = segments
                        .iter()
                        .map(|segment| segment.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let script = dominant_script(&words);
                    let mismatch = language_mismatch(
                        language.as_deref(),
                        request.model_id.ends_with(".en"),
                        script,
                    );
                    serde_json::json!({
                        "segments": rounded(segments),
                        "language": language,
                        "model": request.model_id,
                        "scriptDetected": script.name(),
                        "languageMismatch": mismatch,
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
    use super::{Script, delivery_preset, dominant_script, language_mismatch};

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

    #[test]
    fn the_script_scan_names_the_writing_system() {
        assert_eq!(dominant_script("hello, brave new world"), Script::Latin);
        assert_eq!(dominant_script("Привет, новый мир"), Script::Cyrillic);
        assert_eq!(dominant_script("こんにちは世界"), Script::Japanese);
        assert_eq!(dominant_script("안녕하세요 세계"), Script::Hangul);
        assert_eq!(dominant_script("مرحبا بالعالم"), Script::Arabic);
        // Punctuation and digits alone are not a disagreement.
        assert_eq!(dominant_script("123 ... !?"), Script::Latin);
    }

    #[test]
    fn a_wrong_language_transcript_is_flagged_not_swallowed() {
        // An English-only model handed non-Latin audio.
        assert!(language_mismatch(None, true, Script::Cyrillic));
        assert!(!language_mismatch(None, true, Script::Latin));
        // A hint the transcript's own script contradicts.
        assert!(language_mismatch(Some("ru"), false, Script::Latin));
        assert!(language_mismatch(Some("en"), false, Script::Hangul));
        assert!(!language_mismatch(Some("fr"), false, Script::Latin));
        // Japanese answers for Han and back: kanji-heavy text is still
        // Japanese, and a zh hint over kana-less Han is agreed.
        assert!(!language_mismatch(Some("ja"), false, Script::Han));
        assert!(!language_mismatch(Some("zh"), false, Script::Han));
        assert!(language_mismatch(Some("zh"), false, Script::Japanese));
        // No hint, multilingual model, any script: whisper's own choice.
        assert!(!language_mismatch(None, false, Script::Cyrillic));
        assert!(!language_mismatch(Some("auto"), false, Script::Greek));
        // A BCP-47 tag reads by its primary subtag.
        assert!(language_mismatch(Some("en-US"), false, Script::Thai));
    }
}
