// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The runner: an auto-edit's walk over its program, and everything the
//! walk owes the caller around it - progress events, cancellation with
//! rollback, retries, and the terminal state.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::error::{AutoEditError, ToolCall, ToolError};
use crate::receipt::{
    AcceptanceReceipt, StageOutcome, StageRecord, derive_gates_with_stages, provenance_of,
};
use crate::stage::{EditProgram, ErrorPolicy, ProgramStep, ReplyGate, Stage};
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

/// What one driven queue of calls answers with: its failure, with the
/// attempt that failed, and every reply the queue drew.
type DriveResult = (Option<(AutoEditError, u32)>, Vec<(ToolCall, Value)>);

/// The pause before the first retry. It doubles with each attempt - two
/// seconds, then four - the watcher's own cadence.
const FIRST_PAUSE: Duration = Duration::from_secs(2);

/// The longest a retry pause may run, whatever the error asked for. A
/// tool's own `retry_after` is a hint from the far side, and a hint of
/// hours is still just a hint: the runner waits it out capped, rather
/// than wedging the run - and the runner's own cancel - behind an
/// uninterruptible sleep on the strength of a number a backend sent.
const MAX_RETRY_PAUSE: Duration = Duration::from_secs(30);

/// The slice a capped pause sleeps at a time, so a cancel lands inside
/// a pause instead of after it: the pause between retries was a plain
/// `sleep`, which never wakes for anything, and a stop asked for during
/// it was only ever seen by the next error - if there was one.
const RETRY_PAUSE_SLICE: Duration = Duration::from_millis(100);

/// The idempotency key an [`EDIT_APPLY`] call carries (IDEM): derived
/// from the key epoch, the stage and the call's own arguments - the same
/// edit, in a re-run of the same program under the same epoch, asks
/// under the same key, and the server answers with the batch it already
/// applied instead of a second copy. The epoch moves only when a
/// rollback took the run's edits back off the timeline: the ledger's
/// answer to an undone key is a no-op replay, so a retry after a
/// rollback must ask under a key it has never used, or it would ship
/// the un-edited timeline while reporting the run complete.
/// A caller that named its own key is left alone: its name is the
/// contract, not the runner's.
fn idempotency_key(epoch: u64, stage: Stage, call: &ToolCall) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    epoch.hash(&mut hasher);
    stage.label().hash(&mut hasher);
    call.tool.hash(&mut hasher);
    call.arguments.to_string().hash(&mut hasher);
    format!("{}-{:016x}", stage.label(), hasher.finish())
}

