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
//! every proxy checks the gate when a scheduler worker is ready to
//! start it. This matters because a proxy can already be queued when
//! a warm begins: checking only when the caller enqueues it would let
//! that queued job start under the warm. Such a worker parks the job;
//! the outermost warm release resubmits parked jobs through the same
//! worker-start check, so a new warm cannot slip into that gap either.
//! Nothing is ever interrupted: a warm never waits (the gate is not a
//! lock the warm takes; it is a flag it raises), and a proxy that has
//! passed the worker-start check is left to finish if a warm begins.
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
        // Submit outside the gate lock. A worker may start immediately
        // and must be able to take this lock for its own start-time check.
        for job in released {
            submit_proxy(job);
        }
    }
}

/// Decide whether a proxy may begin at the instant a scheduler worker
/// is ready to run it. Checking at enqueue time is not enough: the
/// scheduler queues jobs, so a warm can begin after an ask is submitted
/// but before a worker dequeues it.
fn take_or_park(job: Job) -> Option<Job> {
    let mut gate = lock();
    if gate.depth > 0 {
        gate.pending.push_back(job);
        None
    } else {
        Some(job)
    }
}

/// The scheduler's proxy-lane wrapper. Every proxy checks the gate when
/// its worker actually starts it, including jobs released by a previous
/// warm. If another warm began while the job was queued, it is parked
/// again instead of competing with the index's disk I/O.
fn guarded(job: Job) -> Job {
    Box::new(move || {
        if let Some(job) = take_or_park(job) {
            job();
        }
    })
}

/// Submit a proxy through the worker-start gate. A job that was queued
/// before a warm began is checked again when a worker dequeues it.
pub(crate) fn submit_proxy(job: Job) {
    crate::scheduler().submit(concat_media::Priority::Proxy, guarded(job));
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
        submit_proxy(Box::new(move || {
            probe.store(true, Ordering::SeqCst);
        }));
        // Drain the scheduler while the warm is held: the queued wrapper
        // must re-check the gate and park the actual work.
        crate::scheduler().drain();
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
        submit_proxy(Box::new(move || {
            probe.store(true, Ordering::SeqCst);
        }));
        crate::scheduler().drain();
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

    /// A proxy may already be queued when a warm begins. Its worker
    /// must check the gate at dequeue time, not trust the state observed
    /// by the caller when it enqueued the job.
    #[test]
    fn a_queued_proxy_rechecks_the_gate_when_its_worker_starts() {
        let _alone = for_this_test_only()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let ran = Arc::new(AtomicBool::new(false));
        let probe = ran.clone();
        let queued = guarded(Box::new(move || {
            probe.store(true, Ordering::SeqCst);
        }));

        let warm = begin_index_warm();
        // Simulate the scheduler dequeuing a job that was submitted
        // before the warm began.
        queued();
        assert_eq!(parked(), 1, "the worker parked the queued proxy");
        assert!(!ran.load(Ordering::SeqCst), "no disk work starts under the warm");

        drop(warm);
        crate::scheduler().drain();
        assert!(ran.load(Ordering::SeqCst), "the proxy ran after the warm");
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
