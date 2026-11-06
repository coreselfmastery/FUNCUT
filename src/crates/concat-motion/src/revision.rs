// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The revision gate: when the critic's score says the edit is not done,
//! and what exactly gets another pass.
//!
//! The director's loop ends with a render and a score. Below the ship
//! line, the tempting move is to re-plan everything - and it is the
//! wrong move twice over: it re-shops slots the critic did not name,
//! which can only lose what the edit already has, and it throws away
//! the one thing the score carries, which is the knowledge of where the
//! edit fell short. The gate makes the right move the only move: below
//! the ship line, exactly the slots the critic named go back to the
//! planning step; everything else stands. After the retries are spent
//! the edit ships anyway, with the failures recorded per slot - shipping
//! with a note beats not shipping.
//!
//! Every attempt renders to its own file: `edit_v1.mp4`, then
//! `edit_v2.mp4`, then `edit_v3.mp4` - an attempt nobody can diff
//! against the one before it is an attempt nobody learns from.
//!
//! ```text
//! score 0.5, notes [hook.1, reveal.hero], retries 0
//!   -> Revise { failing: [hook.1, reveal.hero], attempt: 1 }
//!      -> the planning step re-shops hook.1 and reveal.hero, only
//!      -> re-render to edit_v2.mp4, score again
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The score an edit ships at, and above: the critic's overall, on the
/// 0-1 scale its craft axes carry.
pub const SHIP_SCORE: f64 = 0.65;

/// How many times the planning step may take the critic's notes before
/// the edit ships with them recorded: two revisions, three renders in
/// all, `edit_v1` through `edit_v3`.
pub const MAX_RETRIES: u32 = 2;

/// One slot the critic named, as the planning step reads it back: the
/// slot, and what the critic said about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRevision {
    /// The slot the notes are about.
    pub slot_id: String,
    /// What the critic said, in the words the planning step shops with.
    pub notes: Vec<String>,
}

/// What the gate decides after a scored attempt.
#[derive(Clone, Debug, PartialEq)]
pub enum GateDecision {
    /// The edit ships as it stands.
    Ship,
    /// The named slots go back to the planning step; the rest of the
    /// edit stands untouched. `attempt` is which revision this is - the
    /// first revision renders to `edit_v2.mp4`.
    Revise {
        /// The slots the critic named, only these.
        failing: Vec<SlotRevision>,
        /// Which revision round this is, one-based.
        attempt: u32,
    },
    /// The retries are spent. The edit ships, and the failures travel
    /// with it recorded per slot: the next pass over this plan starts
    /// from what went wrong, not from nothing.
    Exhausted {
        /// The slots that were still failing at the last attempt.
        failing: Vec<SlotRevision>,
        /// How many revisions were spent - [`MAX_RETRIES`].
        attempts: u32,
    },
}

/// What the gate decides, given how many revisions have already run,
/// what the critic scored, and which slots it named.
///
/// At or above the ship line the edit ships, whatever the retries say -
/// a score is not improved by another pass it did not ask for. Below
/// it, the named slots go back to the planning step while the retries
/// last, and after that the edit ships exhausted, failures recorded.
/// The gate revises exactly what the critic named: a note-less pass
/// below the line is a revision round with nothing named, and the
/// caller sees that plainly rather than the gate inventing work.
pub fn decide(retries: u32, overall_score: f64, notes: &[SlotRevision]) -> GateDecision {
    if overall_score >= SHIP_SCORE {
        return GateDecision::Ship;
    }
    if retries < MAX_RETRIES {
        GateDecision::Revise {
            failing: notes.to_vec(),
            attempt: retries + 1,
        }
    } else {
        GateDecision::Exhausted {
            failing: notes.to_vec(),
            attempts: retries,
        }
    }
}

/// The file an attempt renders to: `edit_v1.mp4` inside `dir`, `edit_v2`
/// for the first revision, `edit_v3` for the second. An attempt nobody
/// can diff against the one before it is an attempt nobody learns from.
pub fn attempt_path(dir: &Path, attempt: u32) -> PathBuf {
    dir.join(format!("edit_v{attempt}.mp4"))
}

