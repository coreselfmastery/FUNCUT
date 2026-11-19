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
    /// The music bed is shopped, imported, read back and ducked under
    /// the voice - only where the music gate said the take carries none
    /// of its own.
    Music,
    /// The sound accents land: the library shopped, one accent placed
    /// on each of the measured emphasis peaks.
    Sfx,
    /// The cut is looked at before it is paid for: `watch_render` samples
    /// the OPEN timeline - not the export, which has not been rendered
    /// yet - and `craft_critique` scores the sheet, so the numbers the
    /// revise answers exist before the encode is spent.
    Preview,
    /// The cut is rendered and waited on to a terminal job state (the
    /// seventeenth step).
    Export,
    /// The actual output is inspected and scored: `watch_render`'s
    /// contact sheet, `craft_critique`'s score (the twentieth step).
    Watch,
    /// The critic's measured faults are answered with targeted edits: a
    /// fill for letterbox bars, a bounded head trim for a black opening,
    /// a push-in on every *other* shot for static cuts - one undoable
    /// batch, then the same frames scored again. A cut that measured
    /// clean issues no edit at all, and a fault with no honest fix in the
    /// editing vocabulary is left to the report rather than papered over.
    Revise,
    /// The export is certified: the words re-read from the render, the
    /// loudness measured and asserted, the timeline checked for overlaps,
    /// gaps and duplicates (the eighteenth and nineteenth steps).
    Verify,
}

