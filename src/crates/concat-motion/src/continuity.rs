// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The plan-level continuity gate: what the filled slots will look like
//! cut together, before the edit renders.
//!
//! A slot schema says what each opening wants; a filled manifest says
//! what each opening got. The gate reads the filled manifest as an
//! editor would scrub it - clip beside clip, in order - and flags the
//! places the cut will jump: a warm shot butting a cool one, a pan
//! reversing into the next, two wides in a row, a run of one category
//! with nothing new in it, a clip too still for the slot it was picked
//! for. Each flag names its slot, the rule that caught it, and what to
//! do about it: search the slot again with a hint saying what to avoid,
//! or stop shopping and let the treatment downgrade - b-roll to graphic,
//! b-roll to hold.
//!
//! This is arithmetic over the manifest's own numbers, deliberately:
//! colour bias, motion scores, directions, scales and categories are
//! facts a search already measured, and the gate's job is to notice
//! their adjacencies, not to re-measure the pixels. The frame-level
//! colour checker lives where the frames are; this gate runs after the
//! slots are filled, when nothing is rendered yet and everything is
//! still a plan.
//!
//! ```text
//! manifest [wide warm pan-right, wide cool pan-left, wide cool pan-left]
//!   -> scale:   slot 2 is a wide after a wide
//!   -> colour:  slot 2 is 0.9 cooler than slot 1
//!   -> motion:  slot 2 reverses slot 1's pan
//!   -> monotony: slots 2-3 run three of a category
//! ```

use serde::{Deserialize, Serialize};

/// One filled slot's clip, as the gate reads it: what the search
/// measured, and what the slot asked for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManifestClip {
    /// The slot this clip fills.
    pub slot_id: String,
    /// The clip the search picked.
    pub clip_id: String,
    /// The clip's colour bias, -1.0 cool through +1.0 warm, as the
    /// search's own measurement carries it.
    pub color_temperature: f64,
    /// How much moves, on the library's own motion scale.
    pub motion: f64,
    /// The way the clip's camera or subject travels.
    pub dominant_direction: TravelDirection,
    /// The shot's scale, as the plan named it.
    pub shot_scale: ShotScale,
    /// The clip's broad category, as the plan's vocabulary names it.
    pub category: String,
    /// The least motion the slot would take: the clip answers to this,
    /// whatever the library said.
    pub motion_min: f64,
}

impl Default for ManifestClip {
    fn default() -> Self {
        ManifestClip {
            slot_id: String::new(),
            clip_id: String::new(),
            color_temperature: 0.0,
            motion: 0.0,
            dominant_direction: TravelDirection::Static,
            shot_scale: ShotScale::Medium,
            category: String::new(),
            motion_min: 0.0,
        }
    }
}

impl ManifestClip {
    /// The colour-jump threshold, on the -1.0 cool to +1.0 warm scale:
    /// half the scale between neighbours is a jump the cut feels.
    pub const COLOUR_JUMP: f64 = 0.5;
}

/// The way a clip's camera or subject travels, as the plan's vocabulary
/// carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TravelDirection {
    /// Travelling left.
    Left,
    /// Travelling right.
    Right,
    /// Travelling up.
    Up,
    /// Travelling down.
    Down,
    /// Closing in.
    ZoomIn,
    /// Opening out.
    ZoomOut,
    /// The frame holds still.
    Static,
}

impl TravelDirection {
    /// Whether the two travels undo each other on the cut: a pan into
    /// its opposite, a zoom into its opposite. Static agrees with
    /// everything; two statics is the scale rule's business, not motion's.
    pub fn reverses(self, next: TravelDirection) -> bool {
        matches!(
            (self, next),
            (TravelDirection::Left, TravelDirection::Right)
                | (TravelDirection::Right, TravelDirection::Left)
                | (TravelDirection::Up, TravelDirection::Down)
                | (TravelDirection::Down, TravelDirection::Up)
                | (TravelDirection::ZoomIn, TravelDirection::ZoomOut)
                | (TravelDirection::ZoomOut, TravelDirection::ZoomIn)
        )
    }
}

/// A shot's scale, as the plan names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotScale {
    /// The establishing shot: the world, small on the frame.
    Wide,
    /// The subject and its room.
    Medium,
    /// The face and its detail.
    CloseUp,
}

/// The manifest a gate reads: the filled slots, in the order the edit
/// plays them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetManifest {
    /// The filled slots, in play order. Sorted where they are read;
    /// nothing here trusts the caller's order - though the plan's own
    /// order is what "adjacent" means, and a manifest out of order is a
    /// manifest the caller asked to be read in its given order, so the
    /// sort is stable and the caller's duplicates are their own business.
    pub clips: Vec<ManifestClip>,
}

