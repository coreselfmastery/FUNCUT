// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Where a beat lands: the card-or-scene router.
//!
//! An agent choosing between a motion *card* (one comp, one idea, seconds
//! long) and a *scene* (a stack of layers, each on its own clock) chose by
//! prompt, which nothing could test. This module is the deterministic
//! answer, and the agent's default: [`route`] reads a beat's length and
//! its kind, and says where it lands. A director may override the answer
//! with a reason - [`Decision::overridden`] - and the reason is carried,
//! because a decision nobody can see the why of is a guess with
//! confidence.
//!
//! The rules, in the order they fire:
//!
//! 1. **Under five seconds is a card.** Nothing shorter than five seconds
//!    is a scene: there is no time for a second layer to arrive, which is
//!    the least a scene is.
//! 2. **A hook or a call to action stays a card until eight seconds.**
//!    Both are single-glance beats - the hook is read before it is
//!    watched, and a CTA is one card's worth of words - and five to eight
//!    seconds of either is a held card, not a developed idea.
//! 3. **Five to fifteen seconds is a scene.** That is the span in which a
//!    stack of layers can take the stage one after another and leave
//!    before the cut: a developed idea, at parallax depth.
//! 4. **Over fifteen seconds is a nested composition** - an act, scenes
//!    within - except for the hook and the call to action, which stay one
//!    scene however long the slot is: an act is made of several beats,
//!    and those two are each one beat.
//!
//! A duration that is not a finite number routes as a card, the shortest
//! answer, rather than as whichever branch a NaN comparison fell through.

use serde::{Deserialize, Serialize};

/// The kind of beat a moment of the edit is - the same seven acts the
/// emotion read names, because that read is what drives the edit's plan.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BeatType {
    /// The opening grab: read at a glance, over in a breath.
    Hook,
    /// The ground the video stands on.
    Setup,
    /// The problem, wound up.
    Tension,
    /// The shift the video exists for.
    Turn,
    /// The answer, shown.
    Reveal,
    /// The receipts.
    Proof,
    /// The ask.
    Cta,
}

impl BeatType {
    /// Every beat, in the order a script meets them.
    pub const ALL: [BeatType; 7] = [
        BeatType::Hook,
        BeatType::Setup,
        BeatType::Tension,
        BeatType::Turn,
        BeatType::Reveal,
        BeatType::Proof,
        BeatType::Cta,
    ];

    /// The name the emotion read and the plan carry.
    pub fn name(self) -> &'static str {
        match self {
            BeatType::Hook => "hook",
            BeatType::Setup => "setup",
            BeatType::Tension => "tension",
            BeatType::Turn => "turn",
            BeatType::Reveal => "reveal",
            BeatType::Proof => "proof",
            BeatType::Cta => "cta",
        }
    }

    /// Whether this beat is a single-glance one - a hook is read before
    /// it is watched, a call to action is one card's worth of words -
    /// which keeps it a card longer and never nests it.
    fn is_single_glance(self) -> bool {
        matches!(self, BeatType::Hook | BeatType::Cta)
    }
}

/// Where a beat lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Routing {
    /// One comp, one idea: a card.
    Card,
    /// A stack of layers, each on its own clock: a scene.
    Scene,
    /// Scenes within a scene: a nested composition, for an act.
    Nested,
}

impl Routing {
    /// The name a plan or a prompt carries.
    pub fn name(self) -> &'static str {
        match self {
            Routing::Card => "card",
            Routing::Scene => "scene",
            Routing::Nested => "nested",
        }
    }
}

/// The boundary rules carry their numbers here, so the tests and the
/// doc comment cannot disagree with the code.
const CARD_UNTIL: f64 = 5.0;
/// A hook or a CTA holds its card this long before it earns a scene.
const SINGLE_GLANCE_CARD_UNTIL: f64 = 8.0;
/// The longest a developed idea is one scene.
const SCENE_UNTIL: f64 = 15.0;

