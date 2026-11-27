// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! `auto_edit`: the window's AUTO EDIT button as one MCP call.
//!
//! The button's fixed spine is [`auto_edit_program`](crate::director_program::auto_edit_program).
//! This module supplies the two things the button adds around it - a
//! sibling copy so the source project is never overwritten, and the take
//! and placement rows the program is written from - then drives the same
//! [`Runner`] through this server's own toolbox. The result is the
//! terminal state the button shows, the report and receipt it writes
//! beside the edit, and the path an agent opens next.
//!
//! The Director handoff stays the GUI's: it needs the Forge harness, a
//! model key and a skill installation, none of which belong in a
//! headless server's default call. An agent that wants the Director can
//! still run the harness itself; `auto_edit` is the deterministic edit
//! that always works.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use concat_api::ApiError;
use concat_director::{AutoEditState, RunOutcome, Runner};
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::director::{BackendToolCaller, LaneOpener};
use crate::director_program::{
    AutoEditProgramSource, auto_edit_program, auto_edit_source_from_view,
};
use crate::tools::{Context, Outcome, Toolbox};

/// Keeps the recorded copy of every take that lives outside the project.
///
/// The repair chain lands its result on the take the timeline plays - it
/// has to, because a project's clip names its media and nothing relinks
/// it - so a take imported by reference from the user's own folder would
/// be rewritten in place. The recorded file is copied into the edit's
/// own folder first, so the take as recorded always survives a repair
/// that lands badly. A take already inside the project needs no copy:
/// the project holds it.
fn keep_recorded_takes(project: &Path, source: &AutoEditProgramSource) -> Result<(), String> {
    for (index, take) in source.takes.iter().enumerate() {
        preserve_recorded_take(project, index, Path::new(&take.take))?;
    }
    Ok(())
}

