// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The stages an auto-edit runs through, and the program that names
//! them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ToolCall;

/// One stage of an auto-edit. The names are the pipeline's own - the
/// watcher's numbered phases (`scripts/auto_director.py`), gathered into
/// the stops an unattended run reports - so a stage means on this side of
/// the seam what the same work means on the other.
///
/// A program need not run every stage (a plan without captions has no
/// [`Captions`](Stage::Captions) step), and the stage indices a progress
/// report carries are the program's own order, not this list's: the list
/// is the register the names come from, not a schedule.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// The voiceover lands in the project: found in the bin or imported,
    /// placed on its own lane, the plan's speed applied (the watcher's
    /// ninth step).
    HandoffVoiceover,
    /// Words and timings read from the take (the third step).
    Transcribe,
    /// The take is repaired before anything reads it - `enhance_speech`,
    /// `remove_hiss_hum_rumble` - and every later stage runs on the
    /// repaired file (the second step).
    AudioRepair,
    /// Pauses, emphasis and delivery measured from the voice itself (the
    /// fourth step).
    VoiceSignals,
    /// The take is understood: the emotional read, the footage plan the
    /// dialogue gaps ask for, the music gate (the fifth through seventh
    /// steps).
    Understand,
    /// The canonical edit plan is validated: schema, creative readiness,
    /// the signal-evidence gate (the eighth step).
    Plan,
    /// B-roll is shopped per timed segment, seen in a contact sheet
    /// before import, continuity-checked, replaced where flagged (the
    /// tenth through twelfth steps).
    Gather,
    /// Graphics are rendered: the motion bakes the slots that ask for
    /// them.
    Motion,
    /// The edit itself: stock imported and trimmed at the segment
    /// starts, transitions and entrances keyed, the collision and
    /// timeline checks run (the thirteenth, fourteenth and sixteenth
    /// steps).
    Timeline,
    /// The plan's captions are placed (the fifteenth step's captions).
    Captions,
    /// A preview is rendered to be watched - the frames the Watch stage
    /// scores.
    Preview,
    /// The cut is rendered and waited on to a terminal job state (the
    /// seventeenth step).
    Export,
    /// The actual output is inspected and scored: `watch_render`'s
    /// contact sheet, `craft_critique`'s score (the twentieth step).
    Watch,
    /// The critic's notes are answered per slot - a B-roll slot
    /// re-shopped with the current clip excluded, a graphic re-rendered
    /// with the named fix, a caption corrected - and the preview
    /// re-rendered and re-watched. At most twice, and never the same
    /// edit twice over one note.
    Revise,
    /// The export is certified: the words re-read from the render, the
    /// loudness measured and asserted, the timeline checked for overlaps,
    /// gaps and duplicates (the eighteenth and nineteenth steps).
    Verify,
}

impl Stage {
    /// Every stage, in the canonical order the pipeline runs them: the
    /// register, read left to right.
    pub const ALL: [Stage; 15] = [
        Stage::HandoffVoiceover,
        Stage::Transcribe,
        Stage::AudioRepair,
        Stage::VoiceSignals,
        Stage::Understand,
        Stage::Plan,
        Stage::Gather,
        Stage::Motion,
        Stage::Timeline,
        Stage::Captions,
        Stage::Preview,
        Stage::Export,
        Stage::Watch,
        Stage::Revise,
        Stage::Verify,
    ];

    /// The stage's short name - the same word its JSON carries, the word
    /// a log line or a warning list shows.
    pub fn label(self) -> &'static str {
        match self {
            Stage::HandoffVoiceover => "handoff-voiceover",
            Stage::Transcribe => "transcribe",
            Stage::AudioRepair => "audio-repair",
            Stage::VoiceSignals => "voice-signals",
            Stage::Understand => "understand",
            Stage::Plan => "plan",
            Stage::Gather => "gather",
            Stage::Motion => "motion",
            Stage::Timeline => "timeline",
            Stage::Captions => "captions",
            Stage::Preview => "preview",
            Stage::Export => "export",
            Stage::Watch => "watch",
            Stage::Revise => "revise",
            Stage::Verify => "verify",
        }
    }
}

/// What a stage's failure means for the run.
///
/// Most stages fail the run: an auto-edit that cannot place its
/// voiceover has nothing to ship. The warn policy is for the stages
/// whose failure is a flaw in an otherwise finished edit - a critic note
/// that could not be answered, a verifier's warning - the failures the
/// revise loop records per slot and ships anyway, so the run ends
/// [`CompletedWithWarnings`](crate::state::AutoEditState::CompletedWithWarnings)
/// with every one of them written down.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorPolicy {
    /// The stage's failure fails the run.
    Fail,
    /// The stage's failure is recorded as a warning and the run goes on.
    Warn,
}

