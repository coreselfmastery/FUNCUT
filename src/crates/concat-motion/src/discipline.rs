// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The motion principles, as a lint instead of a mood.
//!
//! The easing library gives a comp every curve it could want; what it does
//! not give is an opinion, and motion without an opinion is motion that
//! looks generated. Four principles, held here so nobody has to hold them
//! by hand:
//!
//! 1. **Entrances arrive; they do not creep.** A value rising out of
//!    nothing should spend its energy early and settle - `ease-out`,
//!    `emphasis`, a spring. An entrance on `ease-in` or `linear` is the
//!    single most robotic read a comp can make, and the one this lint
//!    exists to catch. `steps()` is exempt: a mechanical reveal is a
//!    *choice*, and the typewriter is its poster child.
//! 2. **Exits leave; they do not settle.** A value falling to nothing
//!    should accelerate away (`ease-in`) or step out - an exit that
//!    eases *out* hangs in the doorway, and an exit that travels
//!    linearly reads as a switch turning off.
//! 3. **Overshoot is punctuation, not prose.** A spring that bounces or a
//!    `linear()` that overshoots is a playful moment's curve; on a calm
//!    scene it is a comp that cannot hold still. The lint holds this one
//!    against the scene's personality: a comp wearing the `punchy` style
//!    is *allowed* its bounce, which is what the style is for - the
//!    reserve is against wearing bounce everywhere, not against wearing it
//!    on purpose.
//! 4. **A drift is not an entrance.** A plate panning slowly across a
//!    scene on `linear` is honest motion - the whole point of linear is
//!    steady travel. The lint polices curves only in the windows that are
//!    entrances and exits (a value rising out of, or falling to, nothing),
//!    never the steady stretches between.
//!
//! The audit is **advice, not refusal**: its findings ride the plan's
//! warnings, where the critic can read them and an agent can fix them,
//! because a comp that breaks a principle on purpose is a comp that
//! should render - and be told.

use crate::comp::{Comp, Node};
use crate::curve::Curve;

/// The family a curve's motion belongs to - the shape the principle cares
/// about, not the syntax it was written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    /// Fast out of the gate, a long settle: arrivals.
    Arrive,
    /// Slow to start, accelerating away: departures.
    Leave,
    /// Symmetric: either side's honest companion.
    Both,
    /// Steady travel: drifts, counters, plates.
    Linear,
    /// Held then jumped: mechanical, and exempt.
    Steps,
    /// Bounces past its landing: punctuation, for playful moments.
    Overshoot,
}

/// A spring's damping ratio: below one, it overshoots. The line between
/// "a spring" and "a bounce" is drawn where physics draws it.
fn damping_ratio(stiffness: f64, damping: f64, mass: f64) -> f64 {
    let omega = (stiffness / mass.max(1e-9)).sqrt();
    damping / (2.0 * mass.max(1e-9) * omega)
}

/// Which family a curve travels in, by *measuring* it rather than by
/// reading its syntax: the value at a third of the way in and at a third
/// from the end say together where a curve spends its energy, which is
/// the whole question. `apply` is the crate's own solver, so the lint and
/// the renderer can never disagree about what `ease-out` means.
fn family(curve: &Curve) -> Family {
    match curve {
        Curve::Linear => Family::Linear,
        Curve::Steps { .. } => Family::Steps,
        Curve::Spring(spring) => {
            // 0.85, not 1.0: a damping ratio just under one overshoots by
            // a fraction of a percent, which no eye can see, while the
            // library's own settle spring sits at 0.997 - the line between
            // "a spring" and "a bounce" is drawn where the bounce becomes
            // visible, not where physics stops calling it overdamped.
            if damping_ratio(spring.stiffness, spring.damping, spring.mass) < 0.85 {
                Family::Overshoot
            } else {
                Family::Arrive
            }
        }
        Curve::Bezier { y1, y2, .. } => {
            if *y1 > 1.01 || *y2 > 1.01 || *y1 < -0.01 || *y2 < -0.01 {
                return Family::Overshoot;
            }
            shape_of(curve)
        }
        Curve::Points { stops } => {
            if stops
                .iter()
                .any(|(_, output)| *output > 1.01 || *output < -0.01)
            {
                return Family::Overshoot;
            }
            shape_of(curve)
        }
    }
}