/// Preserve the current bytes before the repair chain can write back to an
/// externally referenced take. Refresh an earlier snapshot on every run:
/// an old copy is not evidence of the current source. A failed copy stops
/// the edit rather than silently risking the only original.
fn preserve_recorded_take(project: &Path, index: usize, path: &Path) -> Result<(), String> {
    if path.starts_with(project) || !path.is_file() {
        return Ok(());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("take.wav");
    let kept = project
        .join("auto-edit")
        .join(format!("recorded-{index:03}-{name}"));
    std::fs::copy(path, &kept).map_err(|error| {
        format!(
            "could not preserve the original take {} at {} before repair: {error}",
            path.display(),
            kept.display()
        )
    })?;
    Ok(())
}


/// The headless tool has no channel metadata to inspect. Default to the
/// restrictive licensing path; personal/research use must be an explicit
/// opt-out rather than an accidental omission by an unattended caller.
fn monetized_argument(arguments: &Value) -> bool {
    arguments
        .get("monetized")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Run the fixed AUTO EDIT spine over `path`, or over a sibling copy of
/// it. The terminal state is returned whether the run completed, warned,
/// failed or was cancelled; a failed or cancelled run is marked as an
/// error outcome so an agent does not mistake it for a finished edit.
pub fn run(toolbox: &Toolbox, arguments: Value, context: Context<'_>) -> Result<Outcome, ApiError> {
    let Context {
        backend,
        cancelled,
        progress: _,
    } = context;
    let path = arguments["path"]
        .as_str()
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| ApiError::invalid("missing field `path`: the project folder"))?;
    let source = PathBuf::from(path);
    if !source.is_dir() {
        return Err(ApiError::invalid(format!(
            "the project folder {path} does not exist"
        )));
    }
    let copy = arguments
        .get("copy")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let script = arguments
        .get("script")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let stock_provider = arguments
        .get("stockProvider")
        .and_then(Value::as_str)
        .unwrap_or("pexels")
        .to_owned();
    // The orientation the run was asked to build for: one of the three
    // words, or nothing at all. Anything else is treated as nothing - the
    // run is never failed over a pick it cannot read - and nothing keeps
    // the frame the project already has, exactly as before.
    let orientation = arguments
        .get("orientation")
        .and_then(Value::as_str)
        .map(|word| word.trim().to_owned())
        .filter(|word| matches!(word.as_str(), "horizontal" | "vertical" | "square"));
    // Whether the edit is monetized, threaded into the brief and the bed
    // calls. The unattended default is the licence-safe side: non-commercial
    // beds and the research-only OpenVid shelf stay refused unless a caller
    // explicitly opts into a personal/research edit with `monetized: false`.
    let monetized = monetized_argument(&arguments);
    // The author's own creative intent, when the caller carries one: the
    // same `creative-brief.json` every later turn reads, merged field by
    // field over whatever the project already persisted so a second run
    // can change the tone without losing the audience it set last time.
    let brief = arguments
        .get("brief")
        .filter(|brief| brief.is_object())
        .cloned();

    let project = if copy {
        copy_project(&source)?
    } else {
        source.clone()
    };
    let project_path = project.to_string_lossy().into_owned();
    std::fs::create_dir_all(project.join("auto-edit")).map_err(|error| {
        ApiError::failed(format!(
            "could not make the auto-edit folder in {project_path}: {error}"
        ))
    })?;
    if let Some(brief) = brief {
        let path = project.join("creative-brief.json");
        let mut merged = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        if let Some(fields) = brief.as_object() {
            for (key, value) in fields {
                if !value.is_null() {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
        let bytes = serde_json::to_vec_pretty(&Value::Object(merged)).map_err(|error| {
            ApiError::failed(format!("could not serialise the creative brief: {error}"))
        })?;
        std::fs::write(&path, bytes).map_err(|error| {
            ApiError::failed(format!(
                "could not write the creative brief to {}: {error}",
                path.display()
            ))
        })?;
    }

    call_json(
        toolbox,
        backend,
        cancelled,
        "project_open",
        json!({ "path": project_path }),
    )?;
    // Every refusal from here to the close below closes what this call
    // opened first: the open took the session's claim on the project,
    // and a claim that outlives its failed run leaves the window's own
    // open refused - the project is open in another process - until the
    // server exits. A failed run must hand the project back, not hold
    // it.
    let view = match call_json(
        toolbox,
        backend,
        cancelled,
        "timeline_read",
        json!({ "path": project_path }),
    ) {
        Ok(view) => view,
        Err(error) => {
            let _ = close_project(toolbox, backend, cancelled, &project_path);
            return Err(error);
        }
    };
    let program_source = match auto_edit_source_from_view(
        &project_path,
        &view,
        script,
        stock_provider,
        orientation,
        monetized,
    ) {
        Ok(source) => source,
        Err(error) => {
            let _ = close_project(toolbox, backend, cancelled, &project_path);
            return Err(ApiError::invalid(error));
        }
    };
    if let Err(error) = keep_recorded_takes(&project, &program_source) {
        let close_error = close_project(toolbox, backend, cancelled, &project_path).err();
        return Err(ApiError::failed(match close_error {
            Some(close) => format!("{error}; additionally, {close}"),
            None => error,
        }));
    }
    let mut program = auto_edit_program(&program_source);
    // The runner's event receiver is dropped on purpose: a headless call
    // has no watcher to read it, and a receiver held and never drained
    // buffers every progress event the run emits in memory for the whole
    // run. Dropped, the runner's sends fail and discard - the contract
    // its own "a receiver nobody reads is fine" note names.
    let (runner, _events) = Runner::new(project.clone());
    drop(_events);
    // The mode is read before the caller takes the backend: the lanes
    // open the way this run's own backend was opened, so a run served by
    // the window's server gets more connections to it and a headless
    // run gets APIs of this process.
    let lane_mode = backend.mode();
    let mut caller = BackendToolCaller::new(toolbox, backend, cancelled);
    // The fan stages - the per-take repair, transcription and voice
    // reads - run side by side here the way the window's own run runs
    // them: every lane its own backend over a toolbox built for the
    // run, stopped by one flag. The client's cancellation is a borrow
    // the lanes cannot hold, so a scoped bridge carries it to their
    // flag: a fan left running after its caller hung up is minutes of
    // machine spent on an answer nobody will read.
    let lane_cancel = Arc::new(AtomicBool::new(false));
    let run_over = Arc::new(AtomicBool::new(false));
    let outcome = std::thread::scope(|scope| {
        watch_client_cancellation(scope, cancelled, &lane_cancel, &run_over, &runner);
        // The guard covers the run's own frame: a panic between the
        // calls the runner's per-call guards already catch must still
        // end the watch, or the join below never returns.
        let _run_ends = RunOverOnDrop(Arc::clone(&run_over));
        let lanes = LaneOpener::new(
            lane_mode,
            Arc::new(Toolbox::new()),
            Arc::clone(&lane_cancel),
        );
        let outcome =
            runner.run_with_lanes_and_cancellation(&mut program, &mut caller, &lanes, cancelled);
        drop(_run_ends);
        outcome
    });
    let state = runner.state();
    let state_value = serde_json::to_value(&state).unwrap_or_else(|_| json!({}));
    // The report and the receipt are written beside the edit whatever the
    // run's standing; a write that cannot land is named in the answer
    // rather than silently answered as one that did.
    let mut write_errors: Vec<String> = Vec::new();
    let report = match write_report(&project, &state_value) {
        Ok(path) => Some(path),
        Err(error) => {
            write_errors.push(error);
            None
        }
    };
    let receipt = match write_receipt(&project, &state) {
        Ok(path) => path,
        Err(error) => {
            write_errors.push(error);
            None
        }
    };
    // Leave the project closed: a headless server holds its locks until
    // it exits, and the next call may be the window opening this edit.
    // The close saves what the run left - though the run's own edits
    // autosaved as they were applied - and a save that cannot land (a
    // read-only copy, a full disk) falls back to closing without one
    // rather than leaving the claim standing behind a refused save. A
    // close that still cannot land is named in the answer, not
    // answered as if it had.
    if let Err(error) = close_project(toolbox, backend, cancelled, &project_path) {
        write_errors.push(error);
    }
    let mut value = json!({
        "project": project_path,
        "source": path,
        "copy": copy,
        "outcome": outcome_label(&outcome),
        "state": state_value,
        "report": report,
        "receipt": receipt,
    });
    if !write_errors.is_empty() {
        value["writeErrors"] = json!(write_errors);
    }
    let mut result = Outcome::json(value);
    if matches!(
        state,
        AutoEditState::Failed { .. } | AutoEditState::Cancelled { .. }
    ) {
        result.is_error = true;
    }
    Ok(result)
}

/// Close the project a run opened, saving what it left. The first close
/// saves; when the save itself is what refuses, the fallback closes
/// without one - the run's edits autosaved as they were applied, so the
/// save is a courtesy, while the claim the close releases is the thing
/// the window's next open needs gone. The sentence is the close's own
/// failure, for the caller to name in its answer.
fn close_project(
    toolbox: &Toolbox,
    backend: &mut dyn Backend,
    cancelled: &AtomicBool,
    path: &str,
) -> Result<(), String> {
    let saved = call_json(
        toolbox,
        backend,
        cancelled,
        "project_close",
        json!({ "path": path, "save": true }),
    );
    match saved {
        Ok(_) => Ok(()),
        Err(save_error) => {
            match call_json(
                toolbox,
                backend,
                cancelled,
                "project_close",
                json!({ "path": path, "save": false }),
            ) {
                Ok(_) => Err(format!(
                    "the run's project was closed without a save ({save_error})"
                )),
                Err(close_error) => Err(format!(
                    "the run's project could not be closed (saving said: {save_error}; \
                     closing without a save said: {close_error})"
                )),
            }
        }
    }
}

fn call_json(
    toolbox: &Toolbox,
    backend: &mut dyn Backend,
    cancelled: &AtomicBool,
    name: &str,
    arguments: Value,
) -> Result<Value, ApiError> {
    let outcome = toolbox
        .call(
            name,
            arguments,
            Context {
                backend,
                cancelled,
                progress: None,
            },
        )
        .ok_or_else(|| ApiError::failed(format!("the {name} tool is not on this server")))?;
    if outcome.is_error {
        let sentence = outcome
            .content
            .first()
            .and_then(|block| block["text"].as_str())
            .unwrap_or("the call was refused")
            .to_owned();
        return Err(ApiError::failed(sentence));
    }
    outcome
        .structured
        .ok_or_else(|| ApiError::failed(format!("the {name} tool answered without a result")))
}

fn outcome_label(outcome: &RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Completed => "completed",
        RunOutcome::CompletedWithWarnings => "completed-with-warnings",
        RunOutcome::Failed(_) => "failed",
        RunOutcome::Cancelled => "cancelled",
    }
}

/// Write the run's report beside the edit. The path is the answer on
/// success; the sentence is the failure the caller surfaces, so a report
/// that could not be written is named rather than answered as one that
/// was.
fn write_report(project: &Path, state_value: &Value) -> Result<String, String> {
    let report = json!({
        "schema": "funcut.auto-edit-report/1",
        "state": state_value,
        "director": {
            "used": false,
            "why": "the fixed spine ran over MCP; the Director is the GUI handoff",
        },
    });
    let path = project.join("auto-edit-report.json");
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("could not encode the auto-edit report: {error}"))?;
    std::fs::write(&path, bytes)
        .map(|_| path.to_string_lossy().into_owned())
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// Write the run's receipt beside the edit, whatever its standing: a
/// completed run's receipt and a failed or cancelled run's partial one
/// (the stages that had completed, the gates their answers had measured)
/// are both evidence the caller keeps. A state with no receipt to write
/// is `Ok(None)`, not a failure: only a receipt that existed and could
/// not be written answers `Err`.
fn write_receipt(project: &Path, state: &AutoEditState) -> Result<Option<String>, String> {
    let receipt = match state {
        AutoEditState::Completed { receipt, .. }
        | AutoEditState::CompletedWithWarnings { receipt, .. }
        | AutoEditState::Failed { receipt, .. }
        | AutoEditState::Cancelled { receipt, .. } => receipt.as_deref(),
        _ => None,
    };
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    let path = project.join("auto-edit-receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt.to_json())
        .map_err(|error| format!("could not encode the auto-edit receipt: {error}"))?;
    std::fs::write(&path, bytes)
        .map(|_| path.to_string_lossy().into_owned())
        .map(Some)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// The sibling folder a copy-mode run edits: `<name> edit`, then
/// `edit 2`, `edit 3`, ... so a second run never overwrites the first.
///
/// A source the caller spelled as `.` (or any path whose literal parent
/// is the empty path) has its parent and its name read from the
/// filesystem: the literal parent of `.` is `""`, and a sibling joined
/// onto `""` is created inside the project it copies.
fn sibling_path(source: &Path) -> PathBuf {
    let real = source.canonicalize().ok();
    let parent = match source.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => real
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| Path::new(".").to_path_buf()),
    };
    let stem = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            real.as_deref()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
        })
        .unwrap_or("funcut");
    for attempt in 0..50 {
        let name = if attempt == 0 {
            format!("{stem} edit")
        } else {
            format!("{stem} edit {}", attempt + 1)
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem} edit {}", std::process::id()))
}

fn copy_project(source: &Path) -> Result<PathBuf, ApiError> {
    let destination = reserve_sibling(source);
    copy_tree(source, &destination).map_err(|error| {
        ApiError::failed(format!(
            "could not copy {} to {}: {error}",
            source.display(),
            destination.display()
        ))
    })?;
    rewrite_media_paths(&destination, source)?;
    Ok(destination)
}

/// The sibling a copy-mode run edits, reserved on the filesystem the
/// moment it is chosen. `sibling_path` names the first name that does not
/// stand, and the reservation is an exclusive create, so two concurrent
/// runs that both read the same free name cannot both copy into it - the
/// second takes `edit 2` instead of writing over the first's edits. The
/// reservation is the copy's first act, so a run that fails after
/// reserving leaves an empty folder behind, not a second run's edits
/// overwritten.
fn reserve_sibling(source: &Path) -> PathBuf {
    for _ in 0..50 {
        let candidate = sibling_path(source);
        match std::fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            // Taken, whatever took it - another run's reservation or a
            // folder that predates the run: the next name is asked for.
            Err(_) => continue,
        }
    }
    sibling_path(source)
}

/// What a copy never carries from the source project's own root: the
/// previous run's auto-edit evidence - the report, the receipt and the
/// `auto-edit/` folder of intermediates. A second run over an earlier
/// edit would otherwise inherit the first run's artifacts as if this
/// run had written them: a run that fails before rewriting them leaves
/// the old receipt beside the new edit, and the old intermediates ship
/// in the copy as though they were this run's. The names are skipped at
/// the project's root only - a user folder that shares a reserved name
/// deeper in the tree is the user's, not the run's.
const NOT_COPIED: &[&str] = &[
    "auto-edit",
    "auto-edit-report.json",
    "auto-edit-receipt.json",
];

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    copy_contents(source, destination, true)
}

