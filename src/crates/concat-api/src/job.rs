// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! What the API remembers about its jobs: how far each running one has
//! got, how each finished one ended, and the flag that stops it.
//!
//! Events are fire-and-forget: a caller that was not listening when an
//! export ended - a script that reconnected, an agent that asks in turns
//! rather than holding a stream open - would otherwise never learn how
//! it went. The book is fed by the same events every caller is sent, so
//! [`crate::Request::ExportStatus`] and the event stream cannot disagree.
//!
//! It is also how a server that is stopping cancels the API's own jobs
//! without touching anybody else's: the export slot can be shared with a
//! window, so "cancel whatever is in the slot" could stop the window's
//! export; the flags kept here are only the API's.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::message::{ApiError, ErrorCode, Event, JobState, JobStatus};

/// How many finished jobs are remembered, oldest forgotten first.
const FINISHED_KEPT: usize = 32;

/// The running jobs' flags and every job's last known state.
#[derive(Default)]
pub(crate) struct Book {
    inner: Mutex<Inner>,
}

/// Which of the API's services a job runs in. An export holds the
/// exporter slot, and the cutout and stabilizer pools beside it, while
/// it runs; an analysis or a voice holds nothing but its own stop flag.
/// `cancel` of a job must reach the shared machinery only when the job
/// is one that holds it - the exporter slot is shared with the window,
/// and an export the window started has no name here to be cancelled
/// by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Side {
    /// The export slot, and the cutout and stabilizer pools with it.
    Export,
    /// Its own stop flag, and nothing shared.
    Own,
}

#[derive(Default)]
struct Inner {
    /// Each running job's stop flag, by name.
    running: BTreeMap<String, Arc<AtomicBool>>,
    /// Running jobs' states, by name.
    states: BTreeMap<String, JobStatus>,
    /// Which service each running job runs in, by name.
    sides: BTreeMap<String, Side>,
    /// Finished jobs, in the order they finished.
    finished: VecDeque<JobStatus>,
}

impl Book {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A job has begun, stopped by `cancel`, running in `side`'s service.
    pub(crate) fn start(&self, status: JobStatus, cancel: Arc<AtomicBool>, side: Side) {
        let mut inner = self.inner();
        inner.running.insert(status.job.clone(), cancel);
        inner.sides.insert(status.job.clone(), side);
        inner.states.insert(status.job.clone(), status);
    }

    /// Whether `job` is a running export: the one kind of job whose
    /// `cancel` must reach the exporter slot and the pools beside it.
    pub(crate) fn is_export(&self, job: &str) -> bool {
        self.inner().sides.get(job) == Some(&Side::Export)
    }

    /// Whether any running job is an export. What a shutdown asks before
    /// it cancels the cutout pool on the way out.
    pub(crate) fn exports_running(&self) -> bool {
        self.inner()
            .sides
            .values()
            .any(|side| *side == Side::Export)
    }

