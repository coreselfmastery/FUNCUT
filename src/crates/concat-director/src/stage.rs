// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The stages an auto-edit runs through, and the program that names
//! them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
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

/// One stage of a program: the calls it makes, in order, and what a
/// failure means.
#[derive(Clone, PartialEq, Debug)]
pub struct StageStep {
    /// The stage this step runs.
    pub stage: Stage,
    /// What this stage's failure means for the run.
    pub policy: ErrorPolicy,
    /// The calls the stage makes, in the order it makes them.
    pub calls: Vec<ToolCall>,
}

impl StageStep {
    /// A stage whose failure fails the run, making `calls`.
    pub fn run(stage: Stage, calls: Vec<ToolCall>) -> StageStep {
        StageStep {
            stage,
            policy: ErrorPolicy::Fail,
            calls,
        }
    }

    /// The same step, with the warn policy instead: its failure is
    /// recorded and the run goes on.
    pub fn warn(mut self) -> StageStep {
        self.policy = ErrorPolicy::Warn;
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
}