/// Where a non-overshooting curve spends its energy: early, late, or
/// evenly. A curve that is a third of the way in at 55 percent of its
/// value is arriving; one holding 18 percent at the same moment is
/// leaving; anything between is symmetric.
fn shape_of(curve: &Curve) -> Family {
    let early = curve.apply(0.3);
    let late = curve.apply(0.7);
    if early + late > 1.2 {
        Family::Arrive
    } else if early + late < 0.8 {
        Family::Leave
    } else {
        Family::Both
    }
}

/// Whether a track's value is "hidden" - the 5 percent band where a rise
/// out of it is an entrance and a fall into it is an exit. Opacity is the
/// obvious one; the band keeps the lint from reading a dip as a door.
const HIDDEN: f64 = 0.05;

/// The entrance of a track, if it has one: the first segment whose value
/// rises out of the hidden band. Returns `(at, curve)` of the segment.
fn entrance(track: &crate::track::Track) -> Option<(f64, Option<&Curve>)> {
    let mut keys: Vec<&crate::track::Key> = track.keys.iter().collect();
    keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    for pair in keys.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let (Some(a), Some(b)) = (from.value.as_number(), to.value.as_number()) else {
            continue;
        };
        if a <= HIDDEN && b > a + HIDDEN {
            return Some((from.at, from.curve.as_ref()));
        }
    }
    None
}

/// The exit of a track, if it has one: the last segment whose value falls
/// into the hidden band. Exits are read from the end because an exit is a
/// promise about how the story ends.
fn exit(track: &crate::track::Track) -> Option<(f64, Option<&Curve>)> {
    let mut keys: Vec<&crate::track::Key> = track.keys.iter().collect();
    keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    for pair in keys.windows(2).rev() {
        let (from, to) = (pair[0], pair[1]);
        let (Some(a), Some(b)) = (from.value.as_number(), to.value.as_number()) else {
            continue;
        };
        if b <= HIDDEN && a > b + HIDDEN {
            return Some((from.at, from.curve.as_ref()));
        }
    }
    None
}

/// The easing library's own words, for the note's suggestion.
fn suggested_for(family: Family) -> &'static str {
    match family {
        Family::Arrive | Family::Overshoot => "ease-in",
        Family::Leave | Family::Both | Family::Steps | Family::Linear => {
            "ease-out, emphasis or a spring"
        }
    }
}

/// Audit a comp against the motion principles, as sentences a plan's
/// warnings or a critic's eye can carry. `playful` is the scene's own
/// leave to bounce - the `punchy` style, or a comp the author says is a
/// joke - and it lifts exactly one finding: overshoot on an arrival.
/// An overshooting exit is flagged whatever the mood, because a bounce
/// on the way out of frame is never what a bounce is for.
pub fn audit(comp: &Comp, playful: bool) -> Vec<String> {
    let mut notes = Vec::new();
    for node in &comp.nodes {
        audit_node(node, playful, &mut notes);
    }
    notes
}

