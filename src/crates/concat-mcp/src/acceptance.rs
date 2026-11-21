// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! What a finished Director run is held to before the pane calls it done.
//!
//! The Director ends its turn and the harness exits zero. That says the
//! agent stopped - not that the edit exists, plays, or carries the
//! narration it was given. Until this module the pane believed the exit
//! code, so a run that never exported still said the edit had shipped.
//!
//! The check is the one the fixed spine already walks at the end of its own
//! program ([`crate::director_program`]'s verify and watch stages): probe
//! the exported file with the engine's own reader, measure the mix against
//! the platform target, re-read the words and score the picture. Both paths
//! therefore answer to one contract, and the receipt is the same shape
//! whichever path produced it.
//!
//! Every call goes through the pane's own [`Toolbox`] and [`Backend`], so
//! what is asked here means exactly what the same call from an agent means.

use std::sync::atomic::{AtomicBool, Ordering};

use concat_director::{
    AcceptanceReceipt, GateRecord, GateStatus, Stage, StageOutcome, StageRecord, ToolCall,
    ToolCaller, ToolError, Verdict, derive_gates,
};
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{Context, Toolbox};

/// What a finished Director run is asked to prove: the project it edited
/// and the export it was told to land.
pub struct DirectorCheck<'a> {
    /// The sibling project the run edited.
    pub project: &'a str,
    /// The file the run was told to land, and the only one worth probing.
    pub export: &'a str,
}

/// The check's answer: the receipt, and whether the pane may present the
/// run as a finished edit. A receipt the gates did not accept keeps the
/// sibling project and names the gates that refused it - the edit is not
/// thrown away, but it is not called finished either.
#[derive(Clone, Debug)]
pub struct DirectorAcceptance {
    /// What was asked, what answered, and the standing the gates earn.
    pub receipt: AcceptanceReceipt,
}

impl DirectorAcceptance {
    /// Whether the run may be presented as a finished edit.
    pub fn accepted(&self) -> bool {
        self.receipt.verdict == Verdict::Accepted
    }
}