/// The rule a discontinuity was caught by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityRule {
    /// A warm shot butts a cool one past the jump threshold.
    Colour,
    /// A clip's travel reverses its neighbour's.
    Motion,
    /// A clip repeats its neighbour's shot scale.
    Scale,
    /// Three or more consecutive clips of one category, nothing new.
    Monotony,
    /// A clip is too still for the slot that picked it.
    Static,
}

/// What to do about a discontinuity: search the slot again with a hint
/// saying what to avoid, or stop shopping and let the treatment
/// downgrade - b-roll to graphic, b-roll to hold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Remedy {
    /// Fill the slot again, avoiding what the hint names.
    Replace {
        /// What the new candidate must not repeat.
        avoid: String,
    },
    /// The slot stops shopping: a graphic carries the span instead.
    DowngradeToGraphic,
    /// The slot stops shopping: the A-roll holds the span.
    DowngradeToHold,
}

/// One place the cut will jump: the slot, the rule, what was seen, and
/// what to do about it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discontinuity {
    /// The slot the flag lands on: the second clip of a pair, the run's
    /// first slot, or the clip itself.
    pub slot_id: String,
    /// The rule that caught it.
    pub rule: ContinuityRule,
    /// What was seen, in the words a plan validator would say it.
    pub detail: String,
    /// What to do about it.
    pub remedy: Remedy,
}

/// The plan-level continuity check: read the filled manifest as an
/// editor scrubbing it, and return every place the cut will jump.
///
/// The rules, in the order an editor notices them: colour, motion,
/// scale, monotony, stillness. A manifest with no clips, or one clip,
/// has no adjacencies and nothing below a floor it does not break -
/// the empty list is the clean answer, not an error.
pub fn check_continuity(manifest: &AssetManifest) -> Vec<Discontinuity> {
    let mut flags = Vec::new();
    let clips = &manifest.clips;

    // Colour, motion and scale are adjacency reads, pair by pair; the
    // flag lands on the second clip of the pair, because that is the one
    // the editor would re-shoot or re-order.
    for pair in clips.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let delta = a.color_temperature - b.color_temperature;
        if delta >= ManifestClip::COLOUR_JUMP {
            flags.push(Discontinuity {
                slot_id: b.slot_id.clone(),
                rule: ContinuityRule::Colour,
                detail: format!(
                    "slot `{}` is {delta:.1} cooler than slot `{}`, a warm-to-cool jump past the {:.1} threshold",
                    b.slot_id, a.slot_id, ManifestClip::COLOUR_JUMP
                ),
                remedy: Remedy::Replace {
                    avoid: format!("a clip cooler than slot `{}`'s", a.slot_id),
                },
            });
        }
        if a.dominant_direction.reverses(b.dominant_direction) {
            flags.push(Discontinuity {
                slot_id: b.slot_id.clone(),
                rule: ContinuityRule::Motion,
                detail: format!(
                    "slot `{}`'s {:?} reverses slot `{}`'s {:?}",
                    b.slot_id, b.dominant_direction, a.slot_id, a.dominant_direction
                ),
                remedy: Remedy::Replace {
                    avoid: format!("a clip travelling opposite slot `{}`'s", a.slot_id),
                },
            });
        }
        if a.shot_scale == b.shot_scale {
            flags.push(Discontinuity {
                slot_id: b.slot_id.clone(),
                rule: ContinuityRule::Scale,
                detail: format!(
                    "slot `{}` is a {:?} after slot `{}`'s {:?}; two of one scale in a row is one scale too many",
                    b.slot_id,
                    b.shot_scale,
                    a.slot_id,
                    a.shot_scale
                ),
                remedy: Remedy::Replace {
                    avoid: format!("a clip at slot `{}`'s own scale", a.slot_id),
                },
            });
        }
    }

    // Monotony: a run of three or more clips of one category, with
    // nothing new among them. The flag lands on the run's first slot and
    // names the whole run; the remedy is the treatment's, not the
    // search's - three of a category is a rhythm problem, not one clip's.
    let mut run: Vec<&ManifestClip> = Vec::new();
    for clip in clips {
        let same = run
            .last()
            .map(|last| last.category == clip.category && !clip.category.is_empty())
            .unwrap_or(true);
        if same {
            run.push(clip);
        } else {
            flag_monotony(&mut flags, &run);
            run.clear();
            run.push(clip);
        }
    }
    flag_monotony(&mut flags, &run);
    // Stillness: any clip below its own slot's floor. The slot asked for
    // motion; the clip that answered with less is the wrong clip, and
    // the honest remedy may be the slot's - if stillness is wanted, the
    // slot can hold instead of shop.
    for clip in clips {
        if clip.motion < clip.motion_min {
            flags.push(Discontinuity {
                slot_id: clip.slot_id.clone(),
                rule: ContinuityRule::Static,
                detail: format!(
                    "slot `{}`'s clip carries motion {} under the {} the slot asked for",
                    clip.slot_id, clip.motion, clip.motion_min
                ),
                remedy: Remedy::DowngradeToHold,
            });
        }
    }
    flags
}

