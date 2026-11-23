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
use std::sync::atomic::AtomicBool;

use concat_api::ApiError;
use concat_director::{AutoEditState, RunOutcome, Runner};
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::director::BackendToolCaller;
use crate::director_program::{auto_edit_program, auto_edit_source_from_view};
use crate::tools::{Context, Outcome, Toolbox};

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

    call_json(
        toolbox,
        backend,
        cancelled,
        "project_open",
        json!({ "path": project_path }),
    )?;
    let view = call_json(
        toolbox,
        backend,
        cancelled,
        "timeline_read",
        json!({ "path": project_path }),
    )?;
    let program_source = auto_edit_source_from_view(&project_path, &view, script, stock_provider)
        .map_err(ApiError::invalid)?;
    let mut program = auto_edit_program(&program_source);
    let (runner, _events) = Runner::new(project.clone());
    let mut caller = BackendToolCaller::new(toolbox, backend, cancelled);
    let outcome = runner.run(&mut program, &mut caller);
    let state = runner.state();
    let state_value = serde_json::to_value(&state).unwrap_or_else(|_| json!({}));
    let report = write_report(&project, &state_value);
    let receipt = write_receipt(&project, &state);
    // Leave the project closed: a headless server holds its locks until
    // it exits, and the next call may be the window opening this edit.
    let _ = call_json(
        toolbox,
        backend,
        cancelled,
        "project_close",
        json!({ "path": project_path, "save": true }),
    );
    let value = json!({
        "project": project_path,
        "source": path,
        "copy": copy,
        "outcome": outcome_label(&outcome),
        "state": state_value,
        "report": report,
        "receipt": receipt,
    });
    let mut result = Outcome::json(value);
    if matches!(
        state,
        AutoEditState::Failed { .. } | AutoEditState::Cancelled { .. }
    ) {
        result.is_error = true;
    }
    Ok(result)
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

fn write_report(project: &Path, state_value: &Value) -> Option<String> {
    let report = json!({
        "schema": "funcut.auto-edit-report/1",
        "state": state_value,
        "director": {
            "used": false,
            "why": "the fixed spine ran over MCP; the Director is the GUI handoff",
        },
    });
    let path = project.join("auto-edit-report.json");
    let bytes = serde_json::to_vec_pretty(&report).ok()?;
    std::fs::write(&path, bytes)
        .ok()
        .map(|_| path.to_string_lossy().into_owned())
}

fn write_receipt(project: &Path, state: &AutoEditState) -> Option<String> {
    let receipt = match state {
        AutoEditState::Completed { receipt, .. }
        | AutoEditState::CompletedWithWarnings { receipt, .. } => receipt.as_deref(),
        _ => None,
    }?;
    let path = project.join("auto-edit-receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt.to_json()).ok()?;
    std::fs::write(&path, bytes)
        .ok()
        .map(|_| path.to_string_lossy().into_owned())
}

/// The sibling folder a copy-mode run edits: `<name> edit`, then
/// `edit 2`, `edit 3`, ... so a second run never overwrites the first.
fn sibling_path(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let stem = source
        .file_name()
        .and_then(|name| name.to_str())
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
    let destination = sibling_path(source);
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

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy() == "concat.lock" {
            continue;
        }
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
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
/// span.
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
        let target = match resolved.strip_prefix(source) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