/// One node's audit: its entrances, its exits, its curves, and a
/// persistent pointer (`/nodes/3/tracks/opacity`) that says where each
/// finding lives in the document an agent would edit.
fn audit_node(node: &Node, playful: bool, notes: &mut Vec<String>) {
    for (name, track) in &node.tracks {
        // Entrances: rise out of the hidden band.
        if let Some((at, curve)) = entrance(track) {
            let curve = curve.cloned().unwrap_or(Curve::Linear);
            match family(&curve) {
                Family::Arrive | Family::Both | Family::Steps => {}
                Family::Overshoot if playful => {}
                Family::Overshoot => notes.push(format!(
                    "node {:?} enters {name} at {:.0}% with a bouncing curve \
                     ({}); overshoot is punctuation for playful moments, and this \
                     scene is not wearing one - ease-out, emphasis or an \
                     over-damped spring lands the same read",
                    node.id,
                    at * 100.0,
                    curve_name(&curve)
                )),
                Family::Leave => notes.push(format!(
                    "node {:?} enters {name} at {:.0}% easing in; an arrival \
                     should spend its energy early and settle - {} (an entrance \
                     that creeps in is the most robotic read a comp can make)",
                    node.id,
                    at * 100.0,
                    suggested_for(Family::Leave)
                )),
                Family::Linear => notes.push(format!(
                    "node {:?} enters {name} at {:.0}% on a straight line; \
                     linear is what makes motion look cheap - {}, or steps() \
                     when the reveal means to be mechanical",
                    node.id,
                    at * 100.0,
                    suggested_for(Family::Linear)
                )),
            }
        }
        // Exits: fall into the hidden band.
        if let Some((at, curve)) = exit(track) {
            let curve = curve.cloned().unwrap_or(Curve::Linear);
            match family(&curve) {
                Family::Leave | Family::Both | Family::Steps => {}
                Family::Overshoot => notes.push(format!(
                    "node {:?} leaves {name} at {:.0}% on a curve that bounces \
                     ({}); a value on its way to nothing should accelerate away, \
                     not wave - ease-in is the exit's own curve",
                    node.id,
                    at * 100.0,
                    curve_name(&curve)
                )),
                Family::Arrive => notes.push(format!(
                    "node {:?} leaves {name} at {:.0}% easing out; an exit that \
                     settles hangs in the doorway - {}",
                    node.id,
                    at * 100.0,
                    suggested_for(Family::Arrive)
                )),
                Family::Linear => notes.push(format!(
                    "node {:?} leaves {name} at {:.0}% on a straight line; a \
                     linear fade to nothing reads as a switch, not a leave - {}",
                    node.id,
                    at * 100.0,
                    suggested_for(Family::Arrive)
                )),
            }
        }
    }
}