/// Flag a monotony run, if it is one: three or more clips of one
/// category. One flag a run, on the run's first slot, naming every slot
/// in it.
fn flag_monotony(flags: &mut Vec<Discontinuity>, run: &[&ManifestClip]) {
    if run.len() < 3 || run[0].category.is_empty() {
        return;
    }
    let slots: Vec<String> = run.iter().map(|clip| clip.slot_id.clone()).collect();
    flags.push(Discontinuity {
        slot_id: run[0].slot_id.clone(),
        rule: ContinuityRule::Monotony,
        detail: format!(
            "{} slots run one category (`{}`) with no variation: {}",
            run.len(),
            run[0].category,
            slots.join(", ")
        ),
        remedy: Remedy::DowngradeToGraphic,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A filled slot, bent one field at a time by the tests.
    fn filled(slot_id: &str) -> ManifestClip {
        ManifestClip {
            slot_id: slot_id.to_owned(),
            clip_id: format!("clip.{slot_id}"),
            color_temperature: 0.0,
            motion: 2.0,
            dominant_direction: TravelDirection::Static,
            shot_scale: ShotScale::Medium,
            category: "city".into(),
            motion_min: 0.0,
        }
    }

    /// A manifest with a wide-to-wide pair is flagged for its scale, and
    /// nothing else: the categories differ, the temperatures agree, the
    /// travels agree.
    #[test]
    fn a_wide_to_wide_pair_is_flagged() {
        let mut a = filled("a");
        a.shot_scale = ShotScale::Wide;
        let mut b = filled("b");
        b.shot_scale = ShotScale::Wide;
        b.category = "ocean".into();
        let flags = check_continuity(&AssetManifest { clips: vec![a, b] });
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert_eq!(flags[0].rule, ContinuityRule::Scale);
        assert_eq!(flags[0].slot_id, "b");
        assert!(flags[0].detail.contains("after slot `a`"), "{flags:?}");
    }

    /// A manifest with a warm-to-cool pair is flagged for its colour, at
    /// the threshold and past it - and a jump inside the threshold is
    /// not one.
    #[test]
    fn a_warm_to_cool_pair_is_flagged() {
        let mut warm = filled("warm");
        warm.color_temperature = 0.8;
        let mut cool = filled("cool");
        cool.color_temperature = -0.6;
        cool.shot_scale = ShotScale::CloseUp;
        cool.category = "ocean".into();
        let flags = check_continuity(&AssetManifest {
            clips: vec![warm.clone(), cool.clone()],
        });
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert_eq!(flags[0].rule, ContinuityRule::Colour);
        assert_eq!(flags[0].slot_id, "cool");
        assert!(flags[0].detail.contains("1.4 cooler"), "{flags:?}");

        // Half the scale is the threshold; a nudge is not a jump.
        let mut nudge = filled("nudge");
        nudge.color_temperature = 0.4;
        nudge.shot_scale = ShotScale::CloseUp;
        nudge.category = "ocean".into();
        let warm = ManifestClip {
            color_temperature: 0.8,
            ..filled("warm")
        };
        let flags = check_continuity(&AssetManifest {
            clips: vec![warm.clone(), nudge],
        });
        assert!(flags.is_empty(), "{flags:?}");

        // And a cool-to-warm move is the edit getting warmer, which an
        // editor reads as a lift, not a jump.
        let flags = check_continuity(&AssetManifest {
            clips: vec![cool, warm],
        });
        assert!(flags.is_empty(), "{flags:?}");
    }

    /// A clean manifest - varied scales, categories and temperatures,
    /// agreeing travels, motion above every floor - returns an empty
    /// list, and an empty manifest has nothing to jump.
    #[test]
    fn a_clean_manifest_returns_an_empty_list() {
        let mut a = filled("a");
        a.shot_scale = ShotScale::Wide;
        a.color_temperature = 0.3;
        a.dominant_direction = TravelDirection::Right;
        let mut b = filled("b");
        b.shot_scale = ShotScale::CloseUp;
        b.color_temperature = 0.1;
        b.category = "ocean".into();
        b.dominant_direction = TravelDirection::Right;
        b.motion_min = 1.0;
        let flags = check_continuity(&AssetManifest { clips: vec![a, b] });
        assert!(flags.is_empty(), "{flags:?}");
        assert!(check_continuity(&AssetManifest::default()).is_empty());
        let one = check_continuity(&AssetManifest {
            clips: vec![filled("only")],
        });
        assert!(one.is_empty(), "{one:?}");
    }

    /// A pan that reverses into the next clip is flagged for its motion,
    /// and a zoom into its opposite is the same jump. Static agrees with
    /// everything.
    #[test]
    fn a_reversing_travel_is_flagged() {
        let mut a = filled("a");
        a.dominant_direction = TravelDirection::Left;
        let mut b = filled("b");
        b.dominant_direction = TravelDirection::Right;
        b.shot_scale = ShotScale::CloseUp;
        b.category = "ocean".into();
        let flags = check_continuity(&AssetManifest { clips: vec![a, b] });
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert_eq!(flags[0].rule, ContinuityRule::Motion);

        let mut zoom_in = filled("zoom.in");
        zoom_in.dominant_direction = TravelDirection::ZoomIn;
        let mut zoom_out = filled("zoom.out");
        zoom_out.dominant_direction = TravelDirection::ZoomOut;
        zoom_out.shot_scale = ShotScale::CloseUp;
        zoom_out.category = "ocean".into();
        let flags = check_continuity(&AssetManifest {
            clips: vec![zoom_in, zoom_out],
        });
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert_eq!(flags[0].rule, ContinuityRule::Motion);
        assert!(
            !TravelDirection::Static.reverses(TravelDirection::Left)
                && !TravelDirection::Up.reverses(TravelDirection::Up),
            "static agrees, and a travel does not reverse itself"
        );
    }

    /// Three or more of one category with nothing new is a monotony run;
    /// two are a pair the edit is allowed to have.
    #[test]
    fn a_run_of_one_category_is_flagged_when_it_reaches_three() {
        // The scales travel so the scale rule stays quiet: the run is
        // about the category, not the framing.
        let mut a = filled("a");
        a.shot_scale = ShotScale::Wide;
        let mut b = filled("b");
        b.shot_scale = ShotScale::CloseUp;
        let flags = check_continuity(&AssetManifest {
            clips: vec![a.clone(), b.clone()],
        });
        assert!(
            flags.is_empty(),
            "two of a category is not a run: {flags:?}"
        );

        let mut c = filled("c");
        c.shot_scale = ShotScale::Medium;
        let flags = check_continuity(&AssetManifest {
            clips: vec![a, b, c],
        });
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert_eq!(flags[0].rule, ContinuityRule::Monotony);
        assert_eq!(flags[0].slot_id, "a", "the flag lands on the run's head");
        assert!(
            flags[0].detail.contains("a, b, c"),
            "the run names its slots: {flags:?}"
        );
        // The remedy is the treatment's: three of one category is a
        // rhythm problem, so the run downgrades to graphics.
        assert_eq!(flags[0].remedy, Remedy::DowngradeToGraphic);

        // A fourth clip of another category breaks the run, at its own
        // scale.
        let mut d = filled("d");
        d.category = "ocean".into();
        d.shot_scale = ShotScale::Wide;
        let mut a = filled("a");
        a.shot_scale = ShotScale::Wide;
        let mut b = filled("b");
        b.shot_scale = ShotScale::CloseUp;
        let mut c = filled("c");
        c.shot_scale = ShotScale::Medium;
        let flags = check_continuity(&AssetManifest {
            clips: vec![a, b, c, d],
        });
        assert_eq!(flags.len(), 1, "{flags:?}");
    }

    /// A clip too still for the slot that picked it is flagged for its
    /// stillness, with the slot's own remedy: hold instead of shop.
    #[test]
    fn a_clip_under_its_own_floor_is_flagged_as_static() {
        let mut still = filled("still");
        still.motion = 0.2;
        still.motion_min = 1.0;
        let flags = check_continuity(&AssetManifest { clips: vec![still] });
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert_eq!(flags[0].rule, ContinuityRule::Static);
        assert_eq!(flags[0].slot_id, "still");
        assert_eq!(flags[0].remedy, Remedy::DowngradeToHold);
        assert!(flags[0].detail.contains("0.2 under the 1"), "{flags:?}");
    }

    /// The gate reads the manifest in its given order, and the wire shape
    /// is the plan's own camelCase.
    #[test]
    fn the_manifest_round_trips_through_the_json() {
        let manifest = AssetManifest {
            clips: vec![ManifestClip {
                slot_id: "hook.1".into(),
                dominant_direction: TravelDirection::ZoomIn,
                shot_scale: ShotScale::Wide,
                ..filled("hook.1")
            }],
        };
        let json = serde_json::to_value(&manifest).expect("serializes");
        assert_eq!(json["clips"][0]["slotId"], "hook.1");
        assert_eq!(json["clips"][0]["dominantDirection"], "zoom_in");
        assert_eq!(json["clips"][0]["shotScale"], "wide");
        let back: AssetManifest = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, manifest);
    }
}
