// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The agent's senses: scene cuts, beat grids and dead air.
//!
//! Three Python scripts live beside this crate in `analysis/` - PySceneDetect
//! for the shots, librosa for the tempo, FFmpeg's `silencedetect` (with a
//! librosa fallback) for the pauses. They are embedded into the binary at
//! build time with [`include_str!`], written once into the app's data folder
//! when the first analysis runs, and spoken to as a subprocess: one JSON
//! result on stdout, or one JSON `{"error": ..., "hint": ...}` and a nonzero
//! exit when something is missing. That contract is what makes the honest
//! refusal possible - a dependency that is not there says so in the reply,
//! instead of the job hanging or lying.
//!
//! Every analysis runs as a job, exactly like an export: the method returns
//! the job's name at once, the work happens on a thread of its own, and the
//! result arrives as [`Event::AnalysisDone`] carrying the JSON the script
//! printed - which is also what the job's status reports as its `output`, so
//! a caller that polls instead of listening sees the same thing. Long work
//! never blocks the one [`Api`] every caller shares.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::message::{ApiError, ErrorCode, Event, JobStatus, Started};
use crate::{Api, AppDirs, EventSink};

/// The scripts and the file that names what they need, embedded from
/// `src/crates/concat-api/analysis/` at build time.
const SCENES: &str = include_str!("../analysis/analyze_scenes.py");
const BEATS: &str = include_str!("../analysis/analyze_beats.py");
const SILENCE: &str = include_str!("../analysis/analyze_silence.py");
const REQUIREMENTS: &str = include_str!("../analysis/requirements.txt");

/// How long a script may run before it is killed: ten minutes is a
/// documentary, on a laptop.
const LIMIT: Duration = Duration::from_secs(600);
/// How often the runner looks at the child: at its frame rate, in effect.
const POLL: Duration = Duration::from_millis(100);

impl Api {
    /// [`crate::Request::AnalysisScenes`]: the shot changes of a video, on
    /// a job of its own.
    pub fn analysis_scenes(
        &mut self,
        path: &str,
        threshold: Option<f64>,
        detector: Option<String>,
        min_scene_len: Option<f64>,
    ) -> Result<Started, ApiError> {
        readable(path)?;
        // Bounded here, before a job is booked: a negative threshold or
        // an unknown detector used to become a Python traceback a caller
        // waited a job to read.
        if let Some(threshold) = threshold
            && (!threshold.is_finite() || !(0.0..=100.0).contains(&threshold))
        {
            return Err(ApiError::invalid(
                "threshold must be a finite number between 0 and 100",
            ));
        }
        if let Some(detector) = detector.as_deref()
            && !matches!(detector, "adaptive" | "content" | "threshold")
        {
            return Err(ApiError::invalid(
                "detector must be adaptive, content, or threshold",
            ));
        }
        if let Some(min_scene_len) = min_scene_len
            && (!min_scene_len.is_finite() || min_scene_len < 0.0)
        {
            return Err(ApiError::invalid(
                "minSceneLen must be a finite number, zero or more",
            ));
        }
        let mut args = vec!["--input".to_owned(), path.to_owned()];
        if let Some(threshold) = threshold {
            args.push("--threshold".to_owned());
            args.push(threshold.to_string());
        }
        if let Some(detector) = detector {
            args.push("--detector".to_owned());
            args.push(detector);
        }
        if let Some(min_scene_len) = min_scene_len {
            args.push("--min-scene-len".to_owned());
            args.push(min_scene_len.to_string());
        }
        self.begin_analysis(path, "analyze_scenes.py", args)
    }

    /// [`crate::Request::AnalysisBeats`]: the tempo, the beat grid and the
    /// onsets of a piece of music, on a job of its own.
    pub fn analysis_beats(&mut self, path: &str) -> Result<Started, ApiError> {
        readable(path)?;
        let args = vec!["--input".to_owned(), path.to_owned()];
        self.begin_analysis(path, "analyze_beats.py", args)
    }

