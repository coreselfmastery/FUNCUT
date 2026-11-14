// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The startup I/O ordering gate (RES-03).
//!
//! Two things the app does at start are each, on their own, the whole
//! disk's business for a while: the OpenVid index's warm - a 435 MB
//! download when it is missing, a 435 MB SQLite open with its pragmas
//! and its caption dictionary when it is not - and proxy generation,
//! one ffmpeg encode per imported file as large as HD. Left to
//! themselves they interleave: the headless server warms the index on
//! its own thread while an agent's first `project.open` queues proxies
//! on the scheduler's lanes, and on the spinning disk of the machine
//! this app is for, the two compete for the same head.
//!
//! The ordering this gate enforces is the honest one: the index warm
//! comes first. [`openvid`](crate::openvid) holds the gate for the
//! whole of a download and the whole of a first open - an RAII
//! [`IndexWarm`] token, so a warm that panics still releases - and
//! while any warm is held, [`proxy`](crate::proxy) parks the copies it
//! is asked to ensure instead of submitting them. The moment the last
//! warm finishes, everything parked is submitted on the scheduler's
//! proxy lane in the order it arrived, and proxy generation runs as it
//! always did. Nothing is ever interrupted: a warm never waits (the
//! gate is not a lock the warm takes; it is a flag it raises), and a
//! proxy already encoding when a warm begins is left to finish - the
//! disk is only ever told which of the two starts next.
//!
//! The gate is process-local and best-effort by design: it orders the
//! work this one process starts, which is the interleaving this app
//! owns; what another process does with the disk is the OS's to say.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// A parked job, in the shape the scheduler takes.
type Job = Box<dyn FnOnce() + Send>;

/// The gate's own state: how many index warms are in flight (the warm
/// paths nest - a download opens the index it fetched), and the proxy
/// jobs parked while they are.
struct Gate {
    depth: usize,
    pending: VecDeque<Job>,
}

