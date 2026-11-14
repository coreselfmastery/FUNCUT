// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The runner: an auto-edit's walk over its program, and everything the
//! walk owes the caller around it - progress events, cancellation with
//! rollback, retries, and the terminal state.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::error::{AutoEditError, ToolCall, ToolError};
use crate::stage::{EditProgram, ErrorPolicy, Stage};
use crate::state::{AutoEditEvent, AutoEditProgress, AutoEditState, RunOutcome, timestamp};

/// The tool that applies one edit. Every `edit_apply` a run makes is one
/// entry on the project's own undo history, which is what the rollback
/// steps back through.
pub const EDIT_APPLY: &str = "edit_apply";

/// The tool that steps the history back one edit. One `edit_undo` per
/// applied `edit_apply` - the same one-at-a-time step the window's undo
/// takes.
pub const EDIT_UNDO: &str = "edit_undo";

/// How many attempts a retryable call gets: the watcher's own three -
/// the first try, and two more.
const ATTEMPTS: u32 = 3;

/// The pause before the first retry. It doubles with each attempt - two
/// seconds, then four - the watcher's own cadence.
const FIRST_PAUSE: Duration = Duration::from_secs(2);

/// The idempotency key an [`EDIT_APPLY`] call carries (IDEM): derived
/// from the stage and the call's own arguments - the same edit, in a
/// re-run of the same program, asks under the same key, and the server
/// answers with the batch it already applied instead of a second copy.
/// A caller that named its own key is left alone: its name is the
/// contract, not the runner's.
fn idempotency_key(stage: Stage, call: &ToolCall) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    stage.label().hash(&mut hasher);
    call.tool.hash(&mut hasher);
    call.arguments.to_string().hash(&mut hasher);
    format!("{}-{:016x}", stage.label(), hasher.finish())
}

/// What the runner drives: one tool call at a time, answered as the JSON
/// the tool replies with.
///
/// This port exists because the `Backend` that will stand behind it
/// lives in `concat-mcp`, and `concat-mcp` is the crate that will depend
/// on this one for its wiring - the runner cannot name a type in the
/// crate that sits above it. The adapter from `Backend` to `ToolCaller`
/// is the same name-to-`Request` translation `concat-mcp`'s protocol
/// performs for `tools/call`, and it lands with the AUTO EDIT tool that
/// stands on this runner.
pub trait ToolCaller: Send {
    /// Runs one call, or refuses it the way the tool would.
    fn call(&mut self, call: &ToolCall) -> Result<Value, ToolError>;
}

/// An auto-edit's runner: the loop over an [`EditProgram`], and the
/// handle a progress reader or a cancel button holds while it runs.
///
/// A runner is cheap to clone - every clone sees the same run - and runs
/// one run at a time: a second [`run`](Runner::run) asked while the
/// first is going fails at once, without touching the running run's
/// state or events, the way a double-pressed button must.
#[derive(Clone)]
pub struct Runner {
    shared: Arc<Shared>,
}

/// What the clones share: the project, the cancel flag, the state, the
/// event channel, the last event (the one source of truth
/// [`progress`](Runner::progress) reads), the one-run latch, the stages
/// a prior run on this runner completed (IDEM's resume record), and the
/// pause before a retry.
struct Shared {
    project: PathBuf,
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<AutoEditState>>,
    events: Mutex<mpsc::Sender<AutoEditEvent>>,
    last: Mutex<Option<AutoEditEvent>>,
    running: AtomicBool,
    completed: Mutex<BTreeSet<Stage>>,
    pause: Mutex<Duration>,
}