    /// [`crate::Request::AnalysisSilence`]: the dead air of a recording, on
    /// a job of its own.
    pub fn analysis_silence(
        &mut self,
        path: &str,
        threshold_db: Option<f64>,
        min_silence_ms: Option<i64>,
    ) -> Result<Started, ApiError> {
        readable(path)?;
        if let Some(threshold_db) = threshold_db
            && (!threshold_db.is_finite() || !(0.0..=100.0).contains(&threshold_db))
        {
            return Err(ApiError::invalid(
                "thresholdDb must be a finite number between 0 and 100",
            ));
        }
        if let Some(min_silence_ms) = min_silence_ms
            && min_silence_ms < 0
        {
            return Err(ApiError::invalid("minSilenceMs must be zero or more"));
        }
        let mut args = vec!["--input".to_owned(), path.to_owned()];
        if let Some(threshold_db) = threshold_db {
            args.push("--threshold-db".to_owned());
            args.push(threshold_db.to_string());
        }
        if let Some(min_silence_ms) = min_silence_ms {
            args.push("--min-silence-ms".to_owned());
            args.push(min_silence_ms.to_string());
        }
        self.begin_analysis(path, "analyze_silence.py", args)
    }

    /// One analysis job: scripts written out (once), the child started, the
    /// job named and booked, the thread run. The job's `path` is the file it
    /// reads and its `output` is empty until it ends - the result rides in
    /// [`Event::AnalysisDone`] and then in the status's `output`.
    fn begin_analysis(
        &mut self,
        path: &str,
        script: &str,
        args: Vec<String>,
    ) -> Result<Started, ApiError> {
        let dir = materialize(&self.dirs)?;
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
        let job = AnalysisJob {
            name: started.job.clone(),
            path: started.path.clone(),
            script: dir.join(script),
            args,
            events: Arc::clone(&self.events),
            cancel,
        };
        let book = Arc::clone(&self.book);
        let name = started.job.clone();
        let thread = std::thread::Builder::new()
            .name(format!("analysis {}", started.job))
            .spawn(move || job.run())
            .map_err(|error| {
                book.abandon(&name);
                ApiError::failed(format!("could not start the analysis: {error}"))
            })?;
        self.jobs.threads.push((started.job.clone(), thread));
        Ok(started)
    }
}

/// One analysis on its thread.
struct AnalysisJob {
    /// The job's name, as every event about it carries.
    name: String,
    /// The file it reads, for the events.
    path: String,
    /// The script to run, materialized in the app's data folder.
    script: PathBuf,
    /// What to say to it, as words.
    args: Vec<String>,
    /// Where its ending goes.
    events: EventSink,
    /// How it is stopped.
    cancel: Arc<AtomicBool>,
}

impl AnalysisJob {
    /// The whole job: run, then say how it went.
    fn run(self) {
        let event = match run_script(&self.script, &self.args, &self.cancel) {
            Ok(output) => Event::AnalysisDone {
                job: self.name,
                path: self.path,
                output,
            },
            Err(error) => Event::AnalysisFailed {
                job: self.name,
                path: self.path,
                error,
            },
        };
        (self.events)(event);
    }
}

/// A file that is not there is not a job; it is a refusal at once.
fn readable(path: &str) -> Result<(), ApiError> {
    if Path::new(path).is_file() {
        Ok(())
    } else {
        Err(ApiError::new(
            ErrorCode::NotFound,
            format!("no such file: {path}"),
        ))
    }
}