/// Where a beat of `duration_secs` seconds and this kind lands. The
/// default an agent takes; see the module docs for the rules in the
/// order they fire.
pub fn route(duration_secs: f64, beat_type: BeatType) -> Routing {
    // Not a number, or not a positive one: the shortest answer. A
    // comparison against NaN is false in every direction, so without
    // this the routing would depend on which branch asked.
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Routing::Card;
    }
    if duration_secs < CARD_UNTIL {
        return Routing::Card;
    }
    if beat_type.is_single_glance() && duration_secs < SINGLE_GLANCE_CARD_UNTIL {
        return Routing::Card;
    }
    if duration_secs <= SCENE_UNTIL {
        return Routing::Scene;
    }
    if beat_type.is_single_glance() {
        return Routing::Scene;
    }
    Routing::Nested
}

/// A routing answer: where the beat lands, and why. The default's why is
/// the rule that fired; an override's why is the director's reason,
/// carried whole, because a decision nobody can see the why of is a guess
/// with confidence.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Decision {
    /// Where the beat lands.
    pub route: Routing,
    /// Why: the rule that fired, or the director's reason.
    pub reason: String,
}

impl Decision {
    /// The default answer for a beat, with the rule that fired as its
    /// reason.
    pub fn taken_by_default(duration_secs: f64, beat_type: BeatType) -> Decision {
        let route = route(duration_secs, beat_type);
        let reason = if !duration_secs.is_finite() || duration_secs <= 0.0 {
            format!(
                "a beat of {duration_secs} s is not a span; the shortest answer is taken"
            )
        } else if duration_secs < CARD_UNTIL {
            format!(
                "a {} of {duration_secs} s is under {CARD_UNTIL} s, and nothing that short is a scene",
                beat_type.name()
            )
        } else if beat_type.is_single_glance() && duration_secs < SINGLE_GLANCE_CARD_UNTIL {
            format!(
                "a {} is read at a glance: it stays a card until {SINGLE_GLANCE_CARD_UNTIL} s, and this one is {duration_secs} s",
                beat_type.name()
            )
        } else if duration_secs <= SCENE_UNTIL {
            format!(
                "a {} of {duration_secs} s is a developed idea in the {CARD_UNTIL}-{SCENE_UNTIL} s span: a scene",
                beat_type.name()
            )
        } else if beat_type.is_single_glance() {
            format!(
                "a {} is one beat, however long the slot: {duration_secs} s stays one scene rather than an act of several",
                beat_type.name()
            )
        } else {
            format!(
                "a {} of {duration_secs} s is over {SCENE_UNTIL} s: an act, scenes within",
                beat_type.name()
            )
        };
        Decision { route, reason }
    }