impl Stage {
    /// Every stage, in the order the AUTO EDIT program runs them: the
    /// register, read left to right. [`Runner::completed`](crate::Runner::completed)
    /// reports a resumed run in this order, so it must be the program's
    /// order and not the enum's.
    pub const ALL: [Stage; 17] = [
        Stage::HandoffVoiceover,
        Stage::AudioRepair,
        Stage::Transcribe,
        Stage::VoiceSignals,
        Stage::Understand,
        Stage::Plan,
        Stage::Timeline,
        Stage::Gather,
        Stage::Captions,
        Stage::Motion,
        Stage::Music,
        Stage::Sfx,
        Stage::Preview,
        Stage::Export,
        Stage::Verify,
        Stage::Watch,
        Stage::Revise,
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
            Stage::Music => "music",
            Stage::Sfx => "sfx",
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

/// A closure that derives a stage's next call from the replies the run
/// has drawn so far: the whole reply log, in call order, successful calls
/// only, growing as the stage's own calls land. Asked again and again
/// until it answers `None`: the reactive queue a smart stage stands on.
/// `Some(call)` is run exactly as a static stage's calls are - same
/// retries, same gates, same IDEM stamping - and the log it next sees
/// carries that call's own reply, so a stage can shop, read back what it
/// shopped, and act on it, one call at a time.
pub type Derive = Box<dyn FnMut(&[Value]) -> Option<ToolCall> + Send>;

/// One stage of a program whose calls are not known until the run is:
/// derived from the replies the earlier stages - and this stage's own
/// calls - drew. This is the seam a smart auto-edit stands on: the
/// tighten that cuts the dead air the voiceSignals stage measured, the
/// captions the transcribe handed back, the music bed shopped, imported,
/// read back and ducked - each call decided when the log that justifies
/// it is in hand, without the program having to guess any of it ahead of
/// the run.
///
/// A derive that answers `None` straight away is a stage that found
/// nothing to do, and completes: no dead air is a tighten that succeeded,
/// not one that failed. Gates on a derived step are checked over
/// whatever replies the derived calls drew; a gate that wants proving
/// belongs on a derive that always makes its proving call.
pub struct DerivedStep {
    /// The stage this step runs.
    pub stage: Stage,
    /// What this stage's failure means for the run.
    pub policy: ErrorPolicy,
    /// The next call, asked with the reply log until it answers `None`.
    pub next: Derive,
    /// What the stage's replies must say for the stage to count as done.
    pub gates: Vec<ReplyGate>,
}

impl DerivedStep {
    /// A stage whose failure fails the run, deriving its calls from the
    /// run's reply log.
    pub fn react(stage: Stage, next: Derive) -> DerivedStep {
        DerivedStep {
            stage,
            policy: ErrorPolicy::Fail,
            next,
            gates: Vec::new(),
        }
    }

    /// The same step, with the warn policy instead.
    pub fn warn(mut self) -> DerivedStep {
        self.policy = ErrorPolicy::Warn;
        self
    }

    /// The same step, asking one more thing of the stage's replies.
    pub fn gate(mut self, gate: ReplyGate) -> DerivedStep {
        self.gates.push(gate);
        self
    }
}

/// One stage of a program whose calls run concurrently: independent
/// groups, one lane each, all reading the same run. The transcriptions
/// of a five-take voiceover run at once instead of one after another;
/// the voice-signal reads of every take land in the time of the slowest.
///
/// A lane is its own caller - its own connection to the same server - so
/// the groups must be independent, and they must be read-only: an
/// `edit_apply` does not ride a fan, because the rollback order of
/// concurrent edits is not a promise the runner can keep. Placements
/// stay sequential; the parallel phase is the measuring (the skill's own
/// law, made the runner's).
#[derive(Clone, Debug, PartialEq)]
pub struct FanStep {
    /// The stage this step runs.
    pub stage: Stage,
    /// What this stage's failure means for the run.
    pub policy: ErrorPolicy,
    /// The independent groups, one lane each. With no lane factory the
    /// groups still run, one after another on the one caller - correct,
    /// only slower.
    pub groups: Vec<Vec<ToolCall>>,
    /// What the stage's replies (all lanes' replies, together) must say
    /// for the stage to count as done.
    pub gates: Vec<ReplyGate>,
}

impl FanStep {
    /// A stage whose failure fails the run, running `groups`
    /// concurrently.
    pub fn fan(stage: Stage, groups: Vec<Vec<ToolCall>>) -> FanStep {
        FanStep {
            stage,
            policy: ErrorPolicy::Fail,
            groups,
            gates: Vec::new(),
        }
    }

    /// The same step, with the warn policy instead.
    pub fn warn(mut self) -> FanStep {
        self.policy = ErrorPolicy::Warn;
        self
    }

    /// The same step, asking one more thing of the stage's replies.
    pub fn gate(mut self, gate: ReplyGate) -> FanStep {
        self.gates.push(gate);
        self
    }
}

/// One step of a program: static, derived from the run's own replies, or
/// fanned across lanes.
pub enum ProgramStep {
    /// A stage whose calls are known before the run starts.
    Static(StageStep),
    /// A stage whose calls the run's replies write.
    Derived(DerivedStep),
    /// A stage whose calls run concurrently, one lane per group.
    Fanned(FanStep),
}

impl ProgramStep {
    /// The stage this step runs, whatever kind of step it is.
    pub fn stage(&self) -> Stage {
        match self {
            ProgramStep::Static(step) => step.stage,
            ProgramStep::Derived(step) => step.stage,
            ProgramStep::Fanned(step) => step.stage,
        }
    }

    /// What this step's failure means for the run.
    pub fn policy(&self) -> ErrorPolicy {
        match self {
            ProgramStep::Static(step) => step.policy,
            ProgramStep::Derived(step) => step.policy,
            ProgramStep::Fanned(step) => step.policy,
        }
    }

    /// What the step's replies must say for the step to count as done.
    pub fn gates(&self) -> &[ReplyGate] {
        match self {
            ProgramStep::Static(step) => &step.gates,
            ProgramStep::Derived(step) => &step.gates,
            ProgramStep::Fanned(step) => &step.gates,
        }
    }

    /// The step as a static one, for the tests and the panes that read a
    /// program's shape: a derived or fanned step answers its stage and
    /// policy with no calls of its own to show.
    pub fn as_static(&self) -> StageStep {
        StageStep {
            stage: self.stage(),
            policy: self.policy(),
            calls: match self {
                ProgramStep::Static(step) => step.calls.clone(),
                _ => Vec::new(),
            },
            gates: self.gates().to_vec(),
        }
    }
}

impl From<StageStep> for ProgramStep {
    fn from(step: StageStep) -> ProgramStep {
        ProgramStep::Static(step)
    }
}

impl From<DerivedStep> for ProgramStep {
    fn from(step: DerivedStep) -> ProgramStep {
        ProgramStep::Derived(step)
    }
}

impl From<FanStep> for ProgramStep {
    fn from(step: FanStep) -> ProgramStep {
        ProgramStep::Fanned(step)
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
///
/// The steps may be static (calls known ahead), derived (calls written
/// by the run's own replies) or fanned (independent groups on concurrent
/// lanes); [`EditProgram::new`] takes the static kind, and
/// [`EditProgram::mixed`](EditProgram::mixed) takes any combination.
pub struct EditProgram {
    /// Where the plan says the export lands.
    pub export: PathBuf,
    /// The steps, in the order the program runs them.
    pub steps: Vec<ProgramStep>,
}

impl EditProgram {
    /// A program exporting to `export`, running `steps`.
    pub fn new(export: PathBuf, steps: Vec<StageStep>) -> EditProgram {
        EditProgram {
            export,
            steps: steps.into_iter().map(ProgramStep::Static).collect(),
        }
    }

    /// A program exporting to `export`, running `steps` of any kind:
    /// static, derived, fanned, in any order.
    pub fn mixed(export: PathBuf, steps: Vec<ProgramStep>) -> EditProgram {
        EditProgram { export, steps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_register_holds_seventeen_stages_and_the_first_and_last_say_so() {
        assert_eq!(Stage::ALL.len(), 17);
        assert_eq!(Stage::ALL.first(), Some(&Stage::HandoffVoiceover));
        // The revise is last: it answers the watch, which answers the
        // verify. The verify certifies the file the export wrote, so it
        // comes before both.
        assert_eq!(Stage::ALL.last(), Some(&Stage::Revise));
        assert_eq!(Stage::ALL[13], Stage::Export);
        assert_eq!(Stage::ALL[14], Stage::Verify);
        assert_eq!(Stage::ALL[15], Stage::Watch);
    }

    #[test]
    fn a_label_is_the_word_the_json_carries() {
        assert_eq!(Stage::HandoffVoiceover.label(), "handoff-voiceover");
        assert_eq!(Stage::AudioRepair.label(), "audio-repair");
        assert_eq!(Stage::VoiceSignals.label(), "voice-signals");
    }

    #[test]
    fn the_new_pair_labels_and_crosses_like_every_stage() {
        assert_eq!(Stage::Music.label(), "music");
        assert_eq!(Stage::Sfx.label(), "sfx");
        for stage in [Stage::Music, Stage::Sfx] {
            let word = serde_json::to_value(stage).expect("a stage is JSON");
            assert_eq!(word, serde_json::json!(stage.label()));
            let back: Stage = serde_json::from_value(word).expect("a stage reads back");
            assert_eq!(back, stage);
        }
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

    #[test]
    fn a_derived_step_answers_its_stage_and_polls_the_log() {
        let mut step = DerivedStep::react(
            Stage::Captions,
            Box::new(|log| {
                let segments: usize = log
                    .iter()
                    .filter_map(|reply| reply.get("segments").and_then(Value::as_array))
                    .map(Vec::len)
                    .sum();
                (segments > 0).then(|| ToolCall {
                    tool: "captions_add".to_owned(),
                    arguments: serde_json::json!({ "path": "/p" }),
                })
            }),
        )
        .warn();
        assert_eq!(step.stage, Stage::Captions);
        assert_eq!(step.policy, ErrorPolicy::Warn);
        // An empty log derives no calls; a log with segments derives one.
        assert!((step.next)(&Vec::new()).is_none());
        let log = vec![serde_json::json!({ "segments": [{ "text": "a" }, { "text": "b" }] })];
        assert!((step.next)(&log).is_some());
    }

    #[test]
    fn a_fan_step_groups_its_lanes_and_keeps_its_policy() {
        let one = ToolCall {
            tool: "transcribe".to_owned(),
            arguments: serde_json::json!({ "path": "/takes/one.wav" }),
        };
        let two = ToolCall {
            tool: "transcribe".to_owned(),
            arguments: serde_json::json!({ "path": "/takes/two.wav" }),
        };
        let step = FanStep::fan(Stage::Transcribe, vec![vec![one], vec![two]]).warn();
        assert_eq!(step.stage, Stage::Transcribe);
        assert_eq!(step.policy, ErrorPolicy::Warn);
        assert_eq!(step.groups.len(), 2);
        assert_eq!(step.groups[0][0].tool, "transcribe");
    }

    #[test]
    fn a_program_step_answers_stage_policy_and_gates_whatever_it_is() {
        let static_step: ProgramStep = StageStep::run(Stage::Export, vec![]).into();
        let derived_step: ProgramStep =
            DerivedStep::react(Stage::Captions, Box::new(|_| None)).into();
        let fanned_step: ProgramStep = FanStep::fan(Stage::Transcribe, vec![]).into();
        for (step, stage) in [
            (&static_step, Stage::Export),
            (&derived_step, Stage::Captions),
            (&fanned_step, Stage::Transcribe),
        ] {
            assert_eq!(step.stage(), stage);
            assert_eq!(step.policy(), ErrorPolicy::Fail);
            assert!(step.gates().is_empty());
            // A derived or fanned step shows no static calls of its own.
            let as_static = step.as_static();
            assert_eq!(as_static.stage, stage);
        }
        assert_eq!(static_step.as_static().calls.len(), 0);
    }

    #[test]
    fn a_mixed_program_keeps_its_steps_in_order() {
        let program = EditProgram::mixed(
            PathBuf::from("/e/export.mp4"),
            vec![
                StageStep::run(Stage::HandoffVoiceover, vec![]).into(),
                DerivedStep::react(Stage::Captions, Box::new(|_| None)).into(),
                FanStep::fan(Stage::Transcribe, vec![]).into(),
            ],
        );
        let stages: Vec<Stage> = program.steps.iter().map(|step| step.stage()).collect();
        assert_eq!(
            stages,
            vec![Stage::HandoffVoiceover, Stage::Captions, Stage::Transcribe]
        );
    }
}