/// What a stage's replies must say for the stage to count as done: the
/// gate the silent-success bugs closed. A tool that answers `Ok` with a
/// measurement that fails the edit - a loudness reading that misses the
/// platform target, a craft score under the bar, a transcript that does
/// not match the edit - used to sail a run straight to
/// [`Completed`](crate::state::AutoEditState::Completed); a gate is the
/// program's own word for what the replies must prove.
///
/// A gate is checked against every reply the stage's successful calls
/// drew that carries its field - a reply without the field is not asked,
/// the gate speaks to the calls that answer it - and a gate no reply
/// answered is the stage's own failure: a verify nobody ran is not a
/// verify. A failed gate is the stage's failure under the stage's own
/// [`ErrorPolicy`]: on the fail policy the run stops; on the warn policy
/// the flaw is recorded and the run ships warned.
#[derive(Clone, PartialEq, Debug)]
pub enum ReplyGate {
    /// A boolean field that must be `true` in every reply carrying it.
    True {
        /// The reply's field: `platformTargetMet` on a loudness reading.
        field: &'static str,
        /// What the gate proves, said as the failure's own sentence.
        what: &'static str,
    },
    /// A string field that must equal `equals` in every reply carrying
    /// it.
    Word {
        /// The reply's field: `verdict` on an export comparison.
        field: &'static str,
        /// The only word that passes: `"pass"`.
        equals: &'static str,
        /// What the gate proves, said as the failure's own sentence.
        what: &'static str,
    },
    /// A numeric field that must be at least `minimum` in every reply
    /// carrying it.
    AtLeast {
        /// The reply's field: `overall` on a craft score.
        field: &'static str,
        /// The smallest number that passes: `7.0` on the craft bar.
        minimum: f64,
        /// What the gate proves, said as the failure's own sentence.
        what: &'static str,
    },
}

impl ReplyGate {
    /// The reply's field this gate reads - the word a "nobody answered"
    /// failure names.
    pub fn field(&self) -> &'static str {
        match self {
            ReplyGate::True { field, .. }
            | ReplyGate::Word { field, .. }
            | ReplyGate::AtLeast { field, .. } => field,
        }
    }

    /// The reply's answer to this gate: `None` when the reply does not
    /// carry the field (not asked), `Some(Ok(()))` when it satisfies the
    /// gate, `Some(Err(seen))` with the value that failed it.
    pub fn checked_by(&self, reply: &serde_json::Value) -> Option<Result<(), String>> {
        match self {
            ReplyGate::True { field, what } => match reply.get(*field)? {
                Value::Bool(true) => Some(Ok(())),
                seen => Some(Err(format!(
                    "{what}: the reply said {field}={seen}, not true"
                ))),
            },
            ReplyGate::Word {
                field,
                equals,
                what,
            } => match reply.get(*field)? {
                Value::String(word) if word == equals => Some(Ok(())),
                seen => Some(Err(format!(
                    "{what}: the reply said {field}={seen}, not \"{equals}\""
                ))),
            },
            ReplyGate::AtLeast {
                field,
                minimum,
                what,
            } => match reply.get(*field)? {
                Value::Number(number) => match number.as_f64() {
                    Some(seen) if seen >= *minimum => Some(Ok(())),
                    Some(seen) => Some(Err(format!(
                        "{what}: the reply said {field}={seen}, under the {minimum} bar"
                    ))),
                    None => Some(Err(format!(
                        "{what}: the reply said {field}={number}, not a finite number"
                    ))),
                },
                seen => Some(Err(format!(
                    "{what}: the reply said {field}={seen}, not a number"
                ))),
            },
        }
    }
}

/// One stage of a program: the calls it makes, in order, what a
/// failure means, and what its replies must prove.
#[derive(Clone, PartialEq, Debug)]
pub struct StageStep {
    /// The stage this step runs.
    pub stage: Stage,
    /// What this stage's failure means for the run.
    pub policy: ErrorPolicy,
    /// The calls the stage makes, in the order it makes them.
    pub calls: Vec<ToolCall>,
    /// What the stage's replies must say for the stage to count as done:
    /// the silent-success gates, checked after every call landed.
    pub gates: Vec<ReplyGate>,
}

impl StageStep {
    /// A stage whose failure fails the run, making `calls`.
    pub fn run(stage: Stage, calls: Vec<ToolCall>) -> StageStep {
        StageStep {
            stage,
            policy: ErrorPolicy::Fail,
            calls,
            gates: Vec::new(),
        }
    }

    /// The same step, with the warn policy instead: its failure is
    /// recorded and the run goes on.
    pub fn warn(mut self) -> StageStep {
        self.policy = ErrorPolicy::Warn;
        self
    }

    /// The same step, asking one more thing of the stage's replies: a
    /// gate is checked after every call landed, and a failed gate is the
    /// stage's own failure under the step's policy.
    pub fn gate(mut self, gate: ReplyGate) -> StageStep {
        self.gates.push(gate);
        self
    }
}

/// A whole auto-edit as the runner is given it: where the export will
/// land, and the stages to run.
///
/// The export path is known before the run starts - the plan promises
/// it - so the runner carries it into the run's terminal states rather
/// than reading it back from a reply; the run's
/// [`verification`](Stage::Verify) record is the one thing the runner
/// takes from the program's own replies: the last successful answer of
/// the Verify stage, the evidence an acceptance read wants.
#[derive(Clone, PartialEq, Debug)]
pub struct EditProgram {
    /// Where the plan says the export lands.
    pub export: PathBuf,
    /// The stages, in the order the program runs them.
    pub steps: Vec<StageStep>,
}