/// The independent media checks, in dependency order. The contact
/// sheet produced by watch_render is passed to craft_critique in check().
pub fn checks(project: &str, export: &str) -> [(&'static str, Value); 3] {
    [
        ("motion_probe", json!({ "path": export })),
        ("measure_loudness", json!({ "path": export })),
        (
            "watch_render",
            json!({ "path": export, "project": project, "audio": true }),
        ),
    ]
}

/// Read the exact plan artifact the Director is instructed to persist.
/// A missing or malformed plan is not fabricated into a passing story gate.
fn saved_plan(project: &str) -> Option<Value> {
    let path = std::path::Path::new(project).join("edit-plan.json");
    let bytes = std::fs::read(path).ok()?;
    let plan: Value = serde_json::from_slice(&bytes).ok()?;
    plan.is_object().then_some(plan)
}

/// Rebuild the expected narration coordinates from the sibling handoff.
/// The mapping is written by the pane from the source timeline's actual
/// clip placements; using it avoids guessing what words the Director meant
/// to preserve.
fn expected_takes(project: &str) -> Result<Vec<Value>, String> {
    let path = std::path::Path::new(project).join("voiceover-mapping.json");
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("voiceover mapping is unavailable: {error}"))?;
    let mapping: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("voiceover mapping is invalid JSON: {error}"))?;
    let segments = mapping
        .get("segments")
        .and_then(Value::as_array)
        .filter(|segments| !segments.is_empty())
        .ok_or_else(|| "voiceover mapping has no segments".to_owned())?;

    let mut rows = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let invalid = |field: &str| {
            format!(
                "voiceover mapping segment {} has an invalid {field}",
                index + 1
            )
        };
        let take = segment
            .get("take")
            .and_then(Value::as_str)
            .filter(|take| !take.trim().is_empty())
            .ok_or_else(|| invalid("take path"))?;
        let start = segment
            .get("start")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(|| invalid("start time"))?;
        let source_in = segment
            .get("source_in")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(|| invalid("source-in time"))?;
        let speed = segment
            .get("speed")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| invalid("playback speed"))?;
        let mut row = json!({
            "path": take,
            "start": start,
            "sourceIn": source_in,
            "speed": speed,
        });
        if let Some(source_out) = segment.get("source_out") {
            let source_out = source_out
                .as_f64()
                .filter(|value| value.is_finite() && *value > source_in)
                .ok_or_else(|| invalid("source-out time"))?;
            row["sourceOut"] = json!(source_out);
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Runs the check: every call, the replies it drew, and the receipt those
/// replies earn. Nothing here can pass on a promise - a gate reads only an
/// answer this function itself received.
pub fn check(
    check: &DirectorCheck<'_>,
    toolbox: &Toolbox,
    backend: &mut dyn Backend,
    cancelled: &AtomicBool,
) -> DirectorAcceptance {
    let mut caller = CheckCaller {
        toolbox,
        backend,
        cancelled,
    };
    let mut observed: Vec<(ToolCall, Value)> = Vec::new();
    let mut answered: Vec<String> = Vec::new();

    // Story validation is evidence only when the Director persisted the
    // exact plan it validated. Do not infer this gate from a successful
    // process exit or from the existence of a creative brief.
    if let Some(plan) = saved_plan(check.project) {
        let arguments = json!({ "plan": plan });
        if let Some(reply) = caller.ask("plan_validate", arguments.clone()) {
            answered.push("plan_validate".to_owned());
            observed.push((
                ToolCall {
                    tool: "plan_validate".to_owned(),
                    arguments,
                },
                reply,
            ));
        }
    }

    let mut watch_sheet = None;
    for (tool, arguments) in checks(check.project, check.export) {
        if let Some(reply) = caller.ask(tool, arguments.clone()) {
            answered.push(tool.to_owned());
            if tool == "watch_render" && reply.get("pngPath").and_then(Value::as_str).is_some() {
                watch_sheet = Some(reply.clone());
            }
            observed.push((
                ToolCall {
                    tool: tool.to_owned(),
                    arguments,
                },
                reply,
            ));
        }
    }

    // craft_critique accepts a watch_render sheet, not an export path.
    // Passing the actual sheet ensures the critic scores the rendered file.
    if let Some(sheet) = watch_sheet {
        let arguments = json!({
            "sheet": sheet,
            "project": check.project,
            "autoSetup": false,
        });
        if let Some(reply) = caller.ask("craft_critique", arguments.clone()) {
            answered.push("craft_critique".to_owned());
            observed.push((
                ToolCall {
                    tool: "craft_critique".to_owned(),
                    arguments,
                },
                reply,
            ));
        }
    }

    // Verify the rendered narration against source takes and their exact
    // timeline placement from voiceover-mapping.json. An absent mapping
    // leaves the required words gate skipped, never falsely passed.
    let mut warnings = Vec::new();
    match expected_takes(check.project) {
        Ok(takes) => {
            let arguments = json!({
                "action": "verify",
                "expectedTakes": takes,
                "actualPath": check.export,
            });
            if let Some(reply) = caller.ask("emotion", arguments.clone()) {
                answered.push("emotion".to_owned());
                observed.push((
                    ToolCall {
                        tool: "emotion".to_owned(),
                        arguments,
                    },
                    reply,
                ));
            }
        }
        Err(error) => warnings.push(error),
    }
    if cancelled.load(Ordering::Relaxed) {
        warnings.push(
            "acceptance checks were cancelled before all required evidence was collected"
                .to_owned(),
        );
    }

    let gates = derive_gates(&observed);
    let verdict = verdict_of(&gates);
    let failure = gates
        .iter()
        .filter(|gate| gate.status != GateStatus::Pass)
        .map(|gate| format!("{}: {}", gate.gate, gate.evidence))
        .collect::<Vec<_>>()
        .join("; ");
    let stage = StageRecord {
        stage: Stage::Verify,
        outcome: match verdict {
            Verdict::Accepted => StageOutcome::Completed,
            Verdict::Incomplete => StageOutcome::Skipped,
            Verdict::Rejected => StageOutcome::Failed,
        },
        calls: answered,
        failure: (!failure.is_empty()).then_some(failure),
    };
    let receipt = AcceptanceReceipt::new(check.project, check.export, vec![stage], gates, warnings);
    DirectorAcceptance { receipt }
}

/// The standing the gates earn: a failure rejects, a required gate with no
/// evidence leaves the run incomplete, and only a fully answered set is
/// accepted.
fn verdict_of(gates: &[GateRecord]) -> Verdict {
    if gates.iter().any(|gate| gate.status == GateStatus::Fail) {
        return Verdict::Rejected;
    }
    if gates
        .iter()
        .any(|gate| gate.required && gate.status == GateStatus::Skipped)
    {
        return Verdict::Incomplete;
    }
    Verdict::Accepted
}

/// A caller that asks its questions through the pane's own toolbox - the
/// same path an agent's `tools/call` takes.
struct CheckCaller<'a> {
    toolbox: &'a Toolbox,
    backend: &'a mut dyn Backend,
    cancelled: &'a AtomicBool,
}

