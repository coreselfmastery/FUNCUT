// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The auto-edit runner: the state machine, the progress events, the
//! cancellation with rollback and the retry contract an unattended edit
//! needs - the machinery the AUTO EDIT tool will stand on.
//!
//! A run is given to a [`Runner`] as an [`EditProgram`]: the export the
//! plan promises, and the stages with the tool calls each one makes. The
//! stages' names are the pipeline's own - the watcher's numbered phases
//! (`scripts/auto_director.py`), gathered into the stops an unattended
//! run reports. The runner walks the program and owns everything around
//! the walking:
//!
//! - **State and progress** - every change of standing is an
//!   [`AutoEditEvent`] on a channel, and [`Runner::progress`] reads the
//!   last event it emitted: one source of truth, so a poller and a
//!   watcher of the same run can never disagree.
//! - **Cancellation** - [`Runner::cancel`] asks the run to stop at the
//!   next stage boundary; a stage that has begun finishes what it is
//!   doing first. The edits the run applied are rolled back with
//!   `edit_undo`, one per applied `edit_apply` - the project's own
//!   history, stepped back the same way the window's undo steps it - and
//!   the run lands [`Cancelled`](AutoEditState::Cancelled), saying
//!   whether the rollback reached all the way back.
//! - **Retries** - a refusal is tried again when the tool is one whose
//!   retries land and the failure sounds like the network's side of the
//!   contract: three attempts, the watcher's own words and pauses, so a
//!   retry means the same thing on both sides of the seam.
//! - **Warnings** - a stage can be given the warn policy: its failure is
//!   recorded and the run ships
//!   [`CompletedWithWarnings`](AutoEditState::CompletedWithWarnings),
//!   the way the revise loop will ship an edit whose critic notes could
//!   not all be answered.
//!
//! The stages themselves are not written here, and no tool call is made
//! here. The runner is the loop; a [`ToolCaller`] - one MCP-shaped call
//! at a time - is the port it drives. The `Backend` that will stand
//! behind that port lives in `concat-mcp`, and `concat-mcp` is the crate
//! that will depend on this one for its wiring, so the runner cannot
//! name it: the adapter from `Backend` to `ToolCaller` (the same
//! name-to-`Request` translation `concat-mcp`'s protocol performs for
//! `tools/call`) lands with the AUTO EDIT tool that stands on this
//! runner.

pub mod error;
pub mod runner;
pub mod stage;
pub mod state;

pub use error::{AutoEditError, ToolCall, ToolError, is_retryable};
pub use runner::{EDIT_APPLY, EDIT_UNDO, Runner, ToolCaller};
pub use stage::{EditProgram, ErrorPolicy, Stage, StageStep};
pub use state::{AutoEditEvent, AutoEditProgress, AutoEditState, RunOutcome};