/// Clears the one-run latch when a run ends, however it ends - a return,
/// or a panic in the runner's own frame. (A panicking tool is caught
/// where it happens; a panic between calls would be the runner's own
/// bug, and even then the runner must not answer "still running"
/// forever.)
struct RunGuard<'a>(&'a AtomicBool);

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Runner {
    /// A runner for the project at `project`, with its event channel:
    /// the receiver end is whoever is watching - a tool answering
    /// progress asks, a log, a UI. A receiver nobody reads is fine; the
    /// state and the last event are kept whatever happens to the
    /// channel.
    pub fn new(project: PathBuf) -> (Runner, mpsc::Receiver<AutoEditEvent>) {
        let (sender, receiver) = mpsc::channel();
        (
            Runner {
                shared: Arc::new(Shared {
                    project,
                    cancel: Arc::new(AtomicBool::new(false)),
                    state: Arc::new(Mutex::new(AutoEditState::Idle)),
                    events: Mutex::new(sender),
                    last: Mutex::new(None),
                    running: AtomicBool::new(false),
                    completed: Mutex::new(BTreeSet::new()),
                    pause: Mutex::new(FIRST_PAUSE),
                }),
            },
            receiver,
        )
    }

    /// The project folder - the one every rollback's `edit_undo` names.
    pub fn project(&self) -> &Path {
        &self.shared.project
    }

    /// Where the run stands right now.
    pub fn state(&self) -> AutoEditState {
        self.shared
            .state
            .lock()
            .expect("the runner's state is not poisoned")
            .clone()
    }

    /// The run's latest progress reading - the last event's own progress,
    /// the single source of truth: a poller and a watcher of the same
    /// run can never disagree. `None` before the first event.
    pub fn progress(&self) -> Option<AutoEditProgress> {
        self.shared
            .last
            .lock()
            .expect("the runner's events are not poisoned")
            .as_ref()
            .map(|event| event.progress.clone())
    }

    /// Asks the run to stop at the next stage boundary. A stage that has
    /// begun finishes what it is doing first - an edit half-applied is
    /// worse than an edit wasted - and the edits the run applied are
    /// rolled back with `edit_undo`, one per applied `edit_apply`, the
    /// history stepping back the way the window's undo steps it.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::SeqCst);
    }

    /// Whether a stop has been asked for. The ask and the stop are two
    /// moments: this says the first, the [`Cancelled`](AutoEditState::Cancelled)
    /// state says the second.
    pub fn cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::SeqCst)
    }

    /// The pause before the first retry, for a caller whose clock runs
    /// faster than a run's own - a test. It still doubles with each
    /// attempt.
    pub fn set_pause(&self, pause: Duration) {
        *self
            .shared
            .pause
            .lock()
            .expect("the runner's pause is not poisoned") = pause;
    }

    /// The stages a run on this runner completed, in the register's own
    /// order (IDEM): what a caller can hand back to [`Runner::resume`]
    /// after a restart, and what a re-run on the same runner will skip.
    /// The set lives with the runner, so a failed run's finished stages
    /// are exactly where the next run picks up.
    pub fn completed(&self) -> Vec<Stage> {
        let completed = self
            .shared
            .completed
            .lock()
            .expect("the runner's stage record is not poisoned");
        Stage::ALL
            .iter()
            .copied()
            .filter(|stage| completed.contains(stage))
            .collect()
    }

    /// Seeds the resume record: the stages a prior run already
    /// completed are skipped by the next [`run`](Runner::run) on this
    /// runner. A caller that read a run's own record - the watcher's
    /// state file, a report - resumes where that run stopped.
    pub fn resume(&self, completed: impl IntoIterator<Item = Stage>) {
        let mut set = self
            .shared
            .completed
            .lock()
            .expect("the runner's stage record is not poisoned");
        for stage in completed {
            set.insert(stage);
        }
    }

    /// A genuinely new edit: the resume record stands down, and the
    /// next [`run`](Runner::run) walks the whole program again.
    pub fn fresh(&self) {
        self.shared
            .completed
            .lock()
            .expect("the runner's stage record is not poisoned")
            .clear();
    }

    /// The pause before the attempt after the one that just failed -
    /// counted from one, doubling each time.
    fn pause_for(&self, attempt: u32) -> Duration {
        let base = *self
            .shared
            .pause
            .lock()
            .expect("the runner's pause is not poisoned");
        base * (1u32 << (attempt - 1).min(30))
    }

    /// Runs `program`, driving `caller`, and comes back with which of
    /// the four ends the run met. The details - the state, the progress,
    /// every event - live in the runner, not the outcome: they are what
    /// a progress reader, a log or a tool answers from.
    ///
    /// A call is retried when the tool is one whose retries land and the
    /// failure sounds like the network's side of the contract, up to
    /// [`ATTEMPTS`] attempts with the watcher's own pauses. A stage
    /// under the warn policy does not fail the run: its failure is
    /// recorded as a warning and the run ships
    /// [`CompletedWithWarnings`](AutoEditState::CompletedWithWarnings).
    /// A cancelled run stops at the next stage boundary and rolls its
    /// edits back. A tool that panics fails the run - the state machine
    /// must never wedge on a tool's bug - though it does not roll back;
    /// a panic in mid-edit leaves the history for a person to read.
    pub fn run(&self, program: &EditProgram, caller: &mut dyn ToolCaller) -> RunOutcome {
        if self.shared.running.swap(true, Ordering::SeqCst) {
            return RunOutcome::Failed(AutoEditError::new(
                Stage::Plan,
                None,
                ToolError {
                    message: "another run is already going on this runner".to_owned(),
                    retryable: Some(false),
                    retry_after: None,
                    code: None,
                },
            ));
        }
        let _still_running = RunGuard(&self.shared.running);

        let run_started = Instant::now();
        let started = timestamp();
        let total = program.steps.len();
        if total == 0 {
            let error = AutoEditError::new(
                Stage::Plan,
                None,
                ToolError {
                    message: "the program has no stages to run".to_owned(),
                    retryable: Some(false),
                    retry_after: None,
                    code: None,
                },
            );
            self.emit(
                AutoEditState::Failed {
                    stage: Stage::Plan,
                    error: error.clone(),
                    attempt: 1,
                },
                AutoEditProgress::new(
                    Stage::Plan,
                    0,
                    0,
                    Duration::ZERO,
                    Duration::ZERO,
                    "the program has no stages to run",
                ),
            );
            return RunOutcome::Failed(error);
        }

        // Every stage that applied an edit, in the order the edits
        // landed. Cancel rolls them back one `edit_undo` each; an
        // `edit_undo` a stage made itself comes off the list too.
        let mut applied: Vec<Stage> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let mut verification = Value::Null;

        for (index, step) in program.steps.iter().enumerate() {
            if self.cancelled() {
                return self.cancel_run(
                    caller,
                    &applied,
                    AutoEditProgress::new(
                        step.stage,
                        index,
                        total,
                        run_started.elapsed(),
                        Duration::ZERO,
                        "cancel asked for before the stage began",
                    ),
                );
            }
            // IDEM: a stage a prior run on this runner completed is
            // skipped whole - its work is already on the timeline, and
            // re-running it would duplicate every placement. The set is
            // the resume record: a failed run leaves exactly the stages
            // it finished, and the next run picks up after them.
            if self
                .shared
                .completed
                .lock()
                .expect("the runner's stage record is not poisoned")
                .contains(&step.stage)
            {
                self.emit(
                    AutoEditState::Running {
                        stage: step.stage,
                        started: started.clone(),
                    },
                    AutoEditProgress::new(
                        step.stage,
                        index,
                        total,
                        run_started.elapsed(),
                        Duration::ZERO,
                        "skipped: the stage completed in a prior run",
                    ),
                );
                continue;
            }
            let stage_started = Instant::now();
            let running = AutoEditState::Running {
                stage: step.stage,
                started: started.clone(),
            };
            self.emit(
                running.clone(),
                AutoEditProgress::new(
                    step.stage,
                    index,
                    total,
                    run_started.elapsed(),
                    Duration::ZERO,
                    "the stage began",
                ),
            );

            let mut failure: Option<AutoEditError> = None;
            let mut failed_attempt = 1;
            // IDEM: every edit this run applies carries an idempotency
            // key derived from the stage and the call itself, so the
            // same edit re-made by a run that never heard the reply
            // asks under the same key - and the server answers with the
            // batch it already applied, not a second copy.
            let calls: Vec<ToolCall> = step
                .calls
                .iter()
                .cloned()
                .map(|mut call| {
                    if call.tool == EDIT_APPLY && call.arguments.get("idempotencyKey").is_none() {
                        let key = idempotency_key(step.stage, &call);
                        if let Some(fields) = call.arguments.as_object_mut() {
                            fields.insert("idempotencyKey".to_owned(), json!(key));
                        }
                    }
                    call
                })
                .collect();
            'calls: for call in &calls {
                self.emit(
                    running.clone(),
                    AutoEditProgress::new(
                        step.stage,
                        index,
                        total,
                        run_started.elapsed(),
                        stage_started.elapsed(),
                        call.tool.as_str(),
                    ),
                );
                let mut attempt = 1;
                loop {
                    // A panicking tool must not wedge the state machine.
                    // The window's own hub catches around each API call
                    // for the same reason; here the run fails, recorded
                    // as plainly as any refusal.
                    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        caller.call(call)
                    }));
                    let made = match made {
                        Ok(made) => made,
                        Err(panic) => {
                            let what = if let Some(text) = panic.downcast_ref::<&str>() {
                                (*text).to_owned()
                            } else if let Some(text) = panic.downcast_ref::<String>() {
                                text.clone()
                            } else {
                                "without a message".to_owned()
                            };
                            failure = Some(AutoEditError::new(
                                step.stage,
                                Some(call.clone()),
                                ToolError {
                                    message: format!("the tool panicked: {what}"),
                                    retryable: Some(false),
                                    retry_after: None,
                                    code: None,
                                },
                            ));
                            failed_attempt = attempt;
                            break 'calls;
                        }
                    };
                    match made {
                        Ok(reply) => {
                            if call.tool == EDIT_APPLY {
                                applied.push(step.stage);
                            } else if call.tool == EDIT_UNDO {
                                applied.pop();
                            }
                            if step.stage == Stage::Verify {
                                verification = reply;
                            }
                            break;
                        }
                        Err(error) => {
                            let mut error =
                                AutoEditError::new(step.stage, Some(call.clone()), error);
                            // A stop asked for while the call was being
                            // made or waited on is the stop arriving: no
                            // more retries, and the run lands cancelled
                            // below rather than failed - the refusal is the
                            // stop's own work, not the tool's.
                            if self.cancelled() {
                                failed_attempt = attempt;
                                failure = Some(error);
                                break 'calls;
                            }
                            if error.retryable && attempt < ATTEMPTS {
                                let pause =
                                    error.retry_after.unwrap_or_else(|| self.pause_for(attempt));
                                std::thread::sleep(pause);
                                attempt += 1;
                                continue;
                            }
                            if error.retryable {
                                // The pause a fresh attempt would wait.
                                error.retry_after = Some(self.pause_for(attempt));
                            }
                            failed_attempt = attempt;
                            failure = Some(error);
                            break 'calls;
                        }
                    }
                }
            }

            match failure {
                None => {
                    // The stage's calls all landed: it joins the resume
                    // record, so a later run on this runner skips it
                    // (IDEM).
                    self.shared
                        .completed
                        .lock()
                        .expect("the runner's stage record is not poisoned")
                        .insert(step.stage);
                }
                // A stop asked for while the failing call was in flight:
                // the run lands cancelled, with its rollback, whatever
                // policy the stage carried - a cancel is not a warning to
                // be shipped past.
                Some(_) if self.cancelled() => {
                    return self.cancel_run(
                        caller,
                        &applied,
                        AutoEditProgress::new(
                            step.stage,
                            index,
                            total,
                            run_started.elapsed(),
                            stage_started.elapsed(),
                            "the stage stopped: a cancel was asked for",
                        ),
                    );
                }
                Some(error) if step.policy == ErrorPolicy::Warn => {
                    warnings.push(error.line());
                }
                Some(error) => {
                    self.emit(
                        AutoEditState::Failed {
                            stage: step.stage,
                            error: error.clone(),
                            attempt: failed_attempt,
                        },
                        AutoEditProgress::new(
                            step.stage,
                            index,
                            total,
                            run_started.elapsed(),
                            stage_started.elapsed(),
                            "the stage failed",
                        ),
                    );
                    return RunOutcome::Failed(error);
                }
            }
        }

        let duration = run_started.elapsed();
        let last = program.steps.last().expect("the program has stages");
        let detail = if warnings.is_empty() {
            "the export is complete"
        } else {
            "the export is complete, with warnings"
        };
        let progress =
            AutoEditProgress::new(last.stage, total - 1, total, duration, duration, detail);
        if warnings.is_empty() {
            self.emit(
                AutoEditState::Completed {
                    export: program.export.clone(),
                    duration,
                    verification: verification.clone(),
                },
                progress,
            );
            RunOutcome::Completed
        } else {
            self.emit(
                AutoEditState::CompletedWithWarnings {
                    export: program.export.clone(),
                    duration,
                    warnings,
                    verification,
                },
                progress,
            );
            RunOutcome::CompletedWithWarnings
        }
    }

    /// The cancelled end: roll the applied edits back, one `edit_undo`
    /// each, then land [`Cancelled`](AutoEditState::Cancelled). An undo
    /// that cannot be made stops the rollback and says so - a rollback
    /// that lied about reaching the end would be worse than one that
    /// stopped halfway and admitted it.
    fn cancel_run(
        &self,
        caller: &mut dyn ToolCaller,
        applied: &[Stage],
        boundary: AutoEditProgress,
    ) -> RunOutcome {
        let running = self.state();
        let mut rolled_back = true;
        let mut undone_to = None;
        for stage in applied.iter().rev() {
            let undo = ToolCall {
                tool: EDIT_UNDO.to_owned(),
                arguments: json!({ "path": self.project().display().to_string() }),
            };
            let mut note = boundary.clone();
            note.detail = format!("cancelling: {} rolled back with edit_undo", stage.label());
            self.emit(running.clone(), note);
            match caller.call(&undo) {
                Ok(_) => {
                    undone_to = Some(*stage);
                }
                Err(_) => {
                    rolled_back = false;
                    break;
                }
            }
        }
        let mut note = boundary;
        note.detail = "cancelled".to_owned();
        self.emit(
            AutoEditState::Cancelled {
                at: timestamp(),
                rolled_back,
                undone_to,
            },
            note,
        );
        RunOutcome::Cancelled
    }

    /// One event: the state and the last event move together, so a
    /// poller and a watcher can never disagree, and the channel is
    /// best-effort - a receiver nobody reads is fine, the records are
    /// kept whatever happens to it.
    fn emit(&self, state: AutoEditState, progress: AutoEditProgress) {
        let event = AutoEditEvent {
            state: state.clone(),
            progress: progress.clone(),
        };
        *self
            .shared
            .last
            .lock()
            .expect("the runner's events are not poisoned") = Some(event.clone());
        *self
            .shared
            .state
            .lock()
            .expect("the runner's state is not poisoned") = state;
        let _ = self
            .shared
            .events
            .lock()
            .expect("the runner's channel is not poisoned")
            .send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::StageStep;
    use std::collections::VecDeque;

    /// A caller that answers from a script, one reply per call, and
    /// records every call it took.
    struct Scripted {
        replies: VecDeque<Result<Value, ToolError>>,
        calls: Vec<ToolCall>,
    }

    impl Scripted {
        fn new(replies: Vec<Result<Value, ToolError>>) -> Scripted {
            Scripted {
                replies: replies.into_iter().collect(),
                calls: Vec::new(),
            }
        }
    }

    impl ToolCaller for Scripted {
        fn call(&mut self, call: &ToolCall) -> Result<Value, ToolError> {
            self.calls.push(call.clone());
            self.replies.pop_front().unwrap_or(Ok(Value::Null))
        }
    }

    /// A caller that asks the run to stop while it is answering the one
    /// tool `when` names - the way a cancel lands mid-stage.
    struct Cancelling {
        runner: Runner,
        when: &'static str,
        replies: VecDeque<Result<Value, ToolError>>,
        calls: Vec<ToolCall>,
    }

    impl ToolCaller for Cancelling {
        fn call(&mut self, call: &ToolCall) -> Result<Value, ToolError> {
            self.calls.push(call.clone());
            if call.tool == self.when {
                self.runner.cancel();
            }
            self.replies.pop_front().unwrap_or(Ok(Value::Null))
        }
    }

    /// A caller that stands still until released - a run to race
    /// against.
    struct Gated {
        gate: mpsc::Receiver<()>,
    }

    impl ToolCaller for Gated {
        fn call(&mut self, _call: &ToolCall) -> Result<Value, ToolError> {
            self.gate.recv().expect("the gate is released");
            Ok(json!({ "valid": true }))
        }
    }

    /// A caller that refuses with the message, and nothing else.
    fn refused(message: &str) -> Result<Value, ToolError> {
        Err(ToolError {
            message: message.to_owned(),
            retryable: None,
            retry_after: None,
            code: None,
        })
    }

    fn call(tool: &str) -> ToolCall {
        ToolCall {
            tool: tool.to_owned(),
            arguments: json!({ "path": "/edits/Reel" }),
        }
    }

    fn program(steps: Vec<StageStep>) -> EditProgram {
        EditProgram::new(PathBuf::from("/edits/Reel/export.mp4"), steps)
    }

    #[test]
    fn a_run_completes_and_reports_its_export() {
        let (runner, events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![
            Ok(json!({ "valid": true })),
            Ok(json!({ "job": "e1" })),
            Ok(json!({ "path": "/edits/Reel/export.mp4" })),
            Ok(json!({ "overlaps": 0, "gaps": 0 })),
        ]);
        let program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(
                Stage::Export,
                vec![call("export_start"), call("export_wait")],
            ),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&program, &mut caller), RunOutcome::Completed);
        let AutoEditState::Completed {
            export,
            duration,
            verification,
        } = runner.state()
        else {
            panic!("the run completed");
        };
        assert_eq!(export, PathBuf::from("/edits/Reel/export.mp4"));
        assert_eq!(verification, json!({ "overlaps": 0, "gaps": 0 }));
        assert!(duration >= Duration::ZERO);

        let received: Vec<AutoEditEvent> = events.try_iter().collect();
        // One event per stage began, one per call, one terminal.
        assert_eq!(received.len(), 8);
        let AutoEditState::Running { stage, .. } = &received[0].state else {
            panic!("the run began running");
        };
        assert_eq!(*stage, Stage::Plan);
        let last = received.last().expect("the terminal event came");
        assert!(matches!(last.state, AutoEditState::Completed { .. }));
        assert_eq!(last.progress.stage, Stage::Verify);
        assert_eq!(last.progress.stage_index, 2);
        assert_eq!(last.progress.total_stages, 3);
        assert_eq!(last.progress.detail, "the export is complete");
        // progress() reads the last event: the one source of truth.
        assert_eq!(runner.progress(), Some(last.progress.clone()));
    }

    #[test]
    fn a_refusal_that_is_not_the_networks_stops_at_once() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![refused("the lane is missing")]);
        let program = program(vec![StageStep::run(
            Stage::Timeline,
            vec![call("clip_add")],
        )]);
        let RunOutcome::Failed(error) = runner.run(&program, &mut caller) else {
            panic!("the run failed");
        };
        assert!(!error.retryable);
        assert_eq!(error.stage, Stage::Timeline);
        let AutoEditState::Failed { stage, attempt, .. } = runner.state() else {
            panic!("the run failed");
        };
        assert_eq!(stage, Stage::Timeline);
        assert_eq!(attempt, 1);
        assert_eq!(caller.calls.len(), 1);
    }

    #[test]
    fn a_retryable_refusal_is_tried_again() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        runner.set_pause(Duration::ZERO);
        let mut caller = Scripted::new(vec![
            refused("the connection was reset"),
            Ok(json!({ "repaired": "/tmp/take.wav" })),
        ]);
        let program = program(vec![StageStep::run(
            Stage::AudioRepair,
            vec![call("enhance_speech")],
        )]);
        assert_eq!(runner.run(&program, &mut caller), RunOutcome::Completed);
        // The same call, made twice: the first refused, the second
        // landed.
        assert_eq!(caller.calls.len(), 2);
        assert!(
            caller
                .calls
                .iter()
                .all(|call| call.tool == "enhance_speech")
        );
    }

    #[test]
    fn a_retryable_refusal_gives_up_after_three_attempts() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        runner.set_pause(Duration::ZERO);
        let mut caller = Scripted::new(vec![
            refused("the index is temporarily unreachable"),
            refused("the index is temporarily unreachable"),
            refused("the index is temporarily unreachable"),
        ]);
        let program = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("plan_validate")],
        )]);
        let RunOutcome::Failed(error) = runner.run(&program, &mut caller) else {
            panic!("the run failed");
        };
        assert!(error.retryable);
        // Three attempts, and the record says how long a fourth would
        // wait: the next step of the runner's own pause schedule, which
        // this test set to zero to make the retries instant.
        assert_eq!(caller.calls.len(), 3);
        let AutoEditState::Failed { attempt, .. } = runner.state() else {
            panic!("the run failed");
        };
        assert_eq!(attempt, 3);
        assert_eq!(error.retry_after, Some(Duration::ZERO));
    }

    #[test]
    fn the_pause_doubles_with_each_attempt_from_two_seconds() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        // The watcher's own cadence: two seconds, then four, then eight.
        assert_eq!(runner.pause_for(1), Duration::from_secs(2));
        assert_eq!(runner.pause_for(2), Duration::from_secs(4));
        assert_eq!(runner.pause_for(3), Duration::from_secs(8));
    }

    #[test]
    fn a_watched_stage_can_warn_instead_of_fail() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![
            Ok(json!({ "job": "e1" })),
            Ok(json!({ "path": "/edits/Reel/export.mp4" })),
            refused("the contact sheet is blank"),
        ]);
        let program = program(vec![
            StageStep::run(
                Stage::Export,
                vec![call("export_start"), call("export_wait")],
            ),
            StageStep::run(Stage::Watch, vec![call("craft_critique")]).warn(),
        ]);
        assert_eq!(
            runner.run(&program, &mut caller),
            RunOutcome::CompletedWithWarnings
        );
        let AutoEditState::CompletedWithWarnings {
            export,
            warnings,
            verification,
            ..
        } = runner.state()
        else {
            panic!("the run completed with warnings");
        };
        assert_eq!(export, PathBuf::from("/edits/Reel/export.mp4"));
        assert_eq!(
            warnings,
            vec!["watch craft_critique: the contact sheet is blank".to_owned()]
        );
        // No Verify stage ran: the verification record stays empty.
        assert_eq!(verification, Value::Null);
    }

    #[test]
    fn a_cancel_stops_at_the_boundary_and_rolls_edits_back() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Cancelling {
            runner: runner.clone(),
            when: "captions_add",
            replies: vec![
                Ok(json!({ "applied": true })),
                Ok(json!({ "placed": 12 })),
                Ok(json!({ "undone": true })),
            ]
            .into_iter()
            .collect(),
            calls: Vec::new(),
        };
        let program = program(vec![
            StageStep::run(Stage::Timeline, vec![call(EDIT_APPLY)]),
            StageStep::run(Stage::Captions, vec![call("captions_add")]),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&program, &mut caller), RunOutcome::Cancelled);
        let AutoEditState::Cancelled {
            rolled_back,
            undone_to,
            ..
        } = runner.state()
        else {
            panic!("the run was cancelled");
        };
        assert!(rolled_back);
        assert_eq!(undone_to, Some(Stage::Timeline));
        // The rollback named the project, and the run never reached
        // Verify.
        assert!(caller.calls.iter().any(|call| {
            call.tool == EDIT_UNDO && call.arguments == json!({ "path": "/edits/Reel" })
        }));
        assert!(!caller.calls.iter().any(|call| call.tool == "timeline_read"));
    }

    #[test]
    fn a_cancel_with_nothing_applied_has_nothing_to_undo() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Cancelling {
            runner: runner.clone(),
            when: "plan_validate",
            replies: vec![Ok(json!({ "valid": true })), Ok(json!({ "job": "e1" }))]
                .into_iter()
                .collect(),
            calls: Vec::new(),
        };
        let program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(Stage::Export, vec![call("export_start")]),
        ]);
        assert_eq!(runner.run(&program, &mut caller), RunOutcome::Cancelled);
        let AutoEditState::Cancelled {
            rolled_back,
            undone_to,
            ..
        } = runner.state()
        else {
            panic!("the run was cancelled");
        };
        assert!(rolled_back);
        assert_eq!(undone_to, None);
        assert!(!caller.calls.iter().any(|call| call.tool == EDIT_UNDO));
    }

    #[test]
    fn a_rollback_that_cannot_undo_says_so() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Cancelling {
            runner: runner.clone(),
            when: "captions_add",
            replies: vec![
                Ok(json!({ "applied": true })),
                Ok(json!({ "placed": 12 })),
                refused("the project is not open"),
            ]
            .into_iter()
            .collect(),
            calls: Vec::new(),
        };
        let program = program(vec![
            StageStep::run(Stage::Timeline, vec![call(EDIT_APPLY)]),
            StageStep::run(Stage::Captions, vec![call("captions_add")]),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&program, &mut caller), RunOutcome::Cancelled);
        let AutoEditState::Cancelled {
            rolled_back,
            undone_to,
            ..
        } = runner.state()
        else {
            panic!("the run was cancelled");
        };
        assert!(!rolled_back);
        assert_eq!(undone_to, None);
    }

    /// A stop asked for while a call is in flight - the wait a cancel
    /// aborts mid-call, `export_wait` giving up on the stop flag - lands
    /// the run cancelled, not failed: the refusal is the stop's own work,
    /// and it wins over the warn policy too, because a cancel is not a
    /// warning to be shipped past.
    #[test]
    fn a_stop_asked_mid_call_lands_cancelled_not_failed() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Cancelling {
            runner: runner.clone(),
            when: "export_wait",
            replies: vec![refused("stopped waiting; the export goes on")]
                .into_iter()
                .collect(),
            calls: Vec::new(),
        };
        let program = program(vec![
            StageStep::run(Stage::Export, vec![call("export_wait")]).warn(),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&program, &mut caller), RunOutcome::Cancelled);
        assert!(matches!(runner.state(), AutoEditState::Cancelled { .. }));
        // The run never reached the stage after the one it stopped in.
        assert!(!caller.calls.iter().any(|call| call.tool == "timeline_read"));
    }

    #[test]
    fn a_runner_runs_one_run_at_a_time() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let (release, gate) = mpsc::channel::<()>();
        let mut gated = Gated { gate };
        let on_thread = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("plan_validate")],
        )]);
        let racing = runner.clone();
        let first = std::thread::spawn(move || racing.run(&on_thread, &mut gated));
        // The first run reaches its call and stands in the gate.
        std::thread::sleep(Duration::from_millis(100));
        let second = runner.run(
            &program(vec![StageStep::run(
                Stage::Plan,
                vec![call("plan_validate")],
            )]),
            &mut Scripted::new(Vec::new()),
        );
        let RunOutcome::Failed(error) = second else {
            panic!("the second run was refused");
        };
        assert!(error.message.contains("another run is already going"));
        // And the refusal touched neither the first run's state nor its
        // events: the first run is still running.
        assert!(matches!(runner.state(), AutoEditState::Running { .. }));
        release.send(()).expect("the gate opens");
        assert_eq!(
            first.join().expect("the first run finishes"),
            RunOutcome::Completed
        );
        assert!(matches!(runner.state(), AutoEditState::Completed { .. }));
    }

    #[test]
    fn a_program_without_stages_fails_at_once() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let program = EditProgram::new(PathBuf::from("/edits/Reel/export.mp4"), Vec::new());
        let RunOutcome::Failed(error) = runner.run(&program, &mut Scripted::new(Vec::new())) else {
            panic!("the run failed");
        };
        assert_eq!(error.message, "the program has no stages to run");
        assert!(matches!(runner.state(), AutoEditState::Failed { .. }));
    }

    #[test]
    fn a_tool_that_panics_fails_the_run_rather_than_wedging_it() {
        struct Panicking;
        impl ToolCaller for Panicking {
            fn call(&mut self, _call: &ToolCall) -> Result<Value, ToolError> {
                panic!("the sense layer lost its mind");
            }
        }
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let program = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("plan_validate")],
        )]);
        let mut panicking = Panicking;
        let RunOutcome::Failed(error) = runner.run(&program, &mut panicking) else {
            panic!("the run failed");
        };
        assert!(error.message.contains("the sense layer lost its mind"));
        assert!(!error.retryable);
        // And the runner is not wedged: the next run runs.
        assert_eq!(
            runner.run(
                &program,
                &mut Scripted::new(vec![Ok(json!({ "valid": true }))])
            ),
            RunOutcome::Completed
        );
    }

    #[test]
    fn a_completed_stage_is_skipped_by_the_next_run_on_the_same_runner() {
        // IDEM: the second run over the same program makes no calls -
        // every stage completed in the first - and a fresh() runner
        // walks the program again.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(Stage::Export, vec![call("export_start")]),
        ]);
        let mut first = Scripted::new(vec![
            Ok(json!({ "valid": true })),
            Ok(json!({ "job": "e1" })),
        ]);
        assert_eq!(runner.run(&program, &mut first), RunOutcome::Completed);
        assert_eq!(first.calls.len(), 2);
        assert_eq!(runner.completed(), vec![Stage::Plan, Stage::Export]);

        // The re-run: no calls at all, still completed.
        let mut second = Scripted::new(Vec::new());
        assert_eq!(runner.run(&program, &mut second), RunOutcome::Completed);
        assert!(
            second.calls.is_empty(),
            "a completed stage re-ran: {:?}",
            second.calls
        );

        // A genuinely new edit walks the program again.
        runner.fresh();
        assert!(runner.completed().is_empty());
        let mut third = Scripted::new(vec![
            Ok(json!({ "valid": true })),
            Ok(json!({ "job": "e2" })),
        ]);
        assert_eq!(runner.run(&program, &mut third), RunOutcome::Completed);
        assert_eq!(third.calls.len(), 2);
    }

    #[test]
    fn a_failed_run_resumes_after_its_completed_stages() {
        // IDEM: the run that fails at its second stage leaves the first
        // in the resume record; the next run takes only the failed
        // stage's calls. A caller that read the record elsewhere seeds
        // it with resume() to the same effect.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(Stage::Timeline, vec![call("edit_apply")]),
        ]);
        let mut first = Scripted::new(vec![
            Ok(json!({ "valid": true })),
            refused("the lane is missing"),
        ]);
        let RunOutcome::Failed(error) = runner.run(&program, &mut first) else {
            panic!("the run failed");
        };
        assert_eq!(error.stage, Stage::Timeline);
        assert_eq!(runner.completed(), vec![Stage::Plan]);

        let mut second = Scripted::new(vec![Ok(json!({ "applied": 3 }))]);
        assert_eq!(runner.run(&program, &mut second), RunOutcome::Completed);
        assert_eq!(second.calls.len(), 1, "the completed stage re-ran");
        assert_eq!(second.calls[0].tool, "edit_apply");
        assert_eq!(runner.completed(), vec![Stage::Plan, Stage::Timeline]);

        // A fresh runner seeded by resume() skips the same way.
        let (other, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        other.resume([Stage::Plan]);
        let mut seeded = Scripted::new(vec![Ok(json!({ "applied": 3 }))]);
        assert_eq!(other.run(&program, &mut seeded), RunOutcome::Completed);
        assert_eq!(seeded.calls.len(), 1);
        assert_eq!(seeded.calls[0].tool, "edit_apply");
    }

    #[test]
    fn every_edit_apply_carries_an_idempotency_key_the_caller_did_not_name() {
        // IDEM: the runner names each edit for the server's dedupe - a
        // deterministic key over the stage and the call - and leaves a
        // caller's own name alone.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let named = ToolCall {
            tool: EDIT_APPLY.to_owned(),
            arguments: json!({ "path": "/edits/Reel", "idempotencyKey": "mine" }),
        };
        let program = program(vec![StageStep::run(
            Stage::Timeline,
            vec![call(EDIT_APPLY), named],
        )]);
        let mut caller = Scripted::new(vec![Ok(json!({})), Ok(json!({}))]);
        assert_eq!(runner.run(&program, &mut caller), RunOutcome::Completed);
        assert_eq!(caller.calls.len(), 2);
        let stamped = caller.calls[0]
            .arguments
            .get("idempotencyKey")
            .and_then(Value::as_str)
            .expect("the runner named the edit");
        assert!(
            stamped.starts_with("timeline-"),
            "the key names its stage: {stamped}"
        );
        let again = idempotency_key(
            Stage::Timeline,
            &ToolCall {
                tool: EDIT_APPLY.to_owned(),
                arguments: json!({ "path": "/edits/Reel" }),
            },
        );
        assert_eq!(stamped, again, "the key is derived, not minted");
        assert_eq!(
            caller.calls[1].arguments.get("idempotencyKey"),
            Some(&json!("mine")),
            "a caller's own key is the contract, not the runner's"
        );
    }
}