/// [`copy_tree`]'s walk: `root` says whether `source` is the project's
/// own folder, so the reserved names above are skipped exactly where the
/// run writes them.
fn copy_contents(source: &Path, destination: &Path, root: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "concat.lock" || (root && NOT_COPIED.contains(&name.as_str())) {
            continue;
        }
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_contents(&entry.path(), &target, false)?;
        } else if kind.is_file() || kind.is_symlink() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The copy's `concat.json` names its own media, never the source's: a
/// file already inside the project is the copied one, and a file from
/// outside is copied into the sibling's `media/` folder. A missing file
/// stops the run before any edit rather than becoming a silent black
/// span. Inside and outside are decided on canonical paths, so a stored
/// `..` or a symlinked spelling cannot strip as "inside" while the
/// target the rewrite joins escapes the sibling.
fn rewrite_media_paths(destination: &Path, source: &Path) -> Result<(), ApiError> {
    let document_path = destination.join("concat.json");
    let text = std::fs::read_to_string(&document_path).map_err(|error| {
        ApiError::failed(format!(
            "could not read the copied project {}: {error}",
            document_path.display()
        ))
    })?;
    let mut document: Value = serde_json::from_str(&text).map_err(|error| {
        ApiError::failed(format!(
            "could not parse the copied project {}: {error}",
            document_path.display()
        ))
    })?;
    let media = document
        .get_mut("media")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ApiError::failed("the copied project has no media array"))?;
    // The containment test is made on real paths: `strip_prefix` matches
    // components literally, so the target it decides must not be joined
    // from a stored spelling that still carries `..` or a symlink.
    let source_root = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    for (index, item) in media.iter_mut().enumerate() {
        let Some(path) = item.get("path").and_then(Value::as_str) else {
            continue;
        };
        if path.trim().is_empty() {
            continue;
        }
        let original = PathBuf::from(path);
        let resolved = if original.is_absolute() {
            original
        } else {
            source.join(&original)
        };
        if !resolved.is_file() {
            return Err(ApiError::failed(format!(
                "the project's media file {} is missing; auto_edit will not edit a project \
                 whose footage cannot be opened",
                resolved.display()
            )));
        }
        let resolved = resolved.canonicalize().map_err(|error| {
            ApiError::failed(format!(
                "the project's media file {} could not be resolved: {error}",
                resolved.display()
            ))
        })?;
        let target = match resolved.strip_prefix(&source_root) {
            Ok(relative) => destination.join(relative),
            Err(_) => {
                let folder = destination.join("media");
                std::fs::create_dir_all(&folder).map_err(|error| {
                    ApiError::failed(format!(
                        "could not make the media folder {}: {error}",
                        folder.display()
                    ))
                })?;
                let name = resolved
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("media");
                let target = folder.join(format!("{index:03}-{name}"));
                std::fs::copy(&resolved, &target).map_err(|error| {
                    ApiError::failed(format!(
                        "could not copy the project's media {}: {error}",
                        resolved.display()
                    ))
                })?;
                target
            }
        };
        item["path"] = json!(target.to_string_lossy());
    }
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| {
        ApiError::failed(format!("could not write the copied project: {error}"))
    })?;
    std::fs::write(&document_path, bytes).map_err(|error| {
        ApiError::failed(format!(
            "could not write the copied project {}: {error}",
            document_path.display()
        ))
    })
}