/// The library's name for a curve, for a note that suggests in the same
/// words the easing library answers to. A spring's parameters are the
/// identity when no preset name matches, because "the spring at 260/15"
/// is still a name an author can find.
fn curve_name(curve: &Curve) -> String {
    for name in Curve::NAMES {
        if let Some(named) = Curve::named(name)
            && &named == curve
        {
            return name.to_string();
        }
    }
    match curve {
        Curve::Spring(spring) => format!(
            "spring {}/{}/{}",
            spring.stiffness as u32, spring.damping as u32, spring.mass as u32
        ),
        _ => "the curve it carries".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Blend, Comp, Node, NodeKind, Rgba, Transform};
    use crate::track::{Key, Track, Value};

    /// A one-node comp whose track is given, so every finding below is
    /// the lint's and nothing else's.
    fn comp_with(track: Track) -> Comp {
        let mut node = Node {
            id: "card".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform::default(),
            opacity: 1.0,
            blend: Blend::default(),
            mask: None,
            effects: Vec::new(),
            tracks: crate::track::Tracks::default(),
            kind: NodeKind::Group {
                children: Vec::new(),
            },
        };
        node.tracks.insert("opacity".into(), track);
        Comp {
            format: crate::comp::FORMAT,
            id: "test".into(),
            name: "The test comp".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 2.0,
            background: Rgba::hex("#00000000"),
            assets: Default::default(),
            curves: Default::default(),
            nodes: vec![node],
            slots: Vec::new(),
        }
    }

    /// The most common robotic read: a linear entrance. Flagged, with
    /// the node named.
    #[test]
    fn a_linear_entrance_is_said() {
        let notes = audit(
            &comp_with(Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::new(0.4, Value::number(1.0)),
            ])),
            false,
        );
        assert_eq!(notes.len(), 1, "one finding, not a lecture: {notes:?}");
        assert!(
            notes[0].contains("\"card\""),
            "the node is named: {}",
            notes[0]
        );
        assert!(
            notes[0].contains("straight line"),
            "the curve is named: {}",
            notes[0]
        );
    }

    /// Ease-in on an entrance: the creep. Flagged with the fix.
    #[test]
    fn an_entrance_that_creeps_in_is_said() {
        let notes = audit(
            &comp_with(Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::EASE_IN),
                Key::new(0.4, Value::number(1.0)),
            ])),
            false,
        );
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("easing in"), "{}", notes[0]);
    }

    /// The arrivals the principles ask for: ease-out, emphasis, a spring
    /// that does not bounce. All clean.
    #[test]
    fn an_arrival_that_settles_is_clean() {
        for curve in [Curve::EASE_OUT, Curve::EMPHASIS, Curve::SPRING] {
            let notes = audit(
                &comp_with(Track::new(vec![
                    Key::eased(0.0, Value::number(0.0), curve.clone()),
                    Key::new(0.4, Value::number(1.0)),
                ])),
                false,
            );
            assert!(notes.is_empty(), "{curve:?} flagged: {notes:?}");
        }
    }

    /// A mechanical reveal is a choice: steps() enters without a word
    /// said, whatever the mood.
    #[test]
    fn a_mechanical_reveal_is_a_choice_the_lint_respects() {
        let notes = audit(
            &comp_with(Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::TYPEWRITER),
                Key::new(0.4, Value::number(1.0)),
            ])),
            false,
        );
        assert!(notes.is_empty(), "steps flagged: {notes:?}");
    }

    /// Overshoot is allowed exactly where the style allows it: flagged
    /// on a calm scene, clean on a playful one.
    #[test]
    fn a_bounce_is_punctuation_and_needs_the_scene_to_ask_for_it() {
        let bouncy = || {
            comp_with(Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::SPRING_POP),
                Key::new(0.4, Value::number(1.0)),
            ]))
        };
        assert_eq!(audit(&bouncy(), false).len(), 1, "flagged when calm");
        assert!(audit(&bouncy(), true).is_empty(), "clean when playful");
    }

    /// An exit that settles or creeps: both wrong, both said. ease-in is
    /// the exit's own curve and is clean.
    #[test]
    fn exits_leave_and_arrivals_arrive() {
        // The curve rides the segment's first key - the one that starts
        // it - so each exit below carries its curve on the key it shapes.
        let leaves = || {
            comp_with(Track::new(vec![
                Key::eased(0.6, Value::number(1.0), Curve::EASE_IN),
                Key::new(1.0, Value::number(0.0)),
            ]))
        };
        assert!(audit(&leaves(), false).is_empty(), "ease-in out is clean");
        let hangs = comp_with(Track::new(vec![
            Key::eased(0.6, Value::number(1.0), Curve::EASE_OUT),
            Key::new(1.0, Value::number(0.0)),
        ]));
        let notes = audit(&hangs, false);
        assert_eq!(notes.len(), 1, "an exit that settles is said: {notes:?}");
        assert!(notes[0].contains("hangs in the doorway"), "{}", notes[0]);
        let switch = comp_with(Track::new(vec![
            Key::new(0.6, Value::number(1.0)),
            Key::new(1.0, Value::number(0.0)),
        ]));
        assert_eq!(audit(&switch, false).len(), 1, "a linear exit is said");
    }

    /// A bounce on the way out is flagged whatever the mood: there is no
    /// playful reading of overshooting an exit.
    #[test]
    fn an_overshooting_exit_is_never_what_a_bounce_is_for() {
        let out_on_a_bounce = comp_with(Track::new(vec![
            Key::eased(0.6, Value::number(1.0), Curve::bounce()),
            Key::new(1.0, Value::number(0.0)),
        ]));
        let notes = audit(&out_on_a_bounce, true);
        assert_eq!(notes.len(), 1, "said even when playful: {notes:?}");
        assert!(
            notes[0].contains("bounces"),
            "the finding names the bounce: {}",
            notes[0]
        );
    }

    /// A drift is not an entrance: steady travel between two visible
    /// values is honest motion, and the lint stays out of it.
    #[test]
    fn a_drift_between_visible_values_is_not_the_lint_s_business() {
        let drifting = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
            Key::new(0.5, Value::number(1.0)),
            Key::new(1.0, Value::number(0.6)),
        ]));
        assert!(audit(&drifting, false).is_empty(), "drift flagged");
    }

    /// The library's own presets hold the principles: an audit that flags
    /// a preset is a lint arguing with a designer, and it had better be
    /// the designer who wins.
    #[test]
    fn every_preset_holds_the_principles() {
        for preset in crate::presets::ALL {
            let comp = crate::presets::build(preset.id).expect("a preset");
            let notes = audit(&comp, false);
            assert!(
                notes.is_empty(),
                "{} broke a principle: {notes:?}",
                preset.id
            );
        }
    }
}