impl EditProgram {
    /// A program exporting to `export`, running `steps`.
    pub fn new(export: PathBuf, steps: Vec<StageStep>) -> EditProgram {
        EditProgram { export, steps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_register_holds_fifteen_stages_and_the_first_and_last_say_so() {
        assert_eq!(Stage::ALL.len(), 15);
        assert_eq!(Stage::ALL.first(), Some(&Stage::HandoffVoiceover));
        assert_eq!(Stage::ALL.last(), Some(&Stage::Verify));
    }

    #[test]
    fn a_label_is_the_word_the_json_carries() {
        assert_eq!(Stage::HandoffVoiceover.label(), "handoff-voiceover");
        assert_eq!(Stage::AudioRepair.label(), "audio-repair");
        assert_eq!(Stage::VoiceSignals.label(), "voice-signals");
    }

    #[test]
    fn every_stage_crosses_as_its_label() {
        for stage in Stage::ALL {
            let word = serde_json::to_value(stage).expect("a stage is JSON");
            assert_eq!(word, serde_json::json!(stage.label()));
            let back: Stage = serde_json::from_value(word).expect("a stage reads back from JSON");
            assert_eq!(back, stage);
        }
    }

    #[test]
    fn a_step_warns_only_when_asked_to() {
        let calls = vec![ToolCall {
            tool: "craft_critique".to_owned(),
            arguments: serde_json::json!({ "overall": 6 }),
        }];
        let fail = StageStep::run(Stage::Watch, calls.clone());
        assert_eq!(fail.policy, ErrorPolicy::Fail);
        let warn = StageStep::run(Stage::Watch, calls).warn();
        assert_eq!(warn.policy, ErrorPolicy::Warn);
    }

    #[test]
    fn a_true_gate_reads_the_replys_own_word() {
        let gate = ReplyGate::True {
            field: "platformTargetMet",
            what: "the mix missed the platform loudness target",
        };
        assert_eq!(gate.field(), "platformTargetMet");
        // A reply that carries the word, said truly.
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "platformTargetMet": true })),
            Some(Ok(()))
        );
        // The word, said falsely: the failure names the value it saw.
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "platformTargetMet": false })),
            Some(Err(
                "the mix missed the platform loudness target: the reply \
                 said platformTargetMet=false, not true"
                    .to_owned()
            ))
        );
        // A reply that carries nothing: not asked.
        assert_eq!(gate.checked_by(&serde_json::json!({ "job": "e1" })), None);
        // The wrong shape is the word said wrongly, not a pass.
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "platformTargetMet": "yes" })),
            Some(Err(
                "the mix missed the platform loudness target: the reply \
                 said platformTargetMet=\"yes\", not true"
                    .to_owned()
            ))
        );
    }

    #[test]
    fn a_word_gate_wants_one_word() {
        let gate = ReplyGate::Word {
            field: "verdict",
            equals: "pass",
            what: "the export comparison did not pass",
        };
        assert_eq!(gate.field(), "verdict");
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "verdict": "pass" })),
            Some(Ok(()))
        );
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "verdict": "fail" })),
            Some(Err("the export comparison did not pass: the reply said \
                 verdict=\"fail\", not \"pass\""
                .to_owned()))
        );
        assert_eq!(gate.checked_by(&serde_json::json!({})), None);
    }

    #[test]
    fn an_at_least_gate_reads_the_bar() {
        let gate = ReplyGate::AtLeast {
            field: "overall",
            minimum: 7.0,
            what: "the cut scored under the craft bar",
        };
        assert_eq!(gate.field(), "overall");
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "overall": 7.0 })),
            Some(Ok(()))
        );
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "overall": 8.5 })),
            Some(Ok(()))
        );
        // Under the bar: the failure names the number it saw.
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "overall": 6 })),
            Some(Err("the cut scored under the craft bar: the reply said \
                 overall=6, under the 7 bar"
                .to_owned()))
        );
        // Not a number at all.
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "overall": "8.5" })),
            Some(Err("the cut scored under the craft bar: the reply said \
                 overall=\"8.5\", not a number"
                .to_owned()))
        );
        assert_eq!(
            gate.checked_by(&serde_json::json!({ "engine": "heuristic" })),
            None
        );
    }

    #[test]
    fn a_step_collects_its_gates() {
        let step = StageStep::run(Stage::Verify, vec![])
            .gate(ReplyGate::True {
                field: "platformTargetMet",
                what: "the mix missed the platform loudness target",
            })
            .gate(ReplyGate::AtLeast {
                field: "overall",
                minimum: 7.0,
                what: "the cut scored under the craft bar",
            });
        assert_eq!(step.gates.len(), 2);
        assert_eq!(step.gates[0].field(), "platformTargetMet");
        assert_eq!(step.gates[1].field(), "overall");
    }
}