fn gate() -> &'static Mutex<Gate> {
    static GATE: OnceLock<Mutex<Gate>> = OnceLock::new();
    GATE.get_or_init(|| {
        Mutex::new(Gate {
            depth: 0,
            pending: VecDeque::new(),
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, Gate> {
    gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The token an index warm holds, released on drop - including the
/// drop a panic unwinds into, so a warm that dies never leaves the
/// proxies parked forever.
pub struct IndexWarm {
    /// Only [`begin_index_warm`] makes one.
    _private: (),
}

/// Raise the gate: from here until the returned token drops, proxy
/// generation parks instead of starting. Reentrant - a warm that opens
/// the index inside a download nests, and the parked proxies are
/// released only when the outermost warm is done.
pub fn begin_index_warm() -> IndexWarm {
    lock().depth += 1;
    IndexWarm { _private: () }
}

impl Drop for IndexWarm {
    fn drop(&mut self) {
        let released = {
            let mut gate = lock();
            gate.depth = gate.depth.saturating_sub(1);
            if gate.depth == 0 && !gate.pending.is_empty() {
                gate.pending.drain(..).collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        // Submitted outside the lock: a job that itself asks about the
        // gate - none does today, and none may, or the release would
        // wait on itself - must never be able to hold the lock the
        // release holds.
        for job in released {
            crate::scheduler().submit(concat_media::Priority::Proxy, job);
        }
    }
}

/// Whether an index warm is in flight and proxy generation should
/// park. The tests' read of the gate; the park itself answers its own
/// question atomically (see [`park_until_index_warm`]).
#[cfg(test)]
pub(crate) fn index_warming() -> bool {
    lock().depth > 0
}

/// Park a proxy job until every in-flight index warm has finished,
/// answered as one move under the gate's lock: `None` when the job is
/// parked, `Some(job)` when no warm is in flight and the job is handed
/// back for the caller to submit as it always did. The check and the
/// park are one acquisition because they must not come apart - a warm
/// that released between a separate `index_warming` read and the park
/// would drain a queue the job had not joined yet, and the job would
/// sit parked with no warm left to ever free it. The caller has
/// already recorded the job's target in its own writing set, so a
/// second ask for the same target parks nothing.
pub(crate) fn park_until_index_warm(job: Job) -> Option<Job> {
    let mut gate = lock();
    if gate.depth == 0 {
        return Some(job);
    }
    gate.pending.push_back(job);
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The ordering the gate promises, in miniature: a job parked while
    /// a warm holds the gate does not run, and runs the moment the warm
    /// is done.
    #[test]
    fn a_parked_job_waits_for_the_warm_and_runs_when_it_ends() {
        let _alone = for_this_test_only()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let ran = Arc::new(AtomicBool::new(false));
        let probe = ran.clone();
        let warm = begin_index_warm();
        assert!(index_warming(), "the warm holds the gate");
        park_until_index_warm(Box::new(move || {
            probe.store(true, Ordering::SeqCst);
        }));
        assert_eq!(parked(), 1, "the job is parked, not running");
        assert!(!ran.load(Ordering::SeqCst), "nothing starts under the warm");
        drop(warm);
        assert!(!index_warming(), "the warm released the gate");
        assert_eq!(parked(), 0, "the parked job was submitted");
        // The scheduler's own drain waits for the submitted job; the
        // probe runs on the worker, so drain first, then read it.
        crate::scheduler().drain();
        assert!(ran.load(Ordering::SeqCst), "the released job ran");
    }

    /// Warms nest: the download holds the gate, opens the index inside
    /// it (a second hold), and the parked jobs wait for the outermost
    /// release - never the inner one.
    #[test]
    fn nested_warms_release_their_parked_jobs_only_at_the_outermost_end() {
        let _alone = for_this_test_only()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let ran = Arc::new(AtomicBool::new(false));
        let probe = ran.clone();
        let download = begin_index_warm();
        let open = begin_index_warm();
        park_until_index_warm(Box::new(move || {
            probe.store(true, Ordering::SeqCst);
        }));
        drop(open); // the inner warm ends; the download still holds
        assert!(index_warming());
        assert_eq!(parked(), 1, "still parked: the download is the outer warm");
        assert!(!ran.load(Ordering::SeqCst));
        drop(download); // the outermost warm ends; the gate opens
        assert_eq!(parked(), 0);
        crate::scheduler().drain();
        assert!(
            ran.load(Ordering::SeqCst),
            "the job ran after the outer warm"
        );
    }

    /// A park with no warm in flight is refused rather than stranded:
    /// the check and the park are one move under the gate's lock, so a
    /// warm that released between the two can never leave a job parked
    /// with no warm left to free it. The job a refused park hands back
    /// is dropped unrun here - which is also what it wants: the caller
    /// goes on to submit it on the scheduler's own lane.
    #[test]
    fn a_park_with_no_warm_in_flight_is_refused_rather_than_stranded() {
        let _alone = for_this_test_only()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        assert!(!index_warming(), "no warm is in flight");
        let refused = park_until_index_warm(Box::new(|| {
            panic!("a refused park must not run its job");
        }))
        .expect("the park was refused: no warm holds the gate");
        drop(refused);
        assert_eq!(parked(), 0, "nothing is left stranded in the queue");
    }

    /// How many jobs are parked, for the tests and for nothing else:
    /// the gate's own answer, not a promise about the scheduler.
    fn parked() -> usize {
        lock().pending.len()
    }

    /// The gate is process-global, so every test that holds it - these,
    /// and the proxy test that proves the ordering end to end - takes
    /// this lock first and runs alone. A parallel runner would otherwise
    /// answer `index_warming` with another test's warm.
    pub(crate) fn for_this_test_only() -> &'static std::sync::Mutex<()> {
        static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
        SERIAL.get_or_init(|| Mutex::new(()))
    }
}
