// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! What a finished Director run is held to before the pane calls it done.
//!
//! The Director ends its turn and the harness exits zero. That says the
//! agent stopped - not that the edit exists, plays, or carries the
//! narration it was given. Until this module the pane believed the exit
//! code, so a run that never saved and verified its timeline still said the edit had shipped.
//!
//! The Director's deliverable is the editable timeline: the check reads the
//! saved project, judges its composited picture and proves its lanes are
//! clean. An export is optional and adds the encoded-file gates: probe the file,
//! measure the mix, re-read the words and score the picture. Both paths therefore answer to one receipt
//! shape, but a timeline-ready run is not failed for not rendering.
//!
//! Every call goes through the pane's own [`Toolbox`] and [`Backend`], so
//! what is asked here means exactly what the same call from an agent means.

use std::sync::atomic::{AtomicBool, Ordering};

use concat_director::{
    AcceptanceReceipt, GateRecord, GateStatus, Stage, StageOutcome, StageRecord, ToolCall,
    ToolCaller, ToolError, Verdict, derive_gates, plan_has_voiceover,
};
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{Context, Toolbox};

/// How far a word may start from where the edit placed it, in the
/// acceptance's own `emotion` verify: the watcher's own line
/// (`VERIFICATION_SYNC_TOLERANCE_MS`, 60 ms) is the tolerance the whole
/// pipeline promises, and an acceptance 2.5x looser would pass exports
/// the agent believed it was held to.
pub const SYNC_TOLERANCE_MS: u64 = 60;

/// What a finished Director run is asked to prove: the project it edited,
/// and optionally the export it was told to land. The default Director
/// deliverable is the timeline, not a render: the user asked to customize
/// the cut, so the pane checks the open project and leaves the export to
/// the user (or to a later explicit render).
pub struct DirectorCheck<'a> {
    /// The sibling project the run edited.
    pub project: &'a str,
    /// The file the run was told to land, when it was asked to render one.
    pub export: Option<&'a str>,
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

/// Record a narration signal profile that could not be measured: the
/// failed profile is the evidence the signal-evidence gate will skip on,
/// so the run is honestly incomplete rather than falsely passing.
fn record_signal_failure(observed: &mut Vec<(ToolCall, Value)>, arguments: Value, message: String) {
    observed.push((
        ToolCall {
            tool: "voice_signals".to_owned(),
            arguments,
        },
        json!({ "ok": false, "error": message }),
    ));
}