/// The one-call form of the IDEM stamping the fan and the reactive queue
/// share with the static path: an `edit_apply` without its own key gets
/// the key the epoch, the stage and the call derive.
fn stamp(epoch: u64, stage: Stage) -> impl FnMut(ToolCall) -> ToolCall {
    move |mut call| {
        if call.tool == EDIT_APPLY && call.arguments.get("idempotencyKey").is_none() {
            let key = idempotency_key(epoch, stage, &call);
            if let Some(fields) = call.arguments.as_object_mut() {
                fields.insert("idempotencyKey".to_owned(), json!(key));
            }
        }
        call
    }
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

/// Mints one caller per lane of a fan step: its own connection to the
/// same server, so a lane's wait never queues behind another lane's.
/// The factory a pane hands the runner is `open` away from a second
/// backend - the window's server takes as many connections as the run
/// asks for, and a headless lane is an in-process `Api` of its own.
///
/// A lane that cannot be minted is that lane's failure, said as a
/// sentence - not the run's: one connection refused while five lanes
/// run is one lane to retry, not a run to stop.
pub trait LaneFactory: Send + Sync {
    /// A fresh caller for one lane of a fan step.
    fn lane(&self) -> Result<Box<dyn ToolCaller>, String>;
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
/// a prior run on this runner completed with the replies that proved
/// them (IDEM's resume record), the idempotency key epoch, and the
/// pause before a retry.
struct Shared {
    project: PathBuf,
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<AutoEditState>>,
    events: Mutex<mpsc::Sender<AutoEditEvent>>,
    last: Mutex<Option<AutoEditEvent>>,
    running: AtomicBool,
    /// The resume record: a completed stage and the calls and replies
    /// that completed it. The replies ride along because a resumed
    /// run's derived stages derive from the log - a skip that replays
    /// nothing starves them, and the tighten would silently no-op on a
    /// timeline the prior run had already measured.
    completed: Mutex<BTreeMap<Stage, Vec<(ToolCall, Value)>>>,
    /// Moves only when a rollback took the run's edits back off the
    /// timeline; see [`idempotency_key`].
    key_epoch: AtomicU64,
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
                    completed: Mutex::new(BTreeMap::new()),
                    key_epoch: AtomicU64::new(1),
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The run's latest progress reading - the last event's own progress,
    /// the single source of truth: a poller and a watcher of the same
    /// run can never disagree. `None` before the first event.
    pub fn progress(&self) -> Option<AutoEditProgress> {
        self.shared
            .last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = pause;
    }

    /// The stages a run on this runner completed, in the register's own
    /// order (IDEM): what a caller can hand back to [`Runner::resume`]
    /// after a restart, and what a re-run on the same runner will skip.
    /// The record lives with the runner, so a failed run's finished stages
    /// are exactly where the next run picks up.
    pub fn completed(&self) -> Vec<Stage> {
        let completed = self
            .shared
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Stage::ALL
            .iter()
            .copied()
            .filter(|stage| completed.contains_key(stage))
            .collect()
    }

    /// Seeds the resume record: the stages a prior run already
    /// completed are skipped by the next [`run`](Runner::run) on this
    /// runner. A caller that read a run's own record - the watcher's
    /// state file, a report - resumes where that run stopped. The
    /// stages carry no replies of their own here (the caller read the
    /// names alone), so a skip of one replays nothing - the same
    /// contract a restart always had.
    pub fn resume(&self, completed: impl IntoIterator<Item = Stage>) {
        let mut set = self
            .shared
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for stage in completed {
            set.entry(stage).or_default();
        }
    }

    /// A genuinely new edit: the resume record stands down, and the
    /// next [`run`](Runner::run) walks the whole program again.
    pub fn fresh(&self) {
        self.shared
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// The pause before the attempt after the one that just failed -
    /// counted from one, doubling each time.
    fn pause_for(&self, attempt: u32) -> Duration {
        let base = *self
            .shared
            .pause
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        base * (1u32 << (attempt - 1).min(30))
    }

    /// The pause before a retry, waited out: capped at
    /// [`MAX_RETRY_PAUSE`] whatever the error asked for, and slept a
    /// [`RETRY_PAUSE_SLICE`] at a time so a cancel is seen inside it
    /// rather than after it. Returns whether the whole pause was served -
    /// `false` means the run was stopped while waiting, and the retry it
    /// was pacing never happens.
    fn wait_pause(&self, pause: Duration) -> bool {
        let mut left = pause.min(MAX_RETRY_PAUSE);
        while left > Duration::ZERO {
            if self.cancelled() {
                return false;
            }
            let slice = left.min(RETRY_PAUSE_SLICE);
            std::thread::sleep(slice);
            left -= slice;
        }
        !self.cancelled()
    }

    /// A panic in mid-edit leaves the history for a person to read.
    pub fn run(&self, program: &mut EditProgram, caller: &mut dyn ToolCaller) -> RunOutcome {
        self.walk(program, caller, None, None)
    }

    /// The same walk, with lanes for the fan steps: each group of a fan
    /// runs on its own caller, minted fresh, so the groups measure at the
    /// same time instead of one after another. A run without fan steps
    /// runs exactly as [`run`](Runner::run) - the factory is never asked.
    pub fn run_with_lanes(
        &self,
        program: &mut EditProgram,
        caller: &mut dyn ToolCaller,
        lanes: &dyn LaneFactory,
    ) -> RunOutcome {
        self.walk(program, caller, Some(lanes), None)
    }

    /// Runs with the MCP client's cancellation flag bridged into the
    /// runner after its per-run reset. The adapter also cancels the lanes'
    /// backends, so in-flight calls return and this runner can roll back.
    pub fn run_with_lanes_and_cancellation(
        &self,
        program: &mut EditProgram,
        caller: &mut dyn ToolCaller,
        lanes: &dyn LaneFactory,
        client_cancelled: &AtomicBool,
    ) -> RunOutcome {
        self.walk(program, caller, Some(lanes), Some(client_cancelled))
    }

    /// One whole walk over the program: the run's public doors above
    /// stand on.
    fn walk(
        &self,
        program: &mut EditProgram,
        caller: &mut dyn ToolCaller,
        lanes: Option<&dyn LaneFactory>,
        client_cancelled: Option<&AtomicBool>,
    ) -> RunOutcome {
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
        // A new run is a new intent: a cancel left over from the run
        // before (the retry that reuses this runner) does not stop the
        // run that was just asked for.
        self.shared.cancel.store(false, Ordering::SeqCst);
        // The client may have cancelled before the run entered this
        // method. The per-run reset must not erase that earlier ask.
        if client_cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            self.cancel();
        }

        let run_started = Instant::now();
        let started = timestamp();
        let total = program.steps.len();
        // The key epoch this run's edit stamps derive from, read once:
        // a rollback moves it between runs, never inside one.
        let epoch = self.shared.key_epoch.load(Ordering::SeqCst);
        // The resume record as this run found it: the stages a prior run
        // completed. The skip check reads this snapshot, not the live
        // record - a program with two steps of one stage would otherwise
        // skip its own second step the moment the first completed.
        let resumed: BTreeSet<Stage> = self
            .shared
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .copied()
            .collect();
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
        // landed. Cancel rolls their back one `edit_undo` each; an
        // `edit_undo` a stage made itself comes off the list too.
        let mut applied: Vec<Stage> = Vec::new();
        // Whether this run's export_start drew a reply: the stop owes that
        // render a cancellation, and nothing else.
        let mut exported = false;
        // When the last stage began: the terminal event's own stage clock
        // reads from here, not from the whole run's start.
        let mut last_stage_started = run_started;
        let mut warnings: Vec<String> = Vec::new();
        let mut verification = Value::Null;
        // Every successful reply the run has drawn, in call order: what
        // a derived step derives from, and the only memory a program's
        // later stages have of its earlier ones.
        let mut log: Vec<Value> = Vec::new();
        // Every tool call that answered, with the call itself: the
        // evidence the acceptance receipt's gates are read from, the
        // capability coverage it reports, and the asset provenance the
        // licensed imports leave behind. Kept apart from `log`, which the
        // derives read by position and must not change shape.
        let mut observed: Vec<(ToolCall, Value)> = Vec::new();
        // The receipt's stage lines, in the program's order. A stage
        // whose outcome is not yet known carries `NotReached` until the
        // walk says otherwise.
        let mut records: Vec<StageRecord> = program
            .steps
            .iter()
            .map(|step| StageRecord {
                stage: step.stage(),
                outcome: StageOutcome::NotReached,
                calls: Vec::new(),
                failure: None,
                elapsed_ms: None,
            })
            .collect();

        // The index is the one handle both the program (borrowed mutably
        // by the derived steps) and the receipt's stage records are read
        // and written through, in lockstep.
        #[allow(clippy::needless_range_loop)]
        for index in 0..program.steps.len() {
            let stage = program.steps[index].stage();
            let policy = program.steps[index].policy();
            let gates: Vec<ReplyGate> = program.steps[index].gates().to_vec();
            if self.cancelled() {
                return self.cancel_run(
                    caller,
                    &applied,
                    exported,
                    AutoEditProgress::new(
                        stage,
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
            // re-running it would duplicate every placement. The skip
            // replays the replies that completed the stage into this
            // run's own log and evidence: the derived stages that come
            // after derive from the log, and a skip that starved them
            // would tighten nothing, caption nothing and verify against
            // placements the run never read.
            if resumed.contains(&stage)
                && let Some(prior) = self
                    .shared
                    .completed
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .get(&stage)
                    .cloned()
            {
                self.emit(
                    AutoEditState::Running {
                        stage,
                        started: started.clone(),
                    },
                    AutoEditProgress::new(
                        stage,
                        index,
                        total,
                        run_started.elapsed(),
                        Duration::ZERO,
                        "skipped: the stage completed in a prior run",
                    ),
                );
                records[index].outcome = StageOutcome::Skipped;
                records[index].elapsed_ms = Some(0);
                for (call, reply) in &prior {
                    log.push(reply.clone());
                    observed.push((call.clone(), reply.clone()));
                    if !records[index].calls.contains(&call.tool) {
                        records[index].calls.push(call.tool.clone());
                    }
                    // The skip replays the stage's whole evidence, and
                    // the terminal states are part of it: a resumed run
                    // that skipped its Verify step used to complete with
                    // no verification at all (the report read empty),
                    // and a render the prior run started was not one
                    // this run could cancel - the replay carries both
                    // exactly as the run that drew them carried them.
                    if stage == Stage::Verify && (call.tool == "emotion" || verification.is_null())
                    {
                        verification = reply.clone();
                    }
                    if call.tool == "export_start" {
                        exported = true;
                    }
                }
                continue;
            }
            let stage_started = Instant::now();
            last_stage_started = stage_started;
            let running = AutoEditState::Running {
                stage,
                started: started.clone(),
            };
            self.emit(
                running.clone(),
                AutoEditProgress::new(
                    stage,
                    index,
                    total,
                    run_started.elapsed(),
                    Duration::ZERO,
                    "the stage began",
                ),
            );

            let mut failure: Option<AutoEditError> = None;
            let mut failed_attempt = 1;
            // The gate evidence: every reply this stage's calls drew, kept
            // for the gates below and dropped with them - only the Verify
            // stage's last answer travels on in `verification`.
            let mut replies: Vec<(ToolCall, Value)> = Vec::new();
            // The stage's last call, for a gate failure to name: the call
            // whose reply the gate read, whatever kind of step drew it.
            #[allow(unused_assignments)]
            let mut last_call: Option<ToolCall> = None;

            // A fan step runs its groups concurrently, one lane each;
            // everything else is one queue of calls on the one caller.
            if let ProgramStep::Fanned(fan) = &program.steps[index] {
                last_call = fan.groups.iter().flatten().last().cloned();
                // Edits do not ride a fan: the rollback order of
                // concurrent edits is not a promise the runner can keep,
                // so the program that tries is refused here rather than
                // rolled back wrongly later.
                if let Some(call) = fan
                    .groups
                    .iter()
                    .flatten()
                    .find(|call| call.tool == EDIT_APPLY || call.tool == EDIT_UNDO)
                {
                    failure = Some(AutoEditError::new(
                        stage,
                        Some(call.clone()),
                        ToolError {
                            message: "edits do not ride a fan: placements stay sequential"
                                .to_owned(),
                            retryable: Some(false),
                            retry_after: None,
                            code: None,
                        },
                    ));
                } else {
                    // Stamp every lane's calls the way the sequential
                    // path stamps its own (IDEM).
                    let groups: Vec<Vec<ToolCall>> = fan
                        .groups
                        .iter()
                        .map(|group| group.iter().cloned().map(stamp(epoch, stage)).collect())
                        .collect();
                    let lane_results: Vec<DriveResult> = match lanes {
                        Some(lanes) => std::thread::scope(|scope| {
                            let handles: Vec<_> = groups
                                .into_iter()
                                .map(|group| {
                                    let runner = self.clone();
                                    let running = running.clone();
                                    scope.spawn(move || match lanes.lane() {
                                        Ok(mut lane) => runner.drive_lane(
                                            stage,
                                            index,
                                            total,
                                            run_started,
                                            running,
                                            group,
                                            &mut lane,
                                        ),
                                        Err(sentence) => (
                                            Some((
                                                AutoEditError::new(
                                                    stage,
                                                    group.first().cloned(),
                                                    ToolError {
                                                        message: format!(
                                                            "the lane could not open: {sentence}"
                                                        ),
                                                        retryable: Some(true),
                                                        retry_after: None,
                                                        code: None,
                                                    },
                                                ),
                                                1,
                                            )),
                                            Vec::new(),
                                        ),
                                    })
                                })
                                .collect();
                            handles
                                .into_iter()
                                .map(|handle| {
                                    handle.join().unwrap_or_else(|panic| {
                                        let what = if let Some(text) = panic.downcast_ref::<&str>()
                                        {
                                            (*text).to_owned()
                                        } else if let Some(text) = panic.downcast_ref::<String>() {
                                            text.clone()
                                        } else {
                                            "without a message".to_owned()
                                        };
                                        (
                                            Some((
                                                AutoEditError::new(
                                                    stage,
                                                    None,
                                                    ToolError {
                                                        message: format!(
                                                            "the lane panicked: {what}"
                                                        ),
                                                        retryable: Some(false),
                                                        retry_after: None,
                                                        code: None,
                                                    },
                                                ),
                                                1,
                                            )),
                                            Vec::new(),
                                        )
                                    })
                                })
                                .collect()
                        }),
                        // No lanes: the groups still run, one after
                        // another on the one caller - correct, only
                        // slower.
                        None => groups
                            .into_iter()
                            .map(|group| {
                                self.drive_calls(
                                    stage,
                                    index,
                                    total,
                                    run_started,
                                    &running,
                                    &mut log,
                                    &mut applied,
                                    group,
                                    caller,
                                )
                            })
                            .collect(),
                    };
                    // The results arrive one per group, in group order -
                    // the order the no-lanes path below draws them in -
                    // so the log the merge keeps is the same log both
                    // paths leave behind.
                    for (lane_failure, lane_replies) in lane_results {
                        if let Some((error, attempt)) = lane_failure {
                            failure.get_or_insert(error);
                            failed_attempt = failed_attempt.max(attempt);
                        }
                        // The log is the memory every later derived
                        // step reads: `drive_calls` pushes each reply
                        // into it as it is drawn, and a lane's replies
                        // must land there too, in group order, or the
                        // stages after a fan step see nothing the fan
                        // measured. This was the difference between
                        // lanes and no lanes: a run whose per-take
                        // transcribes and voice signals rode lanes
                        // produced an edit with no captions, no B-roll
                        // and no accents, because the readers those
                        // stages spend looked at a log the lanes never
                        // wrote.
                        log.extend(lane_replies.iter().map(|(_, reply)| reply.clone()));
                        replies.extend(lane_replies);
                    }
                }
            } else {
                // IDEM: every edit this run applies carries an
                // idempotency key derived from the stage and the call
                // itself, so the same edit re-made by a run that never
                // heard the reply asks under the same key - and the
                // server answers with the batch it already applied, not
                // a second copy.
                match &mut program.steps[index] {
                    ProgramStep::Static(step) => {
                        let calls: Vec<ToolCall> = step
                            .calls
                            .clone()
                            .into_iter()
                            .map(stamp(epoch, stage))
                            .collect();
                        last_call = calls.last().cloned();
                        let (walk_failure, walk_replies) = self.drive_calls(
                            stage,
                            index,
                            total,
                            run_started,
                            &running,
                            &mut log,
                            &mut applied,
                            calls,
                            caller,
                        );
                        if let Some((error, attempt)) = walk_failure {
                            failure = Some(error);
                            failed_attempt = attempt;
                        }
                        replies = walk_replies;
                    }
                    // The reactive queue: asked for its next call with the
                    // log the run has drawn so far - its own replies
                    // included - until it answers None. Each call is
                    // stamped and driven exactly as a static one is.
                    // The queue is bounded: a closure that answered a call
                    // forever would spin the runner without one, and the
                    // bound turns that into the stage's own failure
                    // under its policy instead of a run that never ends.
                    ProgramStep::Derived(derived) => {
                        let mut derived_calls = 0usize;
                        loop {
                            derived_calls += 1;
                            if derived_calls > 10_000 {
                                failure.get_or_insert(AutoEditError::new(
                                    stage,
                                    last_call.clone(),
                                    ToolError {
                                        message: "the derived step never ran out of calls"
                                            .to_owned(),
                                        retryable: Some(false),
                                        retry_after: None,
                                        code: None,
                                    },
                                ));
                                failed_attempt = 1;
                                break;
                            }
                            let next = (derived.next)(&log);
                            let Some(next) = next else {
                                break;
                            };
                            let call = stamp(epoch, stage)(next);
                            last_call = Some(call.clone());
                            let (walk_failure, walk_replies) = self.drive_calls(
                                stage,
                                index,
                                total,
                                run_started,
                                &running,
                                &mut log,
                                &mut applied,
                                vec![call],
                                caller,
                            );
                            replies.extend(walk_replies);
                            if let Some((error, attempt)) = walk_failure {
                                failure = Some(error);
                                failed_attempt = attempt;
                                break;
                            }
                        }
                    }
                    ProgramStep::Fanned(_) => unreachable!("the fan ran above"),
                }
            }
            for (call, reply) in &replies {
                if stage == Stage::Verify {
                    // The words comparison is the verification the run's
                    // terminal states carry - not the last reply the
                    // stage happened to make (the motion probe), which
                    // used to overwrite the verdict, matchRate and drift
                    // the report is read for.
                    //
                    // The first answer of either shape stands: the
                    // emotion verify itself, or the first reply when no
                    // verify ever ran - and a later emotion reply
                    // replaces it, because the verify is the one the
                    // report is read for.
                    if call.tool == "emotion" || verification.is_null() {
                        verification = reply.clone();
                    }
                }
                if call.tool == "export_start" {
                    exported = true;
                }
                observed.push((call.clone(), reply.clone()));
                if !records[index].calls.contains(&call.tool) {
                    records[index].calls.push(call.tool.clone());
                }
            }

            // The silent-success gates: a stage whose calls all landed
            // has still not finished until its replies said what the
            // program demands of them. A gate no reply answered is the
            // stage's own failure - a verify nobody ran is not a verify -
            // and a reply that failed it names the value it saw. The
            // failure lands under the stage's own policy, like any call's.
            if failure.is_none() && !gates.is_empty() {
                for gate in &gates {
                    let mut answered = false;
                    let mut failed = None;
                    // Repeated identical calls are polls of one job: an
                    // earlier poll's "still running" is progress, not the
                    // job's answer, so only the LAST reply of each distinct
                    // call is judged - a lane's two chained waits cover a
                    // budget the first wait alone cannot, and the first
                    // wait's "running" was failing the whole stage after
                    // the work was already paid for. Distinct calls - one
                    // per fan lane, one per question - are each judged by
                    // their own final answer, so a lane that never
                    // finished still fails its gate.
                    let mut last_of_call: Vec<(String, &Value)> = Vec::new();
                    for (call, reply) in &replies {
                        let key = format!("{}\u{1}{}", call.tool, call.arguments);
                        match last_of_call.iter_mut().find(|(k, _)| *k == key) {
                            Some(slot) => slot.1 = reply,
                            None => last_of_call.push((key, reply)),
                        }
                    }
                    for reply in last_of_call.iter().map(|(_, reply)| *reply) {
                        if let Some(answer) = gate.checked_by(reply) {
                            answered = true;
                            if let Err(sentence) = answer {
                                failed.get_or_insert(sentence);
                                break;
                            }
                        }
                    }
                    if !answered {
                        failed.get_or_insert(format!(
                            "the stage's replies never answered the {} gate: \
                             the call that proves it did not run or did not say",
                            gate.field()
                        ));
                    }
                    if let Some(sentence) = failed {
                        failure = Some(AutoEditError::new(
                            stage,
                            last_call.clone(),
                            ToolError {
                                message: sentence,
                                retryable: Some(false),
                                retry_after: None,
                                code: None,
                            },
                        ));
                        failed_attempt = 1;
                        break;
                    }
                }
            }

            let stage_elapsed_ms = stage_started.elapsed().as_millis() as u64;
            match failure {
                None => {
                    // The stage's calls all landed: it joins the resume
                    // record with the replies that proved it, so a later
                    // run on this runner skips it (IDEM) and replays its
                    // evidence into its own log.
                    records[index].outcome = StageOutcome::Completed;
                    records[index].elapsed_ms = Some(stage_elapsed_ms);
                    self.shared
                        .completed
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(stage, replies.clone());
                }
                // A stop asked for while the failing call was in flight:
                // the run lands cancelled, with its rollback, whatever
                // policy the stage carried - a cancel is not a warning to
                // be shipped past.
                Some(_) if self.cancelled() => {
                    return self.cancel_run(
                        caller,
                        &applied,
                        exported,
                        AutoEditProgress::new(
                            stage,
                            index,
                            total,
                            run_started.elapsed(),
                            stage_started.elapsed(),
                            "the stage stopped: a cancel was asked for",
                        ),
                    );
                }
                Some(error) if policy == ErrorPolicy::Warn => {
                    records[index].outcome = StageOutcome::Warned;
                    records[index].elapsed_ms = Some(stage_elapsed_ms);
                    records[index].failure = Some(error.line());
                    warnings.push(error.line());
                }
                Some(error) => {
                    records[index].outcome = StageOutcome::Failed;
                    records[index].elapsed_ms = Some(stage_elapsed_ms);
                    records[index].failure = Some(error.line());
                    self.emit(
                        AutoEditState::Failed {
                            stage,
                            error: error.clone(),
                            attempt: failed_attempt,
                        },
                        AutoEditProgress::new(
                            stage,
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
        let stage_duration = last_stage_started.elapsed();
        let last = program.steps.last().expect("the program has stages");
        // The acceptance receipt: the run's own evidence, read from the
        // replies its calls drew. A stage that never got its turn stays
        // `NotReached`, and a gate whose measurement never landed is
        // `Skipped` - never a pass the run did not earn.
        let gates = derive_gates_with_stages(&observed, &records);
        let receipt = AcceptanceReceipt::new(
            self.project().display().to_string(),
            program.export.display().to_string(),
            records,
            gates,
            warnings.clone(),
        )
        .with_assets(provenance_of(&observed));
        // A completed stage sequence is not an accepted edit. Keep its
        // editable project or explicit export for inspection, but force
        // the warning terminal state whenever any required quality gate
        // is rejected or unmeasured. The receipt keeps stage warnings
        // alone; this sentence is the receipt's verdict, not a warning it
        // recorded.
        if program.enforce_acceptance && receipt.verdict != crate::receipt::Verdict::Accepted {
            warnings.push(format!(
                "quality acceptance {}: {}",
                receipt.verdict.label(),
                receipt.summary()
            ));
        }
        let timeline_delivery = program.export.as_path() == std::path::Path::new("timeline");
        let detail = match (timeline_delivery, warnings.is_empty()) {
            (true, true) => "the editable timeline is ready",
            (true, false) => "the editable timeline is ready, with warnings",
            (false, true) => "the export is complete",
            (false, false) => "the export is complete, with warnings",
        };
        let progress = AutoEditProgress::new(
            last.stage(),
            total - 1,
            total,
            duration,
            stage_duration,
            detail,
        );
        if warnings.is_empty() {
            self.emit(
                AutoEditState::Completed {
                    export: program.export.clone(),
                    duration,
                    verification: verification.clone(),
                    receipt: Some(Box::new(receipt)),
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
                    receipt: Some(Box::new(receipt)),
                },
                progress,
            );
            RunOutcome::CompletedWithWarnings
        }
    }

    /// Drives one queue of calls on one caller: the retry loop, the
    /// panic catch, the applied-edits stack and the reply log, shared by
    /// the sequential stages and a fan without lanes. The replies come
    /// back for the gates; the log grows in place.
    fn drive_calls(
        &self,
        stage: Stage,
        index: usize,
        total: usize,
        run_started: Instant,
        running: &AutoEditState,
        log: &mut Vec<Value>,
        applied: &mut Vec<Stage>,
        calls: Vec<ToolCall>,
        caller: &mut dyn ToolCaller,
    ) -> DriveResult {
        let mut failure: Option<AutoEditError> = None;
        let mut failed_attempt = 1;
        let mut replies: Vec<(ToolCall, Value)> = Vec::new();
        'calls: for call in &calls {
            self.emit(
                running.clone(),
                AutoEditProgress::new(
                    stage,
                    index,
                    total,
                    run_started.elapsed(),
                    Duration::ZERO,
                    call.tool.as_str(),
                ),
            );
            let mut attempt = 1;
            loop {
                // A panicking tool must not wedge the state machine.
                // The window's own hub catches around each API call
                // for the same reason; here the run fails, recorded
                // as plainly as any refusal.
                let made =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| caller.call(call)));
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
                            stage,
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
                            applied.push(stage);
                        } else if call.tool == EDIT_UNDO {
                            applied.pop();
                        }
                        replies.push((call.clone(), reply.clone()));
                        log.push(reply);
                        break;
                    }
                    Err(error) => {
                        let mut error = AutoEditError::new(stage, Some(call.clone()), error);
                        // A stop asked for while the call was being
                        // made, waited on or retried is the stop
                        // arriving: no more retries - not even the one
                        // the pause was being waited out for - and the
                        // run lands cancelled below rather than failed.
                        if self.cancelled() {
                            failed_attempt = attempt;
                            failure = Some(error);
                            break 'calls;
                        }
                        if error.retryable && attempt < ATTEMPTS {
                            let pause =
                                error.retry_after.unwrap_or_else(|| self.pause_for(attempt));
                            if !self.wait_pause(pause) {
                                // A stop that landed inside the pause is the
                                // stop arriving: the retry it was waiting for
                                // never happens.
                                failed_attempt = attempt;
                                failure = Some(error);
                                break 'calls;
                            }
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
        (failure.map(|error| (error, failed_attempt)), replies)
    }

    /// Drives one lane of a fan: its own caller, its own replies, the
    /// same retry loop - but no applied-edits stack, because a fan's
    /// calls are reads and waits (the runner refuses the edits before
    /// any lane starts).
    fn drive_lane(
        &self,
        stage: Stage,
        index: usize,
        total: usize,
        run_started: Instant,
        running: AutoEditState,
        calls: Vec<ToolCall>,
        caller: &mut Box<dyn ToolCaller>,
    ) -> DriveResult {
        let mut failure: Option<AutoEditError> = None;
        let mut failed_attempt = 1;
        let mut replies: Vec<(ToolCall, Value)> = Vec::new();
        'calls: for call in &calls {
            // A stop asked for between a lane's calls is the stop
            // arriving AS a failure, not a silent break: a lane that
            // merely stopped would let its stage join the resume record
            // half-measured, and the next run would skip the reads it
            // never made.
            if self.cancelled() {
                failure = Some(AutoEditError::new(
                    stage,
                    Some(call.clone()),
                    ToolError {
                        message: "the lane stopped: a cancel was asked for".to_owned(),
                        retryable: Some(false),
                        retry_after: None,
                        code: None,
                    },
                ));
                break 'calls;
            }
            self.emit(
                running.clone(),
                AutoEditProgress::new(
                    stage,
                    index,
                    total,
                    run_started.elapsed(),
                    Duration::ZERO,
                    call.tool.as_str(),
                ),
            );
            let mut attempt = 1;
            loop {
                let made =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| caller.call(call)));
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
                            stage,
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
                        replies.push((call.clone(), reply));
                        break;
                    }
                    Err(error) => {
                        let mut error = AutoEditError::new(stage, Some(call.clone()), error);
                        if self.cancelled() {
                            failed_attempt = attempt;
                            failure = Some(error);
                            break 'calls;
                        }
                        if error.retryable && attempt < ATTEMPTS {
                            let pause =
                                error.retry_after.unwrap_or_else(|| self.pause_for(attempt));
                            if !self.wait_pause(pause) {
                                // A stop that landed inside the pause is the
                                // stop arriving: the retry it was waiting for
                                // never happens.
                                failed_attempt = attempt;
                                failure = Some(error);
                                break 'calls;
                            }
                            attempt += 1;
                            continue;
                        }
                        if error.retryable {
                            error.retry_after = Some(self.pause_for(attempt));
                        }
                        failed_attempt = attempt;
                        failure = Some(error);
                        break 'calls;
                    }
                }
            }
        }
        (failure.map(|error| (error, failed_attempt)), replies)
    }

    /// The cancelled end: roll the applied edits back, one `edit_undo`
    /// each, then land [`Cancelled`](AutoEditState::Cancelled). An undo
    /// that cannot be made stops the rollback and says so - a rollback
    /// that lied about reaching the end would be worse than one that
    /// stopped halfway and admitted it.
    ///
    /// When the run had started its render, the render is cancelled
    /// first, best-effort: an export left burning past the stop wastes
    /// the machine for the rest of the render and holds the export slot
    /// the retry's own `export_start` needs. A render that was never
    /// started is not asked.
    fn cancel_run(
        &self,
        caller: &mut dyn ToolCaller,
        applied: &[Stage],
        exported: bool,
        boundary: AutoEditProgress,
    ) -> RunOutcome {
        let running = self.state();
        let mut rolled_back = true;
        let mut undone_to = None;
        if exported {
            // Best-effort: a refusal here (the render already ended, the
            // tool unserved) is not the stop's failure to report - the
            // render ends on its own and the retry's own export_start
            // names a slot it cannot have.
            let _ = caller.call(&ToolCall {
                tool: "export_cancel".to_owned(),
                arguments: json!({ "path": self.project().display().to_string() }),
            });
        }
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
        // The resume record owes the rollback its truth: a stage whose
        // edits were just stepped back is not a stage the next run may
        // skip, or the resume would ship a timeline the rollback took
        // apart. The read-only stages stay - their work lives in the
        // analysis registry, not on the timeline - but every stage that
        // applied an edit and had it undone comes off the record, whole.
        // A partial rollback (the record says so) takes the same stages
        // off: a half-undone stage re-run whole is IDEM's own answer.
        //
        // The key epoch moves with the same truth: the ledger's answer
        // to an undone key is a no-op replay, so the retry must mint its
        // keys from an epoch the ledger has never seen - or it would
        // "resume" a stage whose edits the rollback just removed, and
        // the export would ship the un-edited timeline as complete.
        if !applied.is_empty() {
            self.shared.key_epoch.fetch_add(1, Ordering::SeqCst);
        }
        let mut completed = self
            .shared
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for stage in applied {
            completed.remove(stage);
        }
        drop(completed);
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
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(event.clone());
        *self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
        let _ = self
            .shared
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::{DerivedStep, FanStep, ReplyGate, StageStep};
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

    fn call_with(tool: &str, arguments: Value) -> ToolCall {
        ToolCall {
            tool: tool.to_owned(),
            arguments,
        }
    }

    fn program(steps: Vec<StageStep>) -> EditProgram {
        EditProgram::new(PathBuf::from("/edits/Reel/export.mp4"), steps)
    }

    #[test]
    fn a_client_cancelled_before_the_walk_is_not_erased_by_the_run_reset() {
        struct UnusedLanes;
        impl LaneFactory for UnusedLanes {
            fn lane(&self) -> Result<Box<dyn ToolCaller>, String> {
                Err("no fan was expected".to_owned())
            }
        }

        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let cancelled = AtomicBool::new(true);
        let mut caller = Scripted::new(vec![]);
        let mut program = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("must_not_run")],
        )]);

        let outcome = runner.run_with_lanes_and_cancellation(
            &mut program,
            &mut caller,
            &UnusedLanes,
            &cancelled,
        );
        assert_eq!(outcome, RunOutcome::Cancelled);
        assert!(caller.calls.is_empty(), "a cancelled run made a tool call");
        assert!(matches!(runner.state(), AutoEditState::Cancelled { .. }));
    }

    #[test]
    fn a_derived_step_derives_its_calls_from_the_earlier_replies() {
        // The tighten that reads the voiceSignals reply: the second stage
        // builds its calls from what the first one measured, and the run
        // drives them exactly as it drives the static ones.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![
            Ok(json!({ "pauses": [
                { "start": 4.0, "end": 7.0, "class": "dead_air", "keep": false },
            ]})),
            Ok(json!({ "cut": 1 })),
        ]);
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![
                StageStep::run(Stage::VoiceSignals, vec![call("voice_signals")]).into(),
                DerivedStep::react(
                    Stage::Timeline,
                    Box::new({
                        let mut issued = 0;
                        move |log| {
                            let mut cuts = Vec::new();
                            for reply in log {
                                if let Some(pauses) = reply.get("pauses").and_then(Value::as_array)
                                {
                                    for pause in pauses {
                                        if pause.get("class") == Some(&json!("dead_air")) {
                                            cuts.push(call_with(
                                                "clip_cut_spans",
                                                json!({ "spans": [[
                                                    pause["start"], pause["end"]
                                                ]]}),
                                            ));
                                        }
                                    }
                                }
                            }
                            let next = issued < cuts.len();
                            issued += 1;
                            next.then(|| cuts[issued - 1].clone())
                        }
                    }),
                )
                .into(),
            ],
        );
        let outcome = runner.run(&mut program, &mut caller);
        assert_eq!(outcome, RunOutcome::Completed);
        // The derived call names the measured span, not a guess.
        assert_eq!(caller.calls.len(), 2);
        assert_eq!(caller.calls[1].tool, "clip_cut_spans");
        assert_eq!(
            caller.calls[1].arguments["spans"][0],
            json!([4.0, 7.0]),
            "the derived call carries the reply's own numbers"
        );
        // Both stages joined the resume record.
        let AutoEditState::Completed { .. } = runner.state() else {
            panic!("the run completed");
        };
    }

    #[test]
    fn a_derived_step_that_finds_nothing_completes_the_stage() {
        // No dead air is a tighten that succeeded, not one that failed.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![Ok(json!({ "pauses": [] }))]);
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![
                StageStep::run(Stage::VoiceSignals, vec![call("voice_signals")]).into(),
                DerivedStep::react(
                    Stage::Timeline,
                    Box::new(|log| {
                        let dead_air = log
                            .iter()
                            .filter_map(|reply| reply.get("pauses").and_then(Value::as_array))
                            .flatten()
                            .any(|pause| pause.get("class") == Some(&json!("dead_air")));
                        dead_air.then(|| call("clip_cut_spans"))
                    }),
                )
                .into(),
            ],
        );
        let outcome = runner.run(&mut program, &mut caller);
        assert_eq!(outcome, RunOutcome::Completed);
        assert_eq!(caller.calls.len(), 1, "the derived stage made no calls");
    }

    #[test]
    fn a_fan_step_runs_its_groups_and_merges_the_replies() {
        // Two takes, two lanes: both groups run, and the replies of both
        // reach the gate check together.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(Vec::new());
        let lanes = FanLanes::default();
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![
                FanStep::fan(
                    Stage::Transcribe,
                    vec![
                        vec![call_with("transcribe", json!({ "path": "/takes/one.wav" }))],
                        vec![call_with("transcribe", json!({ "path": "/takes/two.wav" }))],
                    ],
                )
                .gate(ReplyGate::AtLeast {
                    field: "segmentCount",
                    minimum: 1.0,
                    what: "a lane transcribed nothing",
                })
                .into(),
            ],
        );
        lanes.seed(vec![
            vec![Ok(json!({ "segments": [{}], "segmentCount": 1 }))],
            vec![Ok(json!({ "segments": [{}, {}], "segmentCount": 2 }))],
        ]);
        let outcome = runner.run_with_lanes(&mut program, &mut caller, &lanes);
        assert_eq!(outcome, RunOutcome::Completed);
        assert_eq!(lanes.asked(), 2, "one lane per group was minted");
        // The one caller was never asked: the lanes did the work.
        assert_eq!(caller.calls.len(), 0);
    }

    #[test]
    fn a_fan_step_without_lanes_still_runs_on_the_one_caller() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![
            Ok(json!({ "segments": [{}] })),
            Ok(json!({ "segments": [{}, {}] })),
        ]);
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![
                FanStep::fan(
                    Stage::Transcribe,
                    vec![
                        vec![call_with("transcribe", json!({ "path": "/takes/one.wav" }))],
                        vec![call_with("transcribe", json!({ "path": "/takes/two.wav" }))],
                    ],
                )
                .into(),
            ],
        );
        let outcome = runner.run(&mut program, &mut caller);
        assert_eq!(outcome, RunOutcome::Completed);
        assert_eq!(caller.calls.len(), 2, "both groups ran, in order");
        assert_eq!(caller.calls[0].arguments["path"], "/takes/one.wav");
        assert_eq!(caller.calls[1].arguments["path"], "/takes/two.wav");
    }

    #[test]
    fn a_fan_step_on_lanes_leaves_its_replies_in_the_log() {
        // The regression this test holds: a fan stage's lane replies must
        // reach the log, in group order, the way the one-caller path's
        // replies do - because every later derived step reads the log,
        // not the gate evidence. A run whose per-take transcribes rode
        // lanes used to hand the caption stage an empty log, and the
        // button shipped a video with no captions, no B-roll and no
        // accents while the same program without lanes edited fully.
        //
        // The lane answers from the call it was handed - not from a
        // pre-seeded script - because which lane serves which group is
        // the scheduler's choice, and the log's order must be provable
        // whatever it chooses.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(Vec::new());
        let lanes = EchoLanes;
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![
                FanStep::fan(
                    Stage::Transcribe,
                    vec![
                        vec![call_with("transcribe", json!({ "path": "one" }))],
                        vec![call_with("transcribe", json!({ "path": "two" }))],
                    ],
                )
                .into(),
                // A reader of the fan's own output: one call per take the
                // lanes transcribed, in group order, each carrying its
                // take's own marker so the order is provable.
                DerivedStep::react(
                    Stage::Timeline,
                    Box::new({
                        let mut issued = 0;
                        move |log| {
                            let takes: Vec<&str> = log
                                .iter()
                                .filter_map(|reply| reply.get("take").and_then(Value::as_str))
                                .collect();
                            let next = issued < takes.len();
                            issued += 1;
                            next.then(|| {
                                call_with("clip_cut_spans", json!({ "take": takes[issued - 1] }))
                            })
                        }
                    }),
                )
                .into(),
            ],
        );
        let outcome = runner.run_with_lanes(&mut program, &mut caller, &lanes);
        assert_eq!(outcome, RunOutcome::Completed);
        // The derived stage read BOTH lane replies - not only the first,
        // and none of them is invisible either.
        assert_eq!(caller.calls.len(), 2, "both takes reached the reader");
        assert_eq!(
            caller.calls[0].arguments["take"], "one",
            "group order is the log order"
        );
        assert_eq!(caller.calls[1].arguments["take"], "two");
    }

    #[test]
    fn edits_do_not_ride_a_fan() {
        // The rollback order of concurrent edits is not a promise the
        // runner can keep: the program that tries is refused.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(Vec::new());
        let lanes = FanLanes::default();
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![FanStep::fan(Stage::Timeline, vec![vec![call("edit_apply")]]).into()],
        );
        let RunOutcome::Failed(error) = runner.run_with_lanes(&mut program, &mut caller, &lanes)
        else {
            panic!("the run failed");
        };
        assert_eq!(error.stage, Stage::Timeline);
        assert!(
            error.message.contains("edits do not ride a fan"),
            "the refusal names the law: {}",
            error.message
        );
    }

    #[test]
    fn a_lane_that_cannot_open_is_the_lanes_failure() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(Vec::new());
        let lanes = RefusingLanes;
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![FanStep::fan(Stage::Transcribe, vec![vec![call("transcribe")]]).into()],
        );
        let RunOutcome::Failed(error) = runner.run_with_lanes(&mut program, &mut caller, &lanes)
        else {
            panic!("the run failed");
        };
        assert!(
            error.message.contains("the lane could not open"),
            "the failure names the lane: {}",
            error.message
        );
    }

    /// A lane factory whose scripts are seeded per lane before the run;
    /// each minted lane drains its own script.
    #[derive(Default)]
    struct FanLanes {
        scripts: Mutex<std::collections::VecDeque<Vec<Result<Value, ToolError>>>>,
        minted: std::sync::atomic::AtomicUsize,
    }

    /// A lane factory whose caller answers from the call it was handed,
    /// echoing the call's own `path` back as `take`. Which lane serves
    /// which group is the scheduler's choice, so a test that needs each
    /// group's own answer in the log cannot pre-seed scripts: the answer
    /// must derive from the call, as the real server's replies do.
    struct EchoLanes;

    impl LaneFactory for EchoLanes {
        fn lane(&self) -> Result<Box<dyn ToolCaller>, String> {
            Ok(Box::new(Echoing))
        }
    }

    /// The caller `EchoLanes` mints: every call is answered with its own
    /// `path`, so a group's reply is identifiable no matter which lane
    /// drew it.
    struct Echoing;

    impl ToolCaller for Echoing {
        fn call(&mut self, call: &ToolCall) -> Result<Value, ToolError> {
            Ok(json!({
                "take": call.arguments.get("path").cloned().unwrap_or(Value::Null),
                "segments": [{}],
            }))
        }
    }

    impl FanLanes {
        fn seed(&self, scripts: Vec<Vec<Result<Value, ToolError>>>) {
            *self.scripts.lock().unwrap() = scripts.into_iter().collect();
        }
        fn asked(&self) -> usize {
            self.minted.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl LaneFactory for FanLanes {
        fn lane(&self) -> Result<Box<dyn ToolCaller>, String> {
            self.minted
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let script = self
                .scripts
                .lock()
                .unwrap()
                .pop_front()
                .ok_or("no script left")?;
            Ok(Box::new(Draining::new(script)))
        }
    }

    /// A caller that drains a fixed script and then answers nulls.
    struct Draining {
        replies: std::collections::VecDeque<Result<Value, ToolError>>,
    }

    impl Draining {
        fn new(replies: Vec<Result<Value, ToolError>>) -> Draining {
            Draining {
                replies: replies.into_iter().collect(),
            }
        }
    }

    impl ToolCaller for Draining {
        fn call(&mut self, _call: &ToolCall) -> Result<Value, ToolError> {
            self.replies.pop_front().unwrap_or(Ok(Value::Null))
        }
    }

    /// A lane factory that cannot mint anything.
    struct RefusingLanes;

    impl LaneFactory for RefusingLanes {
        fn lane(&self) -> Result<Box<dyn ToolCaller>, String> {
            Err("the connection refused".to_owned())
        }
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
        let mut program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(
                Stage::Export,
                vec![call("export_start"), call("export_wait")],
            ),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Completed);
        let AutoEditState::Completed {
            export,
            duration,
            verification,
            receipt,
        } = runner.state()
        else {
            panic!("the run completed");
        };
        assert_eq!(export, PathBuf::from("/edits/Reel/export.mp4"));
        assert_eq!(verification, json!({ "overlaps": 0, "gaps": 0 }));
        assert!(duration >= Duration::ZERO);
        // The receipt is written from the run's own replies: the
        // timeline read is the only measurement, so the gates that read
        // a plan, a mix or the file are skipped, not passed.
        let receipt = receipt.expect("a completed run carries its receipt");
        assert_eq!(receipt.schema, crate::receipt::SCHEMA);
        assert_eq!(receipt.stages.len(), 3);
        assert!(receipt.stages.iter().all(|stage| stage.outcome.landed()));
        assert!(
            receipt
                .stages
                .iter()
                .all(|stage| stage.elapsed_ms.is_some()),
            "every stage that ran reports its wall-clock time"
        );
        assert!(receipt.capabilities.iter().any(|area| area.consumed));

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
    fn an_acceptance_claiming_program_cannot_report_a_clean_completion() {
        // The auto-edit spine claims the whole edit, so a receipt with a
        // required gate left unmeasured is a warned completion, not a
        // clean one. A partial program records the same receipt without
        // changing its outcome - the tests above depend on that.
        let (runner, events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![Ok(json!({ "valid": true }))]);
        let mut program = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("plan_validate")],
        )])
        .enforcing_acceptance();
        assert_eq!(
            runner.run(&mut program, &mut caller),
            RunOutcome::CompletedWithWarnings
        );
        let AutoEditState::CompletedWithWarnings { warnings, .. } = runner.state() else {
            panic!("the run completed with warnings");
        };
        assert!(
            warnings.iter().any(|warning| {
                warning.starts_with("quality acceptance ")
                    && warning.contains("required gate(s) unmeasured")
            }),
            "{warnings:?}"
        );
        // The progress line the window shows says the same thing as the
        // state: the acceptance warning is not bolted on after the
        // terminal event was written.
        let last = events.try_iter().last().expect("the terminal event came");
        assert_eq!(
            last.progress.detail,
            "the export is complete, with warnings"
        );
    }

    #[test]
    fn a_refusal_that_is_not_the_networks_stops_at_once() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![refused("the lane is missing")]);
        let mut program = program(vec![StageStep::run(
            Stage::Timeline,
            vec![call("clip_add")],
        )]);
        let RunOutcome::Failed(error) = runner.run(&mut program, &mut caller) else {
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
        let mut program = program(vec![StageStep::run(
            Stage::AudioRepair,
            vec![call("enhance_speech")],
        )]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Completed);
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
        let mut program = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("plan_validate")],
        )]);
        let RunOutcome::Failed(error) = runner.run(&mut program, &mut caller) else {
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
        let mut program = program(vec![
            StageStep::run(
                Stage::Export,
                vec![call("export_start"), call("export_wait")],
            ),
            StageStep::run(Stage::Watch, vec![call("craft_critique")]).warn(),
        ]);
        assert_eq!(
            runner.run(&mut program, &mut caller),
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
    fn a_failed_gate_fails_the_stage_that_answered_it() {
        // The silent-success shape: every call landed, and the reply the
        // stage drew said the edit failed its own measurement - a
        // loudness reading that missed the platform target.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![Ok(json!({
            "integratedLufs": -19.4, "lufsError": -5.4,
            "platformTargetMet": false,
        }))]);
        let mut program = program(vec![
            StageStep::run(Stage::Verify, vec![call("measure_loudness")]).gate(
                crate::stage::ReplyGate::True {
                    field: "platformTargetMet",
                    what: "the mix missed the platform loudness target",
                },
            ),
        ]);
        let RunOutcome::Failed(error) = runner.run(&mut program, &mut caller) else {
            panic!("the run failed");
        };
        assert_eq!(error.stage, Stage::Verify);
        assert_eq!(
            error.tool_call.as_ref().map(|call| call.tool.as_str()),
            Some("measure_loudness")
        );
        assert!(!error.retryable);
        assert_eq!(
            error.message,
            "the mix missed the platform loudness target: the reply said \
             platformTargetMet=false, not true"
        );
        // The call was made once; a gate reads a landed answer, it does
        // not re-ask for a better one.
        assert_eq!(caller.calls.len(), 1);
        let AutoEditState::Failed { stage, attempt, .. } = runner.state() else {
            panic!("the run failed");
        };
        assert_eq!(stage, Stage::Verify);
        assert_eq!(attempt, 1);
    }

    #[test]
    fn a_gate_no_reply_answered_fails_the_stage() {
        // A verify nobody ran is not a verify: the call that would prove
        // the gate either did not run or did not say.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![Ok(json!({ "job": "e1" }))]);
        let mut program = program(vec![
            StageStep::run(Stage::Verify, vec![call("measure_loudness")]).gate(
                crate::stage::ReplyGate::True {
                    field: "platformTargetMet",
                    what: "the mix missed the platform loudness target",
                },
            ),
        ]);
        let RunOutcome::Failed(error) = runner.run(&mut program, &mut caller) else {
            panic!("the run failed");
        };
        assert_eq!(error.stage, Stage::Verify);
        assert!(
            error
                .message
                .contains("never answered the platformTargetMet gate")
        );
    }

    #[test]
    fn a_gate_under_the_warn_policy_ships_warned() {
        // The craft bar: a low score is a flaw the edit ships with,
        // recorded - the yellow toast - not a run thrown away.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![
            Ok(json!({ "job": "e1" })),
            Ok(json!({ "path": "/edits/Reel/export.mp4" })),
            Ok(json!({ "pngPath": "/tmp/sheet.png" })),
            Ok(json!({ "engine": "heuristic", "overall": 6.0 })),
        ]);
        let mut program = program(vec![
            StageStep::run(
                Stage::Export,
                vec![call("export_start"), call("export_wait")],
            ),
            StageStep::run(
                Stage::Watch,
                vec![call("watch_render"), call("craft_critique")],
            )
            .warn()
            .gate(crate::stage::ReplyGate::AtLeast {
                field: "overall",
                minimum: 7.0,
                what: "the cut scored under the craft bar",
            }),
        ]);
        assert_eq!(
            runner.run(&mut program, &mut caller),
            RunOutcome::CompletedWithWarnings
        );
        let AutoEditState::CompletedWithWarnings { warnings, .. } = runner.state() else {
            panic!("the run completed with warnings");
        };
        assert_eq!(
            warnings,
            vec![
                "watch craft_critique: the cut scored under the craft bar: the \
                 reply said overall=6, under the 7 bar"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn a_satisfied_gate_stays_silent() {
        // The same program, a score over the bar: the gate is not a
        // flavour the reply must carry, it is a demand it must meet.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![
            Ok(json!({ "pngPath": "/tmp/sheet.png" })),
            Ok(json!({ "engine": "local-vlm", "overall": 8.5 })),
        ]);
        let mut program = program(vec![
            StageStep::run(
                Stage::Watch,
                vec![call("watch_render"), call("craft_critique")],
            )
            .warn()
            .gate(crate::stage::ReplyGate::AtLeast {
                field: "overall",
                minimum: 7.0,
                what: "the cut scored under the craft bar",
            }),
        ]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Completed);
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
        let mut program = program(vec![
            StageStep::run(Stage::Timeline, vec![call(EDIT_APPLY)]),
            StageStep::run(Stage::Captions, vec![call("captions_add")]),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Cancelled);
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
        let mut program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(Stage::Export, vec![call("export_start")]),
        ]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Cancelled);
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
        let mut program = program(vec![
            StageStep::run(Stage::Timeline, vec![call(EDIT_APPLY)]),
            StageStep::run(Stage::Captions, vec![call("captions_add")]),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Cancelled);
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
        let mut program = program(vec![
            StageStep::run(Stage::Export, vec![call("export_wait")]).warn(),
            StageStep::run(Stage::Verify, vec![call("timeline_read")]),
        ]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Cancelled);
        assert!(matches!(runner.state(), AutoEditState::Cancelled { .. }));
        // The run never reached the stage after the one it stopped in.
        assert!(!caller.calls.iter().any(|call| call.tool == "timeline_read"));
    }

    #[test]
    fn a_runner_runs_one_run_at_a_time() {
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let (release, gate) = mpsc::channel::<()>();
        let mut gated = Gated { gate };
        let mut on_thread = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("plan_validate")],
        )]);
        let racing = runner.clone();
        let first = std::thread::spawn(move || racing.run(&mut on_thread, &mut gated));
        // The first run reaches its call and stands in the gate.
        std::thread::sleep(Duration::from_millis(100));
        let second = runner.run(
            &mut program(vec![StageStep::run(
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
        let mut program = EditProgram::new(PathBuf::from("/edits/Reel/export.mp4"), Vec::new());
        let RunOutcome::Failed(error) = runner.run(&mut program, &mut Scripted::new(Vec::new()))
        else {
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
        let mut program = program(vec![StageStep::run(
            Stage::Plan,
            vec![call("plan_validate")],
        )]);
        let mut panicking = Panicking;
        let RunOutcome::Failed(error) = runner.run(&mut program, &mut panicking) else {
            panic!("the run failed");
        };
        assert!(error.message.contains("the sense layer lost its mind"));
        assert!(!error.retryable);
        // And the runner is not wedged: the next run runs.
        assert_eq!(
            runner.run(
                &mut program,
                &mut Scripted::new(vec![Ok(json!({ "valid": true }))])
            ),
            RunOutcome::Completed
        );
    }

    #[test]
    fn a_cancelled_runs_rolled_back_stages_leave_the_resume_record() {
        // A cancel rolls the applied edits back; a stage whose work was
        // just undone must not be a stage the next run skips, or the
        // resume ships a timeline the rollback took apart. The read-only
        // stages stay on the record - their work lives in the analysis
        // registry, not on the timeline.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        // The first stage edits; the second is cancelled mid-call.
        let mut program = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![
                StageStep::run(Stage::Timeline, vec![call("edit_apply")]).into(),
                StageStep::run(Stage::Export, vec![call("export_start")]).into(),
                StageStep::run(Stage::Watch, vec![call("watch_render")]).into(),
            ],
        );
        let mut caller = Cancelling {
            runner: runner.clone(),
            when: "export_start",
            replies: vec![
                Ok(json!({ "applied": true })),
                Ok(json!({ "job": "e1" })),
                Ok(json!({ "sheet": true })),
            ]
            .into(),
            calls: Vec::new(),
        };
        let outcome = runner.run(&mut program, &mut caller);
        assert_eq!(outcome, RunOutcome::Cancelled);
        // The run's rollback undid the edit: the Timeline stage is off
        // the resume record, so the next run makes it again.
        let mut again = EditProgram::mixed(
            PathBuf::from("/edits/Reel/export.mp4"),
            vec![
                StageStep::run(Stage::Timeline, vec![call("edit_apply")]).into(),
                StageStep::run(Stage::Export, vec![call("export_start")]).into(),
                StageStep::run(Stage::Watch, vec![call("watch_render")]).into(),
            ],
        );
        let mut scripted = Scripted::new(vec![
            Ok(json!({ "applied": true })),
            Ok(json!({ "job": "e1" })),
            Ok(json!({ "sheet": true })),
        ]);
        let outcome = runner.run(&mut again, &mut scripted);
        assert_eq!(outcome, RunOutcome::Completed);
        // The edit was re-made - the first call of the second run is an
        // edit_apply, not an export_start the stale record would have
        // skipped to.
        assert_eq!(scripted.calls[0].tool, "edit_apply");
    }

    #[test]
    fn a_completed_stage_is_skipped_by_the_next_run_on_the_same_runner() {
        // IDEM: the second run over the same program makes no calls -
        // every stage completed in the first - and a fresh() runner
        // walks the program again.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(Stage::Export, vec![call("export_start")]),
        ]);
        let mut first = Scripted::new(vec![
            Ok(json!({ "valid": true })),
            Ok(json!({ "job": "e1" })),
        ]);
        assert_eq!(runner.run(&mut program, &mut first), RunOutcome::Completed);
        assert_eq!(first.calls.len(), 2);
        assert_eq!(runner.completed(), vec![Stage::Plan, Stage::Export]);

        // The re-run: no calls at all, still completed.
        let mut second = Scripted::new(Vec::new());
        assert_eq!(runner.run(&mut program, &mut second), RunOutcome::Completed);
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
        assert_eq!(runner.run(&mut program, &mut third), RunOutcome::Completed);
        assert_eq!(third.calls.len(), 2);
    }

    #[test]
    fn a_failed_run_resumes_after_its_completed_stages() {
        // IDEM: the run that fails at its second stage leaves the first
        // in the resume record; the next run takes only the failed
        // stage's calls. A caller that read the record elsewhere seeds
        // it with resume() to the same effect.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(Stage::Timeline, vec![call("edit_apply")]),
        ]);
        let mut first = Scripted::new(vec![
            Ok(json!({ "valid": true })),
            refused("the lane is missing"),
        ]);
        let RunOutcome::Failed(error) = runner.run(&mut program, &mut first) else {
            panic!("the run failed");
        };
        assert_eq!(error.stage, Stage::Timeline);
        assert_eq!(runner.completed(), vec![Stage::Plan]);

        let mut second = Scripted::new(vec![Ok(json!({ "applied": 3 }))]);
        assert_eq!(runner.run(&mut program, &mut second), RunOutcome::Completed);
        assert_eq!(second.calls.len(), 1, "the completed stage re-ran");
        assert_eq!(second.calls[0].tool, "edit_apply");
        assert_eq!(runner.completed(), vec![Stage::Plan, Stage::Timeline]);

        // A fresh runner seeded by resume() skips the same way.
        let (other, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        other.resume([Stage::Plan]);
        let mut seeded = Scripted::new(vec![Ok(json!({ "applied": 3 }))]);
        assert_eq!(other.run(&mut program, &mut seeded), RunOutcome::Completed);
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
        let mut program = program(vec![StageStep::run(
            Stage::Timeline,
            vec![call(EDIT_APPLY), named],
        )]);
        let mut caller = Scripted::new(vec![Ok(json!({})), Ok(json!({}))]);
        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Completed);
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
            1,
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

    #[test]
    fn the_verify_stage_keeps_the_emotion_reply_over_later_probes() {
        // The verify's answer is the emotion read - the words the export
        // says, checked against the plan. A probe that runs after it
        // (the render watch, a frame count) is not a verification, and
        // must not replace the one the report is read for.
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        let mut caller = Scripted::new(vec![
            Ok(json!({ "valid": true })),
            Ok(json!({ "job": "e1" })),
            Ok(json!({ "path": "/edits/Reel/export.mp4" })),
            Ok(json!({ "overlaps": 0, "gaps": 0 })),
            Ok(json!({ "emotion": "verified", "matchRate": 1.0 })),
            Ok(json!({ "frames": 1 })),
        ]);
        let mut program = program(vec![
            StageStep::run(Stage::Plan, vec![call("plan_validate")]),
            StageStep::run(
                Stage::Export,
                vec![call("export_start"), call("export_wait")],
            ),
            StageStep::run(
                Stage::Verify,
                vec![call("timeline_read"), call("emotion"), call("watch_render")],
            ),
        ]);

        assert_eq!(runner.run(&mut program, &mut caller), RunOutcome::Completed);
        let AutoEditState::Completed { verification, .. } = runner.state() else {
            panic!("the run completed");
        };
        assert_eq!(
            verification,
            json!({ "emotion": "verified", "matchRate": 1.0 }),
            "the later render probe must not replace the emotion verification"
        );
    }
}