    /// The director's override: another routing, carried with the reason
    /// it was taken. The default is not second-guessed here - a reason
    /// that says nothing ("because") is the director's to answer for -
    /// but the reason cannot be empty, because an override nobody can see
    /// the why of is exactly what this type exists to prevent.
    pub fn overridden(route: Routing, reason: impl Into<String>) -> Decision {
        let reason = reason.into();
        Decision {
            route,
            reason: format!("overridden by the director: {reason}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every branch of the matrix: each beat at each boundary and one
    /// step either side of it. A routing change that moves one cell is a
    /// policy change, and this is where it is seen.
    #[test]
    fn the_matrix_covers_every_beat_at_every_boundary() {
        // (duration, beat) -> expected routing, as the rules read.
        let cases: Vec<(f64, BeatType, Routing)> = vec![
            // Under five seconds: a card, every beat.
            (0.5, BeatType::Hook, Routing::Card),
            (4.99, BeatType::Setup, Routing::Card),
            (4.99, BeatType::Tension, Routing::Card),
            (4.99, BeatType::Turn, Routing::Card),
            (4.99, BeatType::Reveal, Routing::Card),
            (4.99, BeatType::Proof, Routing::Card),
            (4.99, BeatType::Cta, Routing::Card),
            // Five to eight: the single-glance beats hold their card.
            (5.0, BeatType::Hook, Routing::Card),
            (5.0, BeatType::Cta, Routing::Card),
            (7.99, BeatType::Hook, Routing::Card),
            // ... and the developed ones are scenes.
            (5.0, BeatType::Setup, Routing::Scene),
            (5.0, BeatType::Tension, Routing::Scene),
            (5.0, BeatType::Turn, Routing::Scene),
            (5.0, BeatType::Reveal, Routing::Scene),
            (5.0, BeatType::Proof, Routing::Scene),
            (7.5, BeatType::Tension, Routing::Scene),
            // Eight to fifteen: every beat a scene.
            (8.0, BeatType::Hook, Routing::Scene),
            (8.0, BeatType::Cta, Routing::Scene),
            (12.0, BeatType::Setup, Routing::Scene),
            (15.0, BeatType::Proof, Routing::Scene),
            (15.0, BeatType::Hook, Routing::Scene),
            // Over fifteen: an act, except the single-glance beats.
            (15.01, BeatType::Setup, Routing::Nested),
            (15.01, BeatType::Tension, Routing::Nested),
            (15.01, BeatType::Turn, Routing::Nested),
            (15.01, BeatType::Reveal, Routing::Nested),
            (15.01, BeatType::Proof, Routing::Nested),
            (15.01, BeatType::Hook, Routing::Scene),
            (15.01, BeatType::Cta, Routing::Scene),
            (30.0, BeatType::Turn, Routing::Nested),
            (30.0, BeatType::Cta, Routing::Scene),
        ];
        for (duration, beat, expected) in cases {
            assert_eq!(
                route(duration, beat),
                expected,
                "{beat:?} at {duration} s"
            );
        }
    }

    #[test]
    fn a_span_that_is_not_one_routes_as_the_shortest_answer() {
        assert_eq!(route(f64::NAN, BeatType::Reveal), Routing::Card);
        assert_eq!(route(f64::INFINITY, BeatType::Reveal), Routing::Card);
        assert_eq!(route(0.0, BeatType::Reveal), Routing::Card);
        assert_eq!(route(-3.0, BeatType::Reveal), Routing::Card);
    }

    #[test]
    fn the_default_carries_the_rule_that_fired() {
        let short = Decision::taken_by_default(3.0, BeatType::Hook);
        assert_eq!(short.route, Routing::Card);
        assert!(short.reason.contains("under 5 s"), "{}", short.reason);

        let glance = Decision::taken_by_default(6.0, BeatType::Cta);
        assert_eq!(glance.route, Routing::Card);
        assert!(glance.reason.contains("at a glance"), "{}", glance.reason);

        let scene = Decision::taken_by_default(10.0, BeatType::Reveal);
        assert_eq!(scene.route, Routing::Scene);
        assert!(scene.reason.contains("developed idea"), "{}", scene.reason);

        let act = Decision::taken_by_default(40.0, BeatType::Setup);
        assert_eq!(act.route, Routing::Nested);
        assert!(act.reason.contains("an act"), "{}", act.reason);

        let long_hook = Decision::taken_by_default(40.0, BeatType::Hook);
        assert_eq!(long_hook.route, Routing::Scene);
        assert!(
            long_hook.reason.contains("one beat"),
            "{}",
            long_hook.reason
        );
    }

    #[test]
    fn an_override_is_carried_with_its_reason() {
        // The director wants a 3-second hook AS a scene, for one shot:
        // the default says card, the override says scene, and the reason
        // rides along.
        let default = Decision::taken_by_default(3.0, BeatType::Hook);
        assert_eq!(default.route, Routing::Card);
        let override_ = Decision::overridden(
            Routing::Scene,
            "the whole video is one held scene; the hook is its first layer, not its own card",
        );
        assert_eq!(override_.route, Routing::Scene);
        assert!(override_.reason.contains("director"), "{}", override_.reason);
        assert!(
            override_.reason.contains("first layer"),
            "{}",
            override_.reason
        );
    }

    #[test]
    fn the_names_are_the_vocabulary_the_plan_carries() {
        assert_eq!(BeatType::Hook.name(), "hook");
        assert_eq!(BeatType::Cta.name(), "cta");
        assert_eq!(Routing::Card.name(), "card");
        assert_eq!(Routing::Scene.name(), "scene");
        assert_eq!(Routing::Nested.name(), "nested");
        // Every beat has a name, and no two share one.
        let names: Vec<&str> = BeatType::ALL.iter().map(|b| b.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len());
    }
}