/// The independent media checks, in dependency order. The contact
/// sheet produced by watch_render is passed to craft_critique in check().
///
/// With an export, the checks prove the encoded file: readable streams,
/// platform loudness, the rendered picture, and lane collisions. Without
/// one - the default Director deliverable - they prove the timeline the
/// user is about to customize: it reads, its composited picture is judged,
/// and no two clips share a lane.
pub fn checks(project: &str, export: Option<&str>) -> Vec<(&'static str, Value)> {
    match export {
        Some(export) => vec![
            ("timeline_read", json!({ "path": project })),
            ("motion_probe", json!({ "path": export })),
            ("measure_loudness", json!({ "path": export })),
            (
                "watch_render",
                json!({ "path": export, "project": project, "audio": true }),
            ),
            ("collision_check", json!({ "path": project })),
        ],
        None => vec![
            ("timeline_read", json!({ "path": project })),
            ("watch_render", json!({ "project": project, "audio": true })),
            ("collision_check", json!({ "path": project })),
        ],
    }
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

/// The expected narration coordinates the EDIT ITSELF ended with: the
/// timeline's voiceover clips, each mapped as the take it plays. A
/// Director that tightened the narration - the skill's own Phase 7, the
/// ripple the auto prompt permits - moves every later word, and the
/// mapping the pane wrote before the run would then fail a compliant
/// edit. The mapping remains the fallback when no timeline answers: the
/// placements the run began with, as they always were.
fn timeline_takes(view: &Value, takes: &[String]) -> Vec<Value> {
    let media = crate::director_program::media_paths(view);
    view["timeline"]["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|clip| {
            clip["media"].as_str().is_some_and(|id| {
                media
                    .iter()
                    .any(|(mid, path)| mid == id && takes.iter().any(|take| take == path))
            })
        })
        .map(|clip| {
            let (source_in, source_out, speed) = crate::director_program::clip_file_span(clip);
            json!({
                "path": media
                    .iter()
                    .find(|(mid, _)| Some(mid.as_str()) == clip["media"].as_str())
                    .map(|(_, path)| path.clone())
                    .unwrap_or_default(),
                "start": clip["start"].as_f64().unwrap_or(0.0),
                "sourceIn": source_in,
                "sourceOut": source_out,
                "speed": speed,
            })
        })
        .collect()
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
    let plan = saved_plan(check.project);
    if let Some(plan) = plan.as_ref() {
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

    // Re-read the voice profile for every distinct narration take. This
    // independent pass makes a fabricated signalLog confidence fail even
    // when the plan's own structural validator accepts its vocabulary.
    let signal_required = plan.as_ref().is_some_and(plan_has_voiceover);
    let arc_required = plan.as_ref().is_some_and(|plan| {
        plan.get("signalLog")
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry
                        .get("signals")
                        .and_then(Value::as_array)
                        .is_some_and(|signals| {
                            signals.iter().any(|signal| signal.as_str() == Some("arc"))
                        })
                })
            })
    });
    if signal_required {
        match expected_takes(check.project) {
            Ok(mapping_rows) => {
                let paths: std::collections::BTreeSet<String> = mapping_rows
                    .iter()
                    .filter_map(|row| row.get("path").and_then(Value::as_str).map(str::to_owned))
                    .collect();
                for path in paths {
                    let arguments = json!({ "audio": path });
                    if let Some(reply) = caller.ask("voice_signals", arguments.clone()) {
                        answered.push("voice_signals".to_owned());
                        let lines = reply.get("lines").cloned();
                        observed.push((
                            ToolCall {
                                tool: "voice_signals".to_owned(),
                                arguments,
                            },
                            reply,
                        ));
                        if arc_required {
                            let emotion_arguments = json!({
                                "action": "extract",
                                "lines": lines.clone().unwrap_or_else(|| json!([])),
                                "project": check.project,
                            });
                            if let Some(emotion_reply) =
                                caller.ask("emotion", emotion_arguments.clone())
                            {
                                answered.push("emotion".to_owned());
                                observed.push((
                                    ToolCall {
                                        tool: "emotion".to_owned(),
                                        arguments: emotion_arguments,
                                    },
                                    emotion_reply,
                                ));
                            } else {
                                observed.push((
                                    ToolCall {
                                        tool: "emotion".to_owned(),
                                        arguments: emotion_arguments,
                                    },
                                    json!({
                                        "ok": false,
                                        "error": "emotion extract could not independently measure the voiceover arc"
                                    }),
                                ));
                            }
                        }
                    } else {
                        record_signal_failure(
                            &mut observed,
                            arguments,
                            "voice_signals could not independently read a narration take"
                                .to_owned(),
                        );
                    }
                }
                if mapping_rows.is_empty() {
                    record_signal_failure(
                        &mut observed,
                        json!({}),
                        "voiceover mapping contains no distinct narration takes".to_owned(),
                    );
                }
            }
            Err(error) => record_signal_failure(&mut observed, json!({}), error),
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

    // Verify the rendered narration against the take pieces the EDIT
    // ITSELF ended with - read live from the timeline, the same
    // derivation the fixed spine verifies with - and against the mapping
    // the pane wrote when no timeline answers. An absent mapping leaves
    // the words gate skipped, never falsely passed. Without an export
    // there is no encoded soundtrack to re-read; the timeline itself is
    // the deliverable and the words gate is reported as not required.
    let mut warnings = Vec::new();
    if let Some(export) = check.export {
        match expected_takes(check.project) {
            Ok(mapping_rows) => {
                let take_paths: Vec<String> = mapping_rows
                    .iter()
                    .filter_map(|row| row.get("path").and_then(Value::as_str).map(str::to_owned))
                    .collect();
                // The live timeline, when it answers: a Director that rippled
                // the dead air out moved every later word, and verifying
                // those words against the pre-run mapping would fail a
                // compliant edit.
                let expected = caller
                    .ask("timeline_read", json!({ "path": check.project }))
                    .and_then(|view| {
                        let rows = timeline_takes(&view, &take_paths);
                        (!rows.is_empty()).then_some(rows)
                    })
                    .unwrap_or(mapping_rows);
                let arguments = json!({
                    "action": "verify",
                    "expectedTakes": expected,
                    "actualPath": export,
                    "toleranceMs": SYNC_TOLERANCE_MS,
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
    } else {
        warnings.push(
            "timeline-ready acceptance: no export was requested, so the words and encoded-audio gates are not required; the saved timeline is the deliverable and can be exported later."
                .to_owned(),
        );
    }
    if cancelled.load(Ordering::Relaxed) {
        warnings.push(
            "acceptance checks were cancelled before all required evidence was collected"
                .to_owned(),
        );
    }

    let mut gates = derive_gates(&observed);
    // The Director is the creative path: its plan must reach creativeReady,
    // its rendered picture must be judged, and the timeline it leaves must
    // be free of shared lanes. The fixed spine cannot promise the first, so
    // `derive_gates` leaves the picture and lane gates optional; here, where
    // only the Director's run is checked, they are promoted to required.
    for gate in &mut gates {
        if gate.gate == "visual" || gate.gate == "collisions" {
            gate.required = true;
        }
        // Timeline-ready acceptance: the words, LUFS and encoded-stream
        // gates need a render. Without one they are honestly skipped, not
        // silently passed, and the timeline gate below is the required
        // proof that the saved edit exists and reads.
        if check.export.is_none()
            && matches!(gate.gate.as_str(), "words" | "audio" | "export-integrity")
        {
            gate.required = false;
        }
    }
    let plan_validation = observed
        .iter()
        .rev()
        .find(|(call, _)| call.tool == "plan_validate")
        .and_then(|(_, reply)| reply.get("creativeReady").and_then(Value::as_bool));
    gates.push(GateRecord::new(
        "creative",
        match plan_validation {
            Some(true) => GateStatus::Pass,
            Some(false) => GateStatus::Fail,
            None => GateStatus::Skipped,
        },
        match plan_validation {
            Some(ready) => format!("the Director's plan reported creativeReady={ready}"),
            None => "the saved plan was not independently validated as creativeReady".to_owned(),
        },
        true,
    ));
    let verdict = verdict_of(&gates);
    let failure = gates
        .iter()
        .filter(|gate| {
            gate.status == GateStatus::Fail || (gate.required && gate.status == GateStatus::Skipped)
        })
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
    let receipt = AcceptanceReceipt::new(
        check.project,
        check.export.unwrap_or("timeline"),
        vec![stage],
        gates,
        warnings,
    );
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
    use concat_director::timeline_integrity;

    #[test]
    fn media_checks_use_the_export_and_create_a_real_contact_sheet() {
        let asked = checks("/edits/Reel", Some("/edits/Reel/export/reel.mp4"));
        let tools: Vec<&str> = asked.iter().map(|(tool, _)| *tool).collect();
        assert_eq!(
            tools,
            [
                "timeline_read",
                "motion_probe",
                "measure_loudness",
                "watch_render",
                "collision_check"
            ]
        );
        assert_eq!(asked[0].1["path"], "/edits/Reel");
        assert_eq!(asked[1].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[2].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[3].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[3].1["project"], "/edits/Reel");
        assert_eq!(asked[4].1["path"], "/edits/Reel");
        assert_eq!(asked[2].1["audio"], true);
    }

    #[test]
    fn timeline_ready_checks_read_the_open_project_not_a_render() {
        let asked = checks("/edits/Reel", None);
        let tools: Vec<&str> = asked.iter().map(|(tool, _)| *tool).collect();
        assert_eq!(tools, ["timeline_read", "watch_render", "collision_check"]);
        assert_eq!(asked[0].1["path"], "/edits/Reel");
        assert_eq!(asked[1].1["project"], "/edits/Reel");
        assert!(asked[1].1.get("path").is_none());
        assert_eq!(asked[1].1["audio"], true);
        assert_eq!(asked[2].1["path"], "/edits/Reel");
    }

    #[test]
    fn timeline_integrity_accepts_valid_file_backed_and_title_clips() {
        let dir =
            std::env::temp_dir().join(format!("funcut-acceptance-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        std::fs::write(&source, b"fixture").unwrap();
        let view = json!({
            "timeline": {
                "tracks": [{"id":"video-1"},{"id":"titles-1"}],
                "clips": [
                    {"id":"c1","kind":"video","track":"video-1","start":0.0,"duration":4.0,"sourceStart":0.0,"media":"m1"},
                    {"id":"c2","kind":"text","track":"titles-1","start":1.0,"duration":2.0}
                ]
            },
            "media": [{"id":"m1","path":source.to_string_lossy()}]
        });
        let result = timeline_integrity(&view).unwrap();
        assert!(result.contains("2 clips"));
        assert!(result.contains("1 file-backed"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn timeline_integrity_rejects_missing_media_tracks_and_source_files() {
        let missing_track = json!({
            "timeline":{"tracks":[],"clips":[
                {"id":"c1","kind":"text","track":"gone","start":0.0,"duration":1.0}
            ]},
            "media":[]
        });
        assert!(
            timeline_integrity(&missing_track)
                .unwrap_err()
                .contains("missing track")
        );

        let missing_media = json!({
            "timeline":{"tracks":[{"id":"v1"}],"clips":[
                {"id":"c1","kind":"video","track":"v1","start":0.0,"duration":1.0,"media":"gone"}
            ]},
            "media":[]
        });
        assert!(
            timeline_integrity(&missing_media)
                .unwrap_err()
                .contains("missing media")
        );

        let missing_file = json!({
            "timeline":{"tracks":[{"id":"v1"}],"clips":[
                {"id":"c1","kind":"video","track":"v1","start":0.0,"duration":1.0,"media":"m1"}
            ]},
            "media":[{"id":"m1","path":"/definitely/missing/funcut-source.mp4"}]
        });
        assert!(
            timeline_integrity(&missing_file)
                .unwrap_err()
                .contains("source file is missing")
        );
    }

    #[test]
    fn timeline_integrity_rejects_duplicate_bin_ids_and_source_overruns() {
        let duplicate_media = json!({
            "timeline":{"tracks":[{"id":"v1"}],"clips":[
                {"id":"c1","kind":"video","track":"v1","start":0.0,"duration":1.0,"media":"m1","sourceStart":0.0}
            ]},
            "media":[
                {"id":"m1","path":"/tmp/a.mp4"},
                {"id":"m1","path":"/tmp/b.mp4"}
            ]
        });
        assert!(
            timeline_integrity(&duplicate_media)
                .unwrap_err()
                .contains("duplicate media id")
        );

        let dir =
            std::env::temp_dir().join(format!("funcut-acceptance-overrun-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        std::fs::write(&source, b"fixture").unwrap();
        let source_overrun = json!({
            "timeline":{"tracks":[{"id":"v1"}],"clips":[
                {"id":"c1","kind":"video","track":"v1","start":0.0,"duration":4.0,"speed":2.0,"media":"m1","sourceStart":2.0}
            ]},
            "media":[{"id":"m1","path":source.to_string_lossy(),"duration":8.0}]
        });
        assert!(
            timeline_integrity(&source_overrun)
                .unwrap_err()
                .contains("reads past media")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn timeline_integrity_rejects_invalid_clip_timing_and_duplicate_ids() {
        let invalid_timing = json!({
            "timeline":{"tracks":[{"id":"v1"}],"clips":[
                {"id":"c1","kind":"text","track":"v1","start":-1.0,"duration":1.0}
            ]},
            "media":[]
        });
        assert!(
            timeline_integrity(&invalid_timing)
                .unwrap_err()
                .contains("invalid start")
        );

        let duplicate = json!({
            "timeline":{"tracks":[{"id":"v1"}],"clips":[
                {"id":"same","kind":"text","track":"v1","start":0.0,"duration":1.0},
                {"id":"same","kind":"text","track":"v1","start":1.0,"duration":1.0}
            ]},
            "media":[]
        });
        assert!(
            timeline_integrity(&duplicate)
                .unwrap_err()
                .contains("duplicates clip id")
        );
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

    /// The words gate verifies the edit the Director actually made: the
    /// live timeline's placements outrank the pre-run mapping, so a
    /// ripple that moved every later word is verified where the words
    /// now sit - not failed against where the pane first put them.
    #[test]
    fn the_live_timeline_speaks_over_the_pre_run_mapping() {
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 1.5, "duration": 3.0,
                  "sourceStart": 2.0, "speed": 1.0 },
                { "id": "t1", "kind": "text", "start": 0.0, "duration": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/voice/take.wav" }],
        });
        let rows = timeline_takes(&view, &["/voice/take.wav".to_owned()]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["path"], "/voice/take.wav");
        assert_eq!(rows[0]["start"], 1.5);
        assert_eq!(rows[0]["sourceIn"], 2.0);
        assert_eq!(rows[0]["sourceOut"], 5.0);
        assert_eq!(rows[0]["speed"], 1.0);
        // A take the mapping never named is not the narration, and a
        // timeline with none of them answers nothing - the mapping stays
        // the expected side then.
        assert!(timeline_takes(&view, &["/voice/other.wav".to_owned()]).is_empty());
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
            "collision_check",
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