/// Watches the client's cancellation for as long as the run goes, and
/// carries a rise to the lanes' own flag: the context's flag is a
/// borrow the lanes cannot hold, so a scoped thread does the holding.
/// `run_over` is the run's own end - the watch leaves with it, so a
/// finished run never keeps a thread parked on its caller's flag.
fn watch_client_cancellation<'env>(
    scope: &'env std::thread::Scope<'env, '_>,
    client: &'env AtomicBool,
    lanes: &Arc<AtomicBool>,
    run_over: &Arc<AtomicBool>,
    runner: &Runner,
) {
    let lanes = Arc::clone(lanes);
    let run_over = Arc::clone(run_over);
    let runner = runner.clone();
    scope.spawn(move || {
        while !run_over.load(Ordering::Relaxed) {
            if client.load(Ordering::Relaxed) {
                // Stop both sides of the fan: the backend flag releases
                // in-flight lane calls, and the runner flag makes the
                // joined failure a cancellation with rollback rather
                // than an ordinary failed/warned stage.
                lanes.store(true, Ordering::Relaxed);
                runner.cancel();
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

/// Sets the run-over flag when it drops, however the run ends: a panic
/// unwinds through the scope without running the statement that sets
/// the flag, and the watch must leave with the run or the scope's own
/// join hangs on it.
struct RunOverOnDrop(Arc<AtomicBool>);

impl Drop for RunOverOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_that_hangs_up_is_carried_to_the_runner_and_lanes() {
        let client = AtomicBool::new(false);
        let lanes = Arc::new(AtomicBool::new(false));
        let run_over = Arc::new(AtomicBool::new(false));
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        std::thread::scope(|scope| {
            // Ensure the watcher is released even if the assertion below
            // fails: a failing test must not hang the scoped join.
            let _run_ends = RunOverOnDrop(Arc::clone(&run_over));
            watch_client_cancellation(scope, &client, &lanes, &run_over, &runner);
            // The client hangs up mid-run: the watch carries the rise to
            // the lanes within its poll budget.
            client.store(true, Ordering::Relaxed);
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !lanes.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                lanes.load(Ordering::Relaxed),
                "the lanes were never told the client hung up"
            );
            assert!(
                runner.cancelled(),
                "the runner was not told to cancel and roll back"
            );
        });
    }

    #[test]
    fn the_watch_leaves_with_the_run() {
        let client = AtomicBool::new(false);
        let lanes = Arc::new(AtomicBool::new(false));
        // The run is already over: the watch must leave at once rather
        // than park on the caller's flag - a returned scope is the proof.
        let run_over = Arc::new(AtomicBool::new(true));
        let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
        std::thread::scope(|scope| {
            watch_client_cancellation(scope, &client, &lanes, &run_over, &runner);
        });
        assert!(!lanes.load(Ordering::Relaxed));
        assert!(!runner.cancelled());
    }

    #[test]
    fn a_recorded_take_snapshot_is_refreshed_before_each_repair() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("Reel");
        std::fs::create_dir_all(project.join("auto-edit")).unwrap();
        let source = dir.path().join("take.wav");
        let kept = project.join("auto-edit").join("recorded-000-take.wav");

        std::fs::write(&source, b"first take").unwrap();
        preserve_recorded_take(&project, 0, &source).unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), b"first take");

        // copy=false can revisit a project with an earlier snapshot. It
        // must refresh that file before the next repair, not trust stale bytes.
        std::fs::write(&source, b"new take").unwrap();
        preserve_recorded_take(&project, 0, &source).unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), b"new take");
    }

    #[test]
    fn a_failed_recorded_take_snapshot_is_not_silently_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("Reel");
        std::fs::create_dir_all(project.join("auto-edit").join("recorded-000-take.wav"))
            .unwrap();
        let source = dir.path().join("take.wav");
        std::fs::write(&source, b"original").unwrap();

        let error = preserve_recorded_take(&project, 0, &source).unwrap_err();
        assert!(error.contains("could not preserve the original take"), "{error}");
    }

    #[test]
    fn an_omitted_monetized_flag_defaults_to_the_licence_safe_path() {
        assert!(monetized_argument(&json!({})));
        assert!(monetized_argument(&json!({"monetized": true})));
        assert!(!monetized_argument(&json!({"monetized": false})));
        assert!(
            monetized_argument(&json!({"monetized": "false"})),
            "a malformed flag must not loosen the licensing gate"
        );
    }

    #[test]
    fn a_sibling_name_never_overwrites_an_earlier_edit() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Reel");
        std::fs::create_dir_all(&source).unwrap();
        assert_eq!(sibling_path(&source), dir.path().join("Reel edit"));
        std::fs::create_dir_all(dir.path().join("Reel edit")).unwrap();
        assert_eq!(sibling_path(&source), dir.path().join("Reel edit 2"));
    }

    #[test]
    fn a_reserved_sibling_is_held_not_just_named() {
        // The reservation is the copy's first act: an exclusive create,
        // so two concurrent runs that both read the same free name
        // cannot both copy into it - the second takes `edit 2` instead
        // of writing over the first's edits.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Reel");
        std::fs::create_dir_all(&source).unwrap();
        let first = reserve_sibling(&source);
        assert_eq!(first, dir.path().join("Reel edit"));
        assert!(first.is_dir(), "the reservation is a folder, not a plan");
        let second = reserve_sibling(&source);
        assert_eq!(second, dir.path().join("Reel edit 2"));
    }

    #[test]
    fn a_current_directory_source_names_a_real_sibling() {
        // `.` has no literal parent (`Path::parent` answers the empty
        // path) and no file name; the sibling used to be joined onto
        // `""`, creating the copy inside the project it copies.
        let sibling = sibling_path(Path::new("."));
        assert!(
            sibling.is_absolute(),
            "the sibling is a real folder beside the project: {}",
            sibling.display()
        );
        let stem = sibling
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        assert!(
            stem.ends_with("edit"),
            "named from the real project: {stem}"
        );
    }

    #[test]
    fn a_copy_rewrites_media_paths_inside_and_outside_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Reel");
        std::fs::create_dir_all(source.join("media")).unwrap();
        let inside = source.join("media").join("take.wav");
        let outside = dir.path().join("outside.wav");
        std::fs::write(&inside, b"inside").unwrap();
        std::fs::write(&outside, b"outside").unwrap();
        std::fs::write(source.join("concat.lock"), b"held").unwrap();
        std::fs::write(
            source.join("concat.json"),
            serde_json::to_vec(&json!({
                "media": [
                    { "id": "m1", "path": inside.to_string_lossy() },
                    { "id": "m2", "path": outside.to_string_lossy() },
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let copy = copy_project(&source).unwrap();
        let document: Value =
            serde_json::from_str(&std::fs::read_to_string(copy.join("concat.json")).unwrap())
                .unwrap();
        let paths: Vec<&str> = document["media"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["path"].as_str())
            .collect();
        assert!(paths[0].starts_with(copy.to_string_lossy().as_ref()));
        assert!(paths[0].ends_with("media/take.wav"));
        assert!(paths[1].starts_with(copy.to_string_lossy().as_ref()));
        assert!(paths[1].ends_with("media/001-outside.wav"));
        assert!(Path::new(paths[0]).is_file());
        assert!(Path::new(paths[1]).is_file());
        assert!(
            !copy.join("concat.lock").exists(),
            "a source lock is not the copy's"
        );
    }

    #[test]
    fn a_dot_dot_media_path_cannot_escape_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Reel");
        std::fs::create_dir_all(source.join("media")).unwrap();
        std::fs::write(source.join("media").join("take.wav"), b"inside").unwrap();
        let outside = dir.path().join("outside.wav");
        std::fs::write(&outside, b"outside").unwrap();
        std::fs::write(
            source.join("concat.json"),
            serde_json::to_vec(&json!({
                "media": [
                    { "id": "m1", "path": "media/take.wav" },
                    { "id": "m2", "path": "../outside.wav" },
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let copy = copy_project(&source).unwrap();
        let document: Value =
            serde_json::from_str(&std::fs::read_to_string(copy.join("concat.json")).unwrap())
                .unwrap();
        let paths: Vec<PathBuf> = document["media"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| PathBuf::from(item["path"].as_str().unwrap()))
            .collect();
        for path in &paths {
            assert!(
                !path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir)),
                "a stored path carries no `..`: {}",
                path.display()
            );
            assert!(
                path.starts_with(&copy),
                "the copy names its own: {}",
                path.display()
            );
            assert!(
                path.is_file(),
                "the copy carries the file: {}",
                path.display()
            );
        }
        assert!(
            paths[1].starts_with(copy.join("media")),
            "the outside file is copied in, not referenced where it lies"
        );
    }

    #[test]
    fn a_copy_of_an_earlier_edit_carries_none_of_its_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Reel");
        std::fs::create_dir_all(source.join("auto-edit")).unwrap();
        std::fs::write(
            source.join("auto-edit").join("enhanced-000-a.wav"),
            b"stale",
        )
        .unwrap();
        std::fs::write(source.join("auto-edit-report.json"), b"stale").unwrap();
        std::fs::write(source.join("auto-edit-receipt.json"), b"stale").unwrap();
        std::fs::create_dir_all(source.join("footage").join("auto-edit")).unwrap();
        std::fs::write(
            source.join("footage").join("auto-edit").join("clip.mp4"),
            b"user",
        )
        .unwrap();
        std::fs::write(
            source.join("concat.json"),
            serde_json::to_vec(&json!({ "media": [] })).unwrap(),
        )
        .unwrap();
        let copy = copy_project(&source).unwrap();
        assert!(
            !copy.join("auto-edit").exists(),
            "the intermediates folder is this run's, recreated empty - not the last run's"
        );
        assert!(!copy.join("auto-edit-report.json").exists());
        assert!(!copy.join("auto-edit-receipt.json").exists());
        assert!(
            copy.join("footage")
                .join("auto-edit")
                .join("clip.mp4")
                .is_file(),
            "a user folder that shares the reserved name deeper in the tree is the user's"
        );
    }

    #[test]
    fn a_report_write_failure_is_surfaced_not_silent() {
        let dir = tempfile::tempdir().unwrap();
        // A directory squatting on the report's name: the write cannot
        // land, and the failure is answered rather than swallowed.
        std::fs::create_dir_all(dir.path().join("auto-edit-report.json")).unwrap();
        let report = write_report(dir.path(), &json!({}));
        assert!(report.is_err());
        assert!(report.unwrap_err().contains("auto-edit-report.json"));
    }
}