/// Writes the scripts into the app's data folder, atomically, only when they
/// are not there already (or not what this build embeds), and says where.
fn materialize(dirs: &AppDirs) -> Result<PathBuf, ApiError> {
    let dir = dirs.data.join("analysis");
    let write = |name: &str, contents: &str| -> Result<(), ApiError> {
        let path = dir.join(name);
        let current = std::fs::read(&path).unwrap_or_default();
        if current == contents.as_bytes() {
            return Ok(());
        }
        let part = dir.join(format!("{name}.part"));
        std::fs::write(&part, contents).map_err(|error| {
            ApiError::failed(format!("could not write {}: {error}", part.display()))
        })?;
        std::fs::rename(&part, &path).map_err(|error| {
            ApiError::failed(format!(
                "could not move {} into place: {error}",
                path.display()
            ))
        })
    };
    std::fs::create_dir_all(&dir)
        .map_err(|error| ApiError::failed(format!("could not make {}: {error}", dir.display())))?;
    write("analyze_scenes.py", SCENES)?;
    write("analyze_beats.py", BEATS)?;
    write("analyze_silence.py", SILENCE)?;
    write("requirements.txt", REQUIREMENTS)?;
    Ok(dir)
}

/// Runs one script: starts it, watches it, returns its JSON compacted onto
/// one line. Killed - and said so honestly - when it is cancelled or when it
/// outlives [`LIMIT`].
fn run_script(script: &Path, args: &[String], cancel: &AtomicBool) -> Result<String, ApiError> {
    let program = std::env::var("CONCAT_ANALYSIS_PYTHON")
        .ok()
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "python3".to_owned());
    let mut command = Command::new(&program);
    command
        .arg(script)
        .args(args.iter())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| spawn_refusal(&error, &program))?;
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let mut out_reader = out_pipe.map(|pipe| std::thread::spawn(move || drain(pipe)));
    let mut err_reader = err_pipe.map(|pipe| std::thread::spawn(move || drain(pipe)));
    let deadline = Instant::now() + LIMIT;
    let mut stopped: Option<ApiError> = None;
    let status = loop {
        if stopped.is_none() && cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            stopped = Some(ApiError::new(
                ErrorCode::Cancelled,
                "the analysis was stopped",
            ));
        }
        if stopped.is_none() && Instant::now() > deadline {
            let _ = child.kill();
            stopped = Some(ApiError::failed(format!(
                "the analysis outlived {} seconds",
                LIMIT.as_secs()
            )));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| ApiError::failed(format!("waiting on the analysis failed: {error}")))?
        {
            break status;
        }
        std::thread::sleep(POLL);
    };
    let out = out_reader
        .take()
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let err = err_reader
        .take()
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    if let Some(stopped) = stopped {
        return Err(stopped);
    }
    if !status.success() {
        return Err(script_refusal(status, &program, &out, &err));
    }
    let value: serde_json::Value = serde_json::from_str(&out)
        .map_err(|error| ApiError::failed(format!("the analysis did not print JSON: {error}")))?;
    Ok(value.to_string())
}

/// A spawn that did not happen, as the caller can act on: no Python on this
/// machine is a setup problem, not an analysis failure.
fn spawn_refusal(error: &std::io::Error, program: &str) -> ApiError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ApiError::new(
            ErrorCode::Refused,
            format!(
                "{program} was not found: the analysis tools need Python 3, and the \
                 extras named by requirements.txt in the analysis folder"
            ),
        )
    } else {
        ApiError::failed(format!("could not run {program}: {error}"))
    }
}

/// The script's own word for what went wrong, when it said one; its exit
/// code 3 means a dependency of its own is missing, which the caller can fix.
fn script_refusal(status: ExitStatus, program: &str, out: &str, err: &str) -> ApiError {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(out)
        && let Some(error) = value.get("error").and_then(|word| word.as_str())
    {
        let hint = value
            .get("hint")
            .and_then(|word| word.as_str())
            .unwrap_or_default();
        let code = if status.code() == Some(3) {
            ErrorCode::Refused
        } else {
            ErrorCode::Failed
        };
        let message = if hint.is_empty() {
            error.to_owned()
        } else {
            format!("{error} - {hint}")
        };
        return ApiError::new(code, message);
    }
    let tail = err
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("nothing was said");
    ApiError::failed(format!("{program} failed ({status}): {tail}"))
}

/// Reads a pipe to its end, whatever the child meant by it.
fn drain<R: std::io::Read>(mut pipe: R) -> String {
    let mut said = String::new();
    let _ = pipe.read_to_string(&mut said);
    said
}