    /// Folds one event into its job's state. The last event of a job
    /// moves it to the finished list and lets its flag go.
    pub(crate) fn record(&self, event: &Event) {
        let mut inner = self.inner();
        let job = event.job().to_owned();
        let Some(status) = inner.states.get_mut(&job) else {
            return;
        };
        match event {
            Event::CutoutProgress { fraction, .. } => {
                status.stage = Some("cutout".to_owned());
                status.fraction = f64::from(*fraction).clamp(0.0, 1.0);
            }
            Event::StabilizeProgress { fraction, .. } => {
                status.stage = Some("stabilize".to_owned());
                status.fraction = f64::from(*fraction).clamp(0.0, 1.0);
            }
            Event::ExportProgress {
                frame,
                total,
                stage,
                ..
            } => {
                status.stage = Some(stage.clone());
                status.frame = *frame;
                status.total = *total;
                status.fraction = if *total > 0 {
                    (*frame as f64 / *total as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            Event::ExportDone {
                output,
                width,
                height,
                ..
            } => {
                status.state = JobState::Done;
                status.output = output.clone();
                status.width = Some(*width);
                status.height = Some(*height);
                status.fraction = 1.0;
            }
            Event::ExportFailed { error, .. } => {
                status.state = if error.code == ErrorCode::Cancelled {
                    JobState::Cancelled
                } else {
                    JobState::Failed
                };
                status.error = Some(error.clone());
            }
            Event::AnalysisDone { output, .. } => {
                status.state = JobState::Done;
                status.output = output.clone();
                status.fraction = 1.0;
            }
            Event::AnalysisFailed { error, .. } => {
                status.state = if error.code == ErrorCode::Cancelled {
                    JobState::Cancelled
                } else {
                    JobState::Failed
                };
                status.error = Some(error.clone());
            }
        }
        if status.state != JobState::Running {
            inner.running.remove(&job);
            inner.sides.remove(&job);
            if let Some(done) = inner.states.remove(&job) {
                inner.finished.push_back(done);
                while inner.finished.len() > FINISHED_KEPT {
                    inner.finished.pop_front();
                }
            }
        }
    }

    /// A job's thread ended without saying how - a panic in the render.
    /// Recorded as failed so a caller asking is not told "running" forever.
    pub(crate) fn abandon(&self, job: &str) {
        let running = self.inner().states.contains_key(job);
        if running {
            self.record(&Event::ExportFailed {
                job: job.to_owned(),
                path: String::new(),
                error: ApiError::failed("the job ended without a result"),
            });
        }
    }

    /// The state of `job`, running or finished.
    pub(crate) fn status(&self, job: &str) -> Option<JobStatus> {
        let inner = self.inner();
        inner.states.get(job).cloned().or_else(|| {
            inner
                .finished
                .iter()
                .rev()
                .find(|status| status.job == job)
                .cloned()
        })
    }

    /// The newest job started on `path`, running or last finished: a
    /// caller that never held a job's name - an unattended run whose
    /// program was written before the job was minted - asks for the
    /// work by what it was started on. A running job always answers
    /// before a finished one: work still going is never shadowed by
    /// work that already ended.
    pub(crate) fn status_for_path(&self, path: &str) -> Option<JobStatus> {
        let inner = self.inner();
        inner
            .states
            .values()
            .filter(|status| status.path == path)
            .max_by(|a, b| minted(&a.job).cmp(&minted(&b.job)))
            .cloned()
            .or_else(|| {
                inner
                    .finished
                    .iter()
                    .rev()
                    .find(|status| status.path == path)
                    .cloned()
            })
    }

    /// Raises one running job's stop flag: how `export.cancel` stops an
    /// analysis or a voice at its next frame. False when the job is not
    /// running.
    pub(crate) fn cancel(&self, job: &str) -> bool {
        let inner = self.inner();
        match inner.running.get(job) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Raises every running job's stop flag. How many there were.
    pub(crate) fn cancel_all(&self) -> usize {
        let inner = self.inner();
        for flag in inner.running.values() {
            flag.store(true, Ordering::Relaxed);
        }
        inner.running.len()
    }
}

/// A job name's minted number - `j12` is 12 - so "the newest" is the
/// number the mint counted, not the lexicographic order that would
/// sort `j10` before `j2`. A name that is not the mint's own shape
/// counts as zero.
fn minted(job: &str) -> u64 {
    job.strip_prefix('j')
        .and_then(|rest| rest.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn begun(book: &Book, job: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        book.start(
            JobStatus::running(job, "/p", "/o.mp4"),
            Arc::clone(&flag),
            Side::Export,
        );
        flag
    }

    #[test]
    fn a_job_is_followed_from_progress_to_its_end() {
        let book = Book::default();
        begun(&book, "j1");
        book.record(&Event::ExportProgress {
            job: "j1".to_owned(),
            path: "/p".to_owned(),
            frame: 30,
            total: 120,
            stage: "video".to_owned(),
        });
        let running = book.status("j1").expect("known");
        assert_eq!(running.state, JobState::Running);
        assert_eq!((running.frame, running.total), (30, 120));
        assert!((running.fraction - 0.25).abs() < 1e-9);
        book.record(&Event::ExportDone {
            job: "j1".to_owned(),
            path: "/p".to_owned(),
            output: "/o.mp4".to_owned(),
            width: 16,
            height: 9,
        });
        let done = book.status("j1").expect("still known");
        assert_eq!(done.state, JobState::Done);
        assert_eq!((done.width, done.height), (Some(16), Some(9)));
        assert_eq!(book.cancel_all(), 0, "a finished job has no flag to raise");
        assert!(book.status("j2").is_none());
    }

    #[test]
    fn cancel_all_raises_only_the_running_flags_and_a_cancel_is_its_own_state() {
        let book = Book::default();
        let flag = begun(&book, "j1");
        assert_eq!(book.cancel_all(), 1);
        assert!(flag.load(Ordering::Relaxed));
        book.record(&Event::ExportFailed {
            job: "j1".to_owned(),
            path: "/p".to_owned(),
            error: ApiError::new(ErrorCode::Cancelled, "The export was cancelled"),
        });
        assert_eq!(book.status("j1").expect("known").state, JobState::Cancelled);
    }

    #[test]
    fn a_job_that_vanished_is_failed_and_old_ones_are_forgotten() {
        let book = Book::default();
        begun(&book, "j0");
        book.abandon("j0");
        assert_eq!(book.status("j0").expect("known").state, JobState::Failed);
        for index in 1..=FINISHED_KEPT {
            let job = format!("j{index}");
            begun(&book, &job);
            book.abandon(&job);
        }
        assert!(book.status("j0").is_none(), "the oldest is forgotten");
        assert!(book.status(&format!("j{FINISHED_KEPT}")).is_some());
    }

    /// The ask an unattended run makes: the work, by what it was started
    /// on. A running job answers before a finished one, and of two
    /// running jobs on the same path the newer one does - by the number
    /// the mint counted, not the string order that sorts j10 before j2.
    #[test]
    fn the_newest_job_for_a_path_answers_running_first() {
        let started = |book: &Book, job: &str, path: &str| {
            book.start(
                JobStatus::running(job, path, "/o.mp4"),
                Arc::new(AtomicBool::new(false)),
                Side::Export,
            );
        };
        let book = Book::default();
        started(&book, "j1", "/edits/Reel");
        book.record(&Event::ExportDone {
            job: "j1".to_owned(),
            path: "/edits/Reel".to_owned(),
            output: "/o.mp4".to_owned(),
            width: 16,
            height: 9,
        });
        // Two running jobs on the same path: an export and an analysis,
        // the shape an unattended run and a window share. The finished j1
        // does not shadow them.
        started(&book, "j2", "/edits/Reel");
        started(&book, "j3", "/edits/Reel");
        let running = book.status_for_path("/edits/Reel").expect("a running job");
        assert_eq!(running.job, "j3");
        assert_eq!(running.state, JobState::Running);
        // A path no job was started on says so.
        assert!(book.status_for_path("/edits/Other").is_none());
        // Once both end, the newest finished one answers.
        book.record(&Event::ExportFailed {
            job: "j2".to_owned(),
            path: String::new(),
            error: ApiError::failed("the job ended without a result"),
        });
        book.record(&Event::ExportFailed {
            job: "j3".to_owned(),
            path: String::new(),
            error: ApiError::failed("the job ended without a result"),
        });
        let finished = book.status_for_path("/edits/Reel").expect("a finished job");
        assert_eq!(finished.job, "j3");
        assert_eq!(finished.state, JobState::Failed);
        // The mint's own order, not the string's: the pair string order
        // would get backwards proves the point.
        started(&book, "j9", "/edits/Reel");
        started(&book, "j10", "/edits/Reel");
        let newest = book.status_for_path("/edits/Reel").expect("a running job");
        assert_eq!(newest.job, "j10");
    }
}