impl CheckCaller<'_> {
    /// One question and its answer, or None when cancelled or when the
    /// tool refused/is not served. Missing evidence is omitted so the
    /// corresponding required gate is recorded as skipped, not failed.
    fn ask(&mut self, tool: &str, arguments: Value) -> Option<Value> {
        // Cancellation is checked before every independent acceptance call.
        // The active tool also receives the same flag through Context, so
        // long-running probes can stop at their own cancellation points.
        if self.cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let context = Context {
            backend: &mut *self.backend,
            cancelled: self.cancelled,
            progress: None,
        };
        match self.toolbox.call(tool, arguments, context) {
            Some(outcome) if !outcome.is_error => {
                Some(outcome.structured.unwrap_or_else(|| json!({})))
            }
            _ => None,
        }
    }
}

impl ToolCaller for CheckCaller<'_> {
    fn call(&mut self, call: &ToolCall) -> Result<Value, ToolError> {
        Ok(self
            .ask(&call.tool, call.arguments.clone())
            .unwrap_or_else(|| json!({})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_checks_use_the_export_and_create_a_real_contact_sheet() {
        let asked = checks("/edits/Reel", "/edits/Reel/export/reel.mp4");
        let tools: Vec<&str> = asked.iter().map(|(tool, _)| *tool).collect();
        assert_eq!(tools, ["motion_probe", "measure_loudness", "watch_render"]);
        assert_eq!(asked[0].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[1].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[2].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[2].1["project"], "/edits/Reel");
        assert_eq!(asked[2].1["audio"], true);
    }

    #[test]
    fn expected_takes_are_reconstructed_from_the_handoff_mapping() {
        let dir =
            std::env::temp_dir().join(format!("funcut-acceptance-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("voiceover-mapping.json"),
            br#"{"segments":[{"take":"/voice/take.wav","start":3.0,"source_in":1.0,"source_out":5.0,"speed":2.0}]}"#,
        )
        .unwrap();
        let rows = expected_takes(dir.to_str().unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["path"], "/voice/take.wav");
        assert_eq!(rows[0]["start"], 3.0);
        assert_eq!(rows[0]["sourceIn"], 1.0);
        assert_eq!(rows[0]["sourceOut"], 5.0);
        assert_eq!(rows[0]["speed"], 2.0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_voiceover_mapping_is_not_silently_partially_verified() {
        let dir =
            std::env::temp_dir().join(format!("funcut-acceptance-bad-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("voiceover-mapping.json"),
            br#"{"segments":[{"take":"/voice/good.wav","start":2.0,"source_in":0.0,"speed":1.0},{"take":"/voice/bad.wav","start":"unknown","source_in":0.0,"speed":1.0}]}"#,
        ).unwrap();
        let result = expected_takes(dir.to_str().unwrap());
        assert!(
            result.is_err(),
            "one malformed segment must invalidate the entire mapping"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_check_is_a_tool_the_director_surface_serves() {
        for tool in [
            "plan_validate",
            "motion_probe",
            "measure_loudness",
            "watch_render",
            "craft_critique",
            "emotion",
        ] {
            assert!(
                crate::protocol::DIRECTOR_ALLOWLIST.contains(&tool),
                "{tool} is asked for but not served to the Director"
            );
        }
    }

    #[test]
    fn a_failed_gate_rejects_and_a_missing_required_gate_is_incomplete() {
        let failed = vec![GateRecord::new(
            "export-integrity",
            GateStatus::Fail,
            "the reader answered without a probe",
            true,
        )];
        assert_eq!(verdict_of(&failed), Verdict::Rejected);

        let unmeasured = vec![GateRecord::new(
            "words",
            GateStatus::Skipped,
            "the run never compared the export's words",
            true,
        )];
        assert_eq!(verdict_of(&unmeasured), Verdict::Incomplete);

        let clean = vec![GateRecord::new(
            "export-integrity",
            GateStatus::Pass,
            "the file opened",
            true,
        )];
        assert_eq!(verdict_of(&clean), Verdict::Accepted);
    }
}