/// The slots a revision searches again: only the ones the critic named.
/// A slot the critic did not name has passed, and its candidates stand -
/// the revision never re-shops a passing slot, because a re-roll can
/// only lose what the edit already has.
pub fn failing_slot_ids(decision: &GateDecision) -> Vec<String> {
    match decision {
        GateDecision::Ship => Vec::new(),
        GateDecision::Revise { failing, .. } | GateDecision::Exhausted { failing, .. } => {
            failing.iter().map(|slot| slot.slot_id.clone()).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A critic's note about one slot.
    fn note(slot_id: &str, what: &str) -> SlotRevision {
        SlotRevision {
            slot_id: slot_id.to_owned(),
            notes: vec![what.to_owned()],
        }
    }

    /// The done-when: a score under the line triggers one revision, of
    /// exactly the slots the critic named - never the whole edit.
    #[test]
    fn a_score_of_a_half_triggers_one_revision_of_the_named_slots() {
        let notes = vec![
            note("hook.1", "the hook enters late"),
            note("reveal.hero", "the reveal undercuts"),
        ];
        let decision = decide(0, 0.5, &notes);
        assert_eq!(
            decision,
            GateDecision::Revise {
                failing: notes.clone(),
                attempt: 1,
            },
            "{decision:?}"
        );
        // Only the failing slots get new candidates, and the file the
        // revision renders to is the second one.
        assert_eq!(failing_slot_ids(&decision), vec!["hook.1", "reveal.hero"]);
        assert_eq!(
            attempt_path(Path::new("out"), 2),
            Path::new("out/edit_v2.mp4")
        );
    }

    /// A score at or above the line ships on the first attempt, retries
    /// or no retries, notes or no notes.
    #[test]
    fn a_score_of_point_seven_ships_on_the_first_attempt() {
        assert_eq!(decide(0, 0.7, &[]), GateDecision::Ship);
        assert_eq!(
            decide(0, SHIP_SCORE, &[note("hook.1", "whatever")]),
            GateDecision::Ship
        );
        assert_eq!(decide(2, 0.9, &[]), GateDecision::Ship);
    }

    /// After two revisions the gate gives up on improving and ships with
    /// the failures recorded per slot - the next pass starts from what
    /// went wrong, not from nothing.
    #[test]
    fn after_two_retries_the_plan_ships_with_the_failures_recorded() {
        let notes = vec![
            note("hook.1", "the hook enters late"),
            note("turn.2", "the turn lands soft"),
            note("reveal.hero", "the reveal undercuts"),
        ];
        // The first and second revisions run; the third score below the
        // line finds the retries spent.
        assert_eq!(
            decide(1, 0.5, &notes),
            GateDecision::Revise {
                failing: notes.clone(),
                attempt: 2,
            }
        );
        let decision = decide(2, 0.5, &notes);
        assert_eq!(
            decision,
            GateDecision::Exhausted {
                attempts: 2,
                failing: notes.clone(),
            },
            "{decision:?}"
        );
        // Every failing slot travels with the ship, notes attached.
        let failing = failing_slot_ids(&decision);
        assert_eq!(failing, vec!["hook.1", "turn.2", "reveal.hero"]);
        // And the render the exhausted plan ships is the third.
        assert_eq!(
            attempt_path(Path::new("out"), 3),
            Path::new("out/edit_v3.mp4")
        );
    }

    /// A note-less pass below the line is a revision round with nothing
    /// named: the gate says so plainly rather than inventing work.
    #[test]
    fn a_revision_without_notes_names_nothing() {
        let decision = decide(0, 0.5, &[]);
        assert_eq!(
            decision,
            GateDecision::Revise {
                failing: Vec::new(),
                attempt: 1,
            },
            "{decision:?}"
        );
        assert!(failing_slot_ids(&decision).is_empty());
    }

    /// The wire shape is the plan's own camelCase, so a critic's notes
    /// written in JSON and a gate decision written back read the same
    /// words.
    #[test]
    fn the_notes_round_trip_through_the_json() {
        let notes = vec![note("reveal.hero", "the reveal undercuts")];
        let json = serde_json::to_value(&notes).expect("serializes");
        assert_eq!(json[0]["slotId"], "reveal.hero");
        assert_eq!(json[0]["notes"][0], "the reveal undercuts");
        let back: Vec<SlotRevision> = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, notes);
    }
}
