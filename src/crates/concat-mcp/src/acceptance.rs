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

use std::sync::atomic::AtomicBool;

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

/// The checks a run is asked to answer, in the order they are asked: the
/// tool, and the arguments it takes.
pub fn checks(export: &str) -> [(&'static str, Value); 4] {
    [
        // The file: the engine's own reader. A missing or unreadable file
        // answers without a probe, and that is the gate a craft score can
        // never stand in for.
        ("motion_probe", json!({ "path": export })),
        // The mix: the platform target the export is published against.
        ("measure_loudness", json!({ "path": export })),
        // The picture: the critic the fallback's watch stage reads too.
        ("craft_critique", json!({ "path": export })),
        // The words: what the export actually says. The Director ran its own
        // verify over the expected takes; this asks the same question over
        // the finished file, and a reply with no verdict is recorded as a
        // failure rather than a silent pass.
        (
            "emotion",
            json!({ "action": "verify", "actualPath": export }),
        ),
    ]
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
    for (tool, arguments) in checks(check.export) {
        let reply = caller.ask(tool, arguments.clone());
        if !reply.as_object().is_some_and(|reply| reply.is_empty()) {
            answered.push(tool.to_owned());
        }
        observed.push((
            ToolCall {
                tool: tool.to_owned(),
                arguments,
            },
            reply,
        ));
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
    let receipt =
        AcceptanceReceipt::new(check.project, check.export, vec![stage], gates, Vec::new());
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
    /// One question and its answer, or an empty object when the tool
    /// refused or is not served - which the gate reads as no evidence,
    /// never as a pass.
    fn ask(&mut self, tool: &str, arguments: Value) -> Value {
        let context = Context {
            backend: &mut *self.backend,
            cancelled: self.cancelled,
            progress: None,
        };
        match self.toolbox.call(tool, arguments, context) {
            Some(outcome) if !outcome.is_error => outcome.structured.unwrap_or_else(|| json!({})),
            _ => Value::Object(Default::default()),
        }
    }
}

impl ToolCaller for CheckCaller<'_> {
    fn call(&mut self, call: &ToolCall) -> Result<Value, ToolError> {
        Ok(self.ask(&call.tool, call.arguments.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_asks_for_the_file_the_mix_the_picture_and_the_words() {
        let asked = checks("/edits/Reel/export/reel.mp4");
        let tools: Vec<&str> = asked.iter().map(|(tool, _)| *tool).collect();
        assert_eq!(
            tools,
            [
                "motion_probe",
                "measure_loudness",
                "craft_critique",
                "emotion"
            ]
        );
        // Every call that reads the file reads the file the run was told to
        // land: a check of a different path proves nothing.
        assert_eq!(asked[0].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[1].1["path"], "/edits/Reel/export/reel.mp4");
        assert_eq!(asked[3].1["actualPath"], "/edits/Reel/export/reel.mp4");
    }

    #[test]
    fn every_check_is_a_tool_the_director_surface_serves() {
        // A check refused by the run's own server is the same silent success
        // this module exists to end.
        for (tool, _) in checks("/edits/Reel/export/reel.mp4") {
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
