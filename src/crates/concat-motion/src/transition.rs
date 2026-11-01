// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Scene-to-scene transitions: the cut between two scenes, choreographed
//! instead of spliced.
//!
//! The router knows the beat; the curtain dips through a colour. Neither
//! of those is a *transition* - a dip is the admission that nothing was
//! choreographed. A [`Cut`] here says what the moments either side of the
//! boundary do: the plates sweep, the focus racks, the zoom travels
//! through, the elements leave one by one. Both sides are written into
//! the timeline the engine already plays - keyframes on the layers'
//! transforms and the comp nodes' tracks - so a transition is a document,
//! not an effect.
//!
//! ```text
//! whip pan left:   outgoing plates sweep left out of frame (ease-in),
//!                  incoming plates sweep in from the right (ease-out)
//! zoom through:    outgoing grows to 1.35, incoming lands from 0.72
//! inverse zoom:    outgoing shrinks to 0.72, incoming arrives from 1.35
//! rack focus:      outgoing blurs to 16px, incoming resolves from 16px
//! velocity match:  the incoming plate enters with the outgoing's own
//!                  drift velocity, and settles - the cut hides itself
//! stagger exit:    the outgoing comp's elements leave one by one, each
//!                  a step after the one before, not on the same frame
//! ```
//!
//! A cut that is choreographed replaces the curtain's dip at that
//! boundary - a whip pan and a colour wash at the same frame is two
//! transitions fighting, and the choreography wins.

use serde::{Deserialize, Serialize};

use crate::comp::{Comp, Effect, Node, Point};
use crate::curve::Curve;
use crate::track::{Key, Value};

/// Which side of a cut a scene sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// The scene the cut leaves: its motion plays at its end.
    Exit,
    /// The scene the cut enters: its motion plays at its start.
    Enter,
}

/// The direction a whip pan travels.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    /// The outgoing scene exits toward the left.
    Left,
    /// Toward the right.
    Right,
    /// Upward.
    Up,
    /// Downward.
    Down,
}

impl Direction {
    /// The unit vector the outgoing scene travels along.
    fn vector(self) -> (f64, f64) {
        match self {
            Direction::Left => (-1.0, 0.0),
            Direction::Right => (1.0, 0.0),
            Direction::Up => (0.0, -1.0),
            Direction::Down => (0.0, 1.0),
        }
    }
}

/// One choreographed cut between two scenes, as a storyboard writes it
/// on the scene the cut enters.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Cut {
    /// A horizontal (or vertical) whip: the outgoing scene sweeps out,
    /// the incoming sweeps in from the opposite side, fast enough to
    /// read as one camera move rather than two edits.
    WhipPan {
        /// The direction the outgoing scene exits toward.
        direction: Direction,
        /// How long each side of the whip takes.
        #[serde(default = "default_whip")]
        seconds: f64,
    },
    /// The camera travels *through* the outgoing scene and out the other
    /// side: outgoing grows to 135 percent, incoming lands from 72.
    ZoomThrough {
        /// How long each side takes.
        #[serde(default = "default_zoom")]
        seconds: f64,
    },
    /// The reverse reading: the outgoing scene shrinks away to 72
    /// percent, the incoming arrives already large at 135 and settles.
    InverseZoom {
        /// How long each side takes.
        #[serde(default = "default_zoom")]
        seconds: f64,
    },
    /// A focus pull across the cut: the outgoing scene blurs out, the
    /// incoming resolves in. Rack focus says "same world, new subject"
    /// like nothing else does.
    RackFocus {
        /// How long each side takes.
        #[serde(default = "default_focus")]
        seconds: f64,
        /// The blur radius at the cut, in pixels.
        #[serde(default = "default_blur")]
        radius: f64,
    },
    /// The cut hides itself: the incoming plate enters with the
    /// outgoing's own drift velocity - direction and speed both - and
    /// settles, so the eye never meets a seam. The direction is read
    /// from the outgoing scene when the plan is laid; nothing to write.
    VelocityMatch {
        /// How long the match takes to settle.
        #[serde(default = "default_match")]
        seconds: f64,
    },
    /// The outgoing scene's elements leave one by one - each a step
    /// after the one before it - instead of on the same frame. The exit
    /// of a list, a row of cards, a cast of names.
    StaggerExit {
        /// How long each element's own exit takes.
        #[serde(default = "default_stagger_seconds")]
        seconds: f64,
        /// The step between one element and the next.
        #[serde(default = "default_stagger_step")]
        step: f64,
    },
}

fn default_whip() -> f64 {
    0.24
}
fn default_zoom() -> f64 {
    0.35
}
fn default_focus() -> f64 {
    0.3
}
fn default_blur() -> f64 {
    16.0
}
fn default_match() -> f64 {
    0.4
}
fn default_stagger_seconds() -> f64 {
    0.22
}
fn default_stagger_step() -> f64 {
    0.06
}

impl Cut {
    /// How long this cut's side of the boundary runs, in seconds.
    pub fn seconds(&self) -> f64 {
        match self {
            Cut::WhipPan { seconds, .. }
            | Cut::ZoomThrough { seconds }
            | Cut::InverseZoom { seconds }
            | Cut::RackFocus { seconds, .. }
            | Cut::VelocityMatch { seconds }
            | Cut::StaggerExit { seconds, .. } => *seconds,
        }
    }

    /// The library's own read of the cut, for a plan's warnings and a
    /// document's own words.
    pub fn name(&self) -> &'static str {
        match self {
            Cut::WhipPan { .. } => "whip-pan",
            Cut::ZoomThrough { .. } => "zoom-through",
            Cut::InverseZoom { .. } => "inverse-zoom",
            Cut::RackFocus { .. } => "rack-focus",
            Cut::VelocityMatch { .. } => "velocity-match",
            Cut::StaggerExit { .. } => "stagger-exit",
        }
    }
}

/// How far a whip carries, as a fraction of the frame: sixty percent
/// reads as a camera move; a hundred leaves nothing on screen to whip
/// with.
const WHIP_REACH: f64 = 0.6;
/// The zoom a zoom-through travels to, and the one an inverse zoom
/// pulls back to. Continuity is the whole read: a push in ends large
/// and the next scene arrives still large, settling; a pull back ends
/// small and the next scene starts small, growing. Both sides of a
/// zoom cut carry the same number - that is what makes it one camera
/// move rather than two edits.
const ZOOM_PUSH: f64 = 1.35;
const ZOOM_PULL: f64 = 0.72;
/// How far a velocity match carries, given the outgoing drift's own
/// speed: the distance its velocity covers in the match's own seconds,
/// capped at half a frame so a fast plate does not throw the incoming
/// one in from off-screen.
const MATCH_CAP: f64 = 0.5;

/// What a cut does to one **plate layer**, as offsets and scales the
/// layer clip writes into its transform keyframes. Positions are
/// fractions of the frame, the same vocabulary a layer's `pan` and
/// `offset` speak; `from` is where the layer starts (enter) or ends
/// (exit) its motion, and `rest` is where it sits once the cut is over.
#[derive(Clone, Debug)]
pub(crate) struct PlateCut {
    /// How long the cut's side runs.
    pub seconds: f64,
    /// The offset (fractions of the frame) at the cut's edge: an enter
    /// starts here and settles to rest; an exit starts at rest and
    /// travels to here.
    pub from: (f64, f64),
    /// The scale at the cut's edge.
    pub scale_from: f64,
    /// The blur radius at the cut's edge, in pixels; `0` when this cut
    /// does not rack.
    pub blur_from: f64,
    /// How the cut's own segment travels.
    pub curve: Curve,
}

/// The plate side of a cut. `drift` is the *outgoing* scene's drift
/// velocity in fractions per second - read by [`Cut::VelocityMatch`],
/// ignored by the rest - and `frame` is the sequence's own frame, for
/// the reach of a whip.
pub(crate) fn plate_cut(cut: &Cut, side: Side, drift: (f64, f64), _frame: (f64, f64)) -> PlateCut {
    let seconds = cut.seconds();
    let sign = match side {
        Side::Enter => -1.0,
        Side::Exit => 1.0,
    };
    match cut {
        Cut::WhipPan { direction, .. } => {
            let (dx, dy) = direction.vector();
            PlateCut {
                seconds,
                from: (dx * WHIP_REACH * sign, dy * WHIP_REACH * sign),
                scale_from: 1.0,
                blur_from: 0.0,
                curve: match side {
                    // Out: accelerate away. In: arrive and settle.
                    Side::Exit => Curve::EASE_IN,
                    Side::Enter => Curve::EASE_OUT,
                },
            }
        }
        Cut::ZoomThrough { .. } => PlateCut {
            seconds,
            from: (0.0, 0.0),
            scale_from: ZOOM_PUSH,
            blur_from: 0.0,
            curve: match side {
                Side::Exit => Curve::EASE_IN,
                Side::Enter => Curve::EASE_OUT,
            },
        },
        Cut::InverseZoom { .. } => PlateCut {
            seconds,
            from: (0.0, 0.0),
            scale_from: ZOOM_PULL,
            blur_from: 0.0,
            curve: match side {
                Side::Exit => Curve::EASE_IN,
                Side::Enter => Curve::EASE_OUT,
            },
        },
        Cut::RackFocus { radius, .. } => PlateCut {
            seconds,
            from: (0.0, 0.0),
            scale_from: 1.0,
            blur_from: match side {
                Side::Exit => *radius,
                Side::Enter => *radius,
            },
            curve: Curve::EASE_IN_OUT,
        },
        Cut::VelocityMatch { .. } => {
            // The incoming plate enters carrying the outgoing's own
            // velocity: the distance that velocity covers in the match's
            // seconds, spent against the drift's direction, capped so a
            // fast plate cannot throw it in from off-screen. The
            // outgoing side does nothing at all - it just keeps drifting
            // the way it was, which is the whole trick.
            let reach = match side {
                Side::Exit => (0.0, 0.0),
                Side::Enter => (
                    (-drift.0 * seconds).clamp(-MATCH_CAP, MATCH_CAP),
                    (-drift.1 * seconds).clamp(-MATCH_CAP, MATCH_CAP),
                ),
            };
            // Three decimals, the crate's own precision: a reach that
            // carries float dust is a cache key that never settles.
            let reach = (
                (reach.0 * 1000.0).round() / 1000.0,
                (reach.1 * 1000.0).round() / 1000.0,
            );
            PlateCut {
                seconds,
                from: reach,
                scale_from: 1.0,
                blur_from: 0.0,
                curve: match side {
                    Side::Exit => Curve::Linear,
                    Side::Enter => Curve::EASE_OUT,
                },
            }
        }
        // A staggered exit is the comp's own business; the plates hold
        // the frame steady while the elements leave.
        Cut::StaggerExit { .. } => PlateCut {
            seconds,
            from: (0.0, 0.0),
            scale_from: 1.0,
            blur_from: 0.0,
            curve: Curve::Linear,
        },
    }
}

/// The comp side of a cut: the tracks its nodes carry at the boundary.
/// Exit tracks play over the last `seconds` of the scene; enter tracks
/// over the first. A node that is not on screen for the whole window
/// keeps its own motion - the ones a staggered exit exists for are.
///
/// Keys are appended to a node's own tracks, never inserted among them:
/// a comp's authored animation is deliberate, and a transition rides on
/// top of it rather than rewriting it. A node whose own track already
/// runs past where the cut's keys would land keeps what it wrote.
pub(crate) fn prepare_comp(comp: &mut Comp, side: Side, cut: &Cut, drift: (f64, f64)) {
    let seconds = cut.seconds().max(0.0);
    if seconds <= 1e-9 {
        return;
    }
    let frame = (f64::from(comp.width), f64::from(comp.height));
    let count = comp.nodes.len();
    for (index, node) in comp.nodes.iter_mut().enumerate() {
        let span = if node.duration > 0.0 {
            node.duration
        } else {
            (comp.duration - node.start).max(0.0)
        };
        if span <= 1e-9 {
            continue;
        }
        // The stagger: each element's exit begins a step after the one
        // before it, front of the list first - the leader leaves, the
        // followers trail, and the last one is gone exactly at the cut.
        let stagger = match cut {
            Cut::StaggerExit { step, .. } => {
                (count.saturating_sub(1) - index) as f64 * step.max(0.0)
            }
            _ => 0.0,
        };
        match side {
            Side::Enter => enter_node(node, cut, span, frame, drift),
            Side::Exit => exit_node(node, cut, span, stagger, frame),
        }
    }
}

/// The enter side: motion over the node's first `seconds`.
fn enter_node(node: &mut Node, cut: &Cut, span: f64, frame: (f64, f64), drift: (f64, f64)) {
    let seconds = cut.seconds().min(span);
    if seconds <= 1e-9 {
        return;
    }
    match cut {
        Cut::WhipPan { direction, .. } => {
            let (dx, dy) = direction.vector();
            slide(
                node,
                span,
                0.0,
                seconds,
                -dx * WHIP_REACH,
                -dy * WHIP_REACH,
                Side::Enter,
                frame,
            );
        }
        Cut::ZoomThrough { .. } => {
            scale_to(node, span, 0.0, seconds, ZOOM_PUSH, Side::Enter);
        }
        Cut::InverseZoom { .. } => {
            scale_to(node, span, 0.0, seconds, ZOOM_PULL, Side::Enter);
        }
        Cut::RackFocus { radius, .. } => rack(node, span, 0.0, seconds, *radius, Side::Enter),
        Cut::VelocityMatch { .. } => {
            // The comp's own entrance carries the plate's incoming
            // reach: the distance the outgoing drift covers in the
            // match's seconds, capped, spent against the drift's own
            // direction.
            let reach = (
                (-drift.0 * seconds).clamp(-MATCH_CAP, MATCH_CAP),
                (-drift.1 * seconds).clamp(-MATCH_CAP, MATCH_CAP),
            );
            slide(
                node,
                span,
                0.0,
                seconds,
                reach.0,
                reach.1,
                Side::Enter,
                frame,
            );
        }
        Cut::StaggerExit { .. } => {}
    }
}

/// The exit side: motion over the node's last `seconds`, staggered by
/// `stagger` for the cuts that stagger.
fn exit_node(node: &mut Node, cut: &Cut, span: f64, stagger: f64, frame: (f64, f64)) {
    // A node whose own span ends before the cut's window reaches it is
    // not on screen to leave: it keeps what it wrote.
    let window = cut.seconds() + stagger;
    if window >= span {
        return;
    }
    match cut {
        Cut::WhipPan { direction, .. } => {
            let (dx, dy) = direction.vector();
            slide(
                node,
                span,
                span - cut.seconds() - stagger,
                cut.seconds(),
                dx * WHIP_REACH,
                dy * WHIP_REACH,
                Side::Exit,
                frame,
            );
        }
        Cut::ZoomThrough { .. } => {
            scale_to(
                node,
                span,
                span - cut.seconds(),
                cut.seconds(),
                ZOOM_PUSH,
                Side::Exit,
            );
        }
        Cut::InverseZoom { .. } => {
            scale_to(
                node,
                span,
                span - cut.seconds(),
                cut.seconds(),
                ZOOM_PULL,
                Side::Exit,
            );
        }
        Cut::RackFocus { radius, .. } => rack(
            node,
            span,
            span - cut.seconds(),
            cut.seconds(),
            *radius,
            Side::Exit,
        ),
        Cut::VelocityMatch { .. } => {}
        Cut::StaggerExit { seconds, .. } => {
            // Each element fades out over its own window, a step after
            // the one before it, with a small drop that reads as
            // "leaving" rather than "switched off".
            let start = span - stagger - seconds;
            fade_out(node, span, start, *seconds);
            slide(node, span, start, *seconds, 0.0, 0.04, Side::Exit, frame);
        }
    }
}

/// Slide a node between its place and an offset over `seconds` starting
/// `at` seconds into its span. The offset is in fractions of the comp's
/// own frame, spent as pixels; a node that is never placed keeps its own
/// position, because inventing one would move it. The side says which
/// end the offset is: an enter *starts* there and settles, an exit
/// *travels* there and ends.
fn slide(
    node: &mut Node,
    span: f64,
    at: f64,
    seconds: f64,
    dx: f64,
    dy: f64,
    side: Side,
    frame: (f64, f64),
) {
    let Some(base) = placed_at(node) else {
        return;
    };
    let away = Point::new(base.x + dx * frame.0, base.y + dy * frame.1);
    // Out accelerates away. In arrives and settles.
    let curve = match side {
        Side::Exit => Curve::EASE_IN,
        Side::Enter => Curve::EASE_OUT,
    };
    let keys = match side {
        Side::Enter => [
            (
                at,
                Value::Point {
                    x: away.x,
                    y: away.y,
                },
                Some(curve),
            ),
            (
                at + seconds,
                Value::Point {
                    x: base.x,
                    y: base.y,
                },
                None,
            ),
        ],
        // The curve starts the segment - Geneva's `ease` shapes the
        // segment starting at its keyframe, so a curve on the segment's
        // last key shapes nothing and the exit slides a straight line.
        Side::Exit => [
            (
                at,
                Value::Point {
                    x: base.x,
                    y: base.y,
                },
                Some(curve),
            ),
            (
                at + seconds,
                Value::Point {
                    x: away.x,
                    y: away.y,
                },
                None,
            ),
        ],
    };
    append_keys(node, "position", span, &keys);
}

/// Scale a node between `from` and its resting scale over `seconds`
/// starting `at` into its span: an enter arrives from `from` and
/// settles, an exit rests and travels to `from`.
fn scale_to(node: &mut Node, span: f64, at: f64, seconds: f64, from: f64, side: Side) {
    let rest = node.transform.scale;
    let curve = match side {
        Side::Exit => Curve::EASE_IN,
        Side::Enter => Curve::EASE_OUT,
    };
    let keys = match side {
        Side::Enter => [
            (at, Value::number(from), Some(curve)),
            (at + seconds, Value::number(rest), None),
        ],
        // The curve starts the segment it shapes; see `slide`.
        Side::Exit => [
            (at, Value::number(rest), Some(curve)),
            (at + seconds, Value::number(from), None),
        ],
    };
    append_keys(node, "scale", span, &keys);
}

/// Rack a node's focus to `radius` and back to sharp over `seconds`
/// starting `at` into its span: an enter resolves in from the radius, an
/// exit blurs out to it. A node that already blurs keeps its own radius
/// as the rest value.
fn rack(node: &mut Node, span: f64, at: f64, seconds: f64, radius: f64, side: Side) {
    let rest = match node.effects.as_slice() {
        [Effect::Blur { radius, .. }, ..] => *radius,
        _ => {
            node.effects.insert(0, Effect::Blur { radius: 0.0 });
            0.0
        }
    };
    if (radius - rest).abs() <= 1e-9 {
        return;
    }
    let curve = Curve::EASE_IN_OUT;
    let keys = match side {
        Side::Enter => [
            (at, Value::number(radius), Some(curve)),
            (at + seconds, Value::number(rest), None),
        ],
        // The curve starts the segment it shapes; see `slide`.
        Side::Exit => [
            (at, Value::number(rest), Some(curve)),
            (at + seconds, Value::number(radius), None),
        ],
    };
    append_keys(node, "blur", span, &keys);
}

/// Fade a node out over `seconds` starting `at` into its span.
fn fade_out(node: &mut Node, span: f64, at: f64, seconds: f64) {
    let rest = node.opacity;
    // The curve starts the segment it shapes; see `slide`.
    append_keys(
        node,
        "opacity",
        span,
        &[
            (at, Value::number(rest), Some(Curve::EASE_IN)),
            (at + seconds, Value::number(0.0), None),
        ],
    );
}

/// Append keys to a node's track, as fractions of its span, keeping
/// every existing key: a key that would land at or before the track's
/// own last one is dropped, because the author's animation is not the
/// transition's to rewrite.
fn append_keys(node: &mut Node, property: &str, span: f64, keys: &[(f64, Value, Option<Curve>)]) {
    let span = span.max(1e-9);
    // The rest value is read before the track is borrowed, for the single
    // key that would otherwise animate nothing.
    let rest = if !node.tracks.contains_key(property) {
        rest_value(node, property)
    } else {
        None
    };
    let track = node.tracks.entry(property.into()).or_default();
    let last = track.keys.iter().map(|key| key.at).fold(f64::MIN, f64::max);
    for (at, value, curve) in keys {
        let at: f64 = (at / span).clamp(0.0, 1.0);
        if at <= last + 1e-6 {
            continue;
        }
        let value: Value = value.clone();
        let curve: Option<Curve> = curve.clone();
        track.keys.push(Key { at, value, curve });
    }
    if track.keys.len() == 1
        && let Some(rest) = rest
    {
        // A single key animates nothing and fails validate: the value at
        // rest is the node's own, so the lone key is the transition's
        // far end and the rest is written for it to travel from.
        track.keys.insert(
            0,
            Key {
                at: 0.0,
                value: rest,
                curve: None,
            },
        );
    }
}

/// The value a property rests at, for a track that would otherwise hold
/// a single key.
fn rest_value(node: &Node, property: &str) -> Option<Value> {
    match property {
        "position" => node.transform.position.map(|point| Value::Point {
            x: point.x,
            y: point.y,
        }),
        "scale" => Some(Value::number(node.transform.scale)),
        "opacity" => Some(Value::number(node.opacity)),
        "blur" => Some(Value::number(0.0)),
        _ => None,
    }
}

/// Where a node is placed: its own base position, or the last key of
/// its position track - the one the transition rides from.
fn placed_at(node: &Node) -> Option<Point> {
    if let Some(track) = node.tracks.get("position") {
        let mut keys = track.keys.iter().collect::<Vec<_>>();
        keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(Value::Point { x, y }) = keys.last().map(|key| &key.value) {
            return Some(Point::new(*x, *y));
        }
    }
    node.transform.position
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, NodeKind, Rgba};
    use crate::track::{Track, Value};

    fn node_comp(node: Node) -> Comp {
        Comp {
            format: FORMAT,
            id: "test.cut".into(),
            name: "Cut".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 4.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: std::collections::BTreeMap::new(),
            curves: std::collections::BTreeMap::new(),
            nodes: vec![node],
            slots: Vec::new(),
        }
    }

    fn placed_node() -> Node {
        Node {
            id: "card".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: crate::comp::Transform {
                position: Some(Point::new(960.0, 540.0)),
                ..Default::default()
            },
            opacity: 1.0,
            blend: crate::comp::Blend::Normal,
            mask: None,
            effects: Vec::new(),
            tracks: std::collections::BTreeMap::new(),
            kind: NodeKind::Rect {
                width: crate::comp::Length::px(200.0),
                height: crate::comp::Length::px(80.0),
                fill: None,
                radius: 0.0,
            },
        }
    }

    /// A whip pan slides the outgoing node out and the incoming node in,
    /// from opposite sides, and the timeline carries both as position
    /// keyframes with the curves a whip reads by.
    #[test]
    fn a_whip_pan_slides_both_sides_from_opposite_ends() {
        let cut = Cut::WhipPan {
            direction: Direction::Left,
            seconds: 0.24,
        };
        let mut outgoing = node_comp(placed_node());
        prepare_comp(&mut outgoing, Side::Exit, &cut, (0.0, 0.0));
        let track = &outgoing.nodes[0].tracks["position"];
        let last = track.keys.last().expect("the exit key");
        assert_eq!(
            last.value,
            Value::Point {
                x: 960.0 - 0.6 * 1920.0,
                y: 540.0
            }
        );
        // Out accelerates away - and the curve that accelerates it rides
        // the key the segment starts from, for Geneva's `ease` shapes the
        // segment starting at its keyframe: on the exit's last key it
        // would shape nothing, and the sweep would run straight.
        let first = track.keys.first().expect("the segment's start");
        assert_eq!(first.curve, Some(Curve::EASE_IN));

        let mut incoming = node_comp(placed_node());
        prepare_comp(&mut incoming, Side::Enter, &cut, (0.0, 0.0));
        let track = &incoming.nodes[0].tracks["position"];
        let first = track.keys.first().expect("the enter key");
        assert_eq!(
            first.value,
            Value::Point {
                x: 960.0 + 0.6 * 1920.0,
                y: 540.0
            }
        );
        // In arrives and settles.
        assert_eq!(first.curve, Some(Curve::EASE_OUT));
    }

    /// A zoom-through grows the outgoing scene and lands the incoming
    /// one from large; the inverse zoom pulls back and grows in - both
    /// sides carry the same number, which is the continuity.
    #[test]
    fn a_zoom_through_grows_out_and_lands_in() {
        let cut = Cut::ZoomThrough { seconds: 0.35 };
        let mut outgoing = node_comp(placed_node());
        prepare_comp(&mut outgoing, Side::Exit, &cut, (0.0, 0.0));
        let last = outgoing.nodes[0].tracks["scale"]
            .keys
            .last()
            .expect("the exit key");
        assert_eq!(last.value, Value::number(1.35));

        let mut incoming = node_comp(placed_node());
        prepare_comp(&mut incoming, Side::Enter, &cut, (0.0, 0.0));
        let first = incoming.nodes[0].tracks["scale"]
            .keys
            .first()
            .expect("the enter key");
        assert_eq!(first.value, Value::number(1.35));

        let inverse = Cut::InverseZoom { seconds: 0.35 };
        let mut outgoing = node_comp(placed_node());
        prepare_comp(&mut outgoing, Side::Exit, &inverse, (0.0, 0.0));
        let last = outgoing.nodes[0].tracks["scale"]
            .keys
            .last()
            .expect("the exit key");
        assert_eq!(last.value, Value::number(0.72));
        let mut incoming = node_comp(placed_node());
        prepare_comp(&mut incoming, Side::Enter, &inverse, (0.0, 0.0));
        let first = incoming.nodes[0].tracks["scale"]
            .keys
            .first()
            .expect("the enter key");
        assert_eq!(first.value, Value::number(0.72));
    }

    /// A rack focus blurs the outgoing comp out to the radius and
    /// resolves the incoming one in from it, as a blur effect with a
    /// keyframed radius.
    #[test]
    fn a_rack_focus_blurs_out_and_resolves_in() {
        let cut = Cut::RackFocus {
            seconds: 0.3,
            radius: 16.0,
        };
        let mut outgoing = node_comp(placed_node());
        prepare_comp(&mut outgoing, Side::Exit, &cut, (0.0, 0.0));
        let node = &outgoing.nodes[0];
        assert!(matches!(node.effects.first(), Some(Effect::Blur { .. })));
        let last = node.tracks["blur"].keys.last().expect("the blur key");
        assert_eq!(last.value, Value::number(16.0));

        let mut incoming = node_comp(placed_node());
        prepare_comp(&mut incoming, Side::Enter, &cut, (0.0, 0.0));
        let first = incoming.nodes[0].tracks["blur"]
            .keys
            .first()
            .expect("the blur key");
        assert_eq!(first.value, Value::number(16.0));
    }

    /// A staggered exit leaves one element after the next: three cards,
    /// each a step after the one before it, each fading out over its own
    /// window - not on the same frame.
    #[test]
    fn a_staggered_exit_leaves_one_element_after_the_next() {
        let cut = Cut::StaggerExit {
            seconds: 0.22,
            step: 0.06,
        };
        let mut comp = node_comp(placed_node());
        for index in 1..3 {
            let mut card = placed_node();
            card.id = format!("card{index}");
            comp.nodes.push(card);
        }
        prepare_comp(&mut comp, Side::Exit, &cut, (0.0, 0.0));
        let starts: Vec<Option<f64>> = comp
            .nodes
            .iter()
            .map(|node| {
                node.tracks
                    .get("opacity")
                    .and_then(|track| track.keys.first().map(|key| key.at))
            })
            .collect();
        // The front of the list leaves first, the last one is gone
        // exactly at the cut: element 0 begins a step ahead of element 1.
        for (index, start) in starts.iter().enumerate() {
            let start = start.expect("every element fades");
            let expected = (4.0 - 0.22 - (2 - index) as f64 * 0.06) / 4.0;
            assert!(
                (start - expected).abs() < 1e-6,
                "{index}: {start} vs {expected}"
            );
        }
        assert!(starts[0] < starts[1] && starts[1] < starts[2], "{starts:?}");
        // And each one lands at zero.
        for node in &comp.nodes {
            let last = node.tracks["opacity"].keys.last().expect("the end");
            assert_eq!(last.value, Value::number(0.0));
        }
    }

    /// The authored animation is never rewritten: a node whose own
    /// track runs past where the cut's keys would land keeps what it
    /// wrote, and a node that is never placed is not given a position.
    #[test]
    fn a_transition_rides_on_top_of_the_authored_motion() {
        let cut = Cut::ZoomThrough { seconds: 0.35 };
        let mut node = placed_node();
        node.tracks.insert(
            "scale".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.9)),
                Key::new(0.95, Value::number(1.0)),
            ]),
        );
        let mut comp = node_comp(node);
        prepare_comp(&mut comp, Side::Exit, &cut, (0.0, 0.0));
        let track = &comp.nodes[0].tracks["scale"];
        // The author's own keys stand untouched: 0.9 at 0, 1.0 at 0.95.
        // The cut's first key (at (4.0 - 0.35) / 4.0 = 0.9125) lands
        // before the author's last, and is dropped rather than inserted
        // among the author's own; the cut's *end* value lands after it,
        // and is appended - the exit still arrives, shortened to the
        // room the author left it.
        assert_eq!(track.keys.len(), 3, "{:?}", track.keys);
        assert_eq!(track.keys[0].at, 0.0);
        assert_eq!(track.keys[1].at, 0.95);
        assert_eq!(track.keys[1].value, Value::number(1.0));
        assert_eq!(track.keys[2].at, 1.0);
        assert_eq!(track.keys[2].value, Value::number(1.35));

        // A node with no position is not invented one by a whip.
        let mut bare = placed_node();
        bare.transform.position = None;
        let mut comp = node_comp(bare);
        prepare_comp(
            &mut comp,
            Side::Exit,
            &Cut::WhipPan {
                direction: Direction::Left,
                seconds: 0.24,
            },
            (0.0, 0.0),
        );
        assert!(
            !comp.nodes[0].tracks.contains_key("position"),
            "no position was invented"
        );
    }

    /// The plate side: a whip's reach is the vocabulary a layer's pan
    /// speaks - fractions of the frame, opposite signs either side of
    /// the cut - and a velocity match reads the drift it is handed.
    #[test]
    fn the_plate_side_carries_the_cut_in_the_layers_own_vocabulary() {
        let whip = Cut::WhipPan {
            direction: Direction::Left,
            seconds: 0.24,
        };
        let exit = plate_cut(&whip, Side::Exit, (0.0, 0.0), (1920.0, 1080.0));
        let enter = plate_cut(&whip, Side::Enter, (0.0, 0.0), (1920.0, 1080.0));
        assert_eq!(exit.from, (-WHIP_REACH, 0.0));
        assert_eq!(enter.from, (WHIP_REACH, 0.0));
        assert_eq!(exit.curve, Curve::EASE_IN);

        // A velocity match spends the drift's own distance, capped.
        let drift = (0.1, 0.0); // a tenth of a frame per second
        let cut = Cut::VelocityMatch { seconds: 0.4 };
        let matched = plate_cut(&cut, Side::Enter, drift, (1920.0, 1080.0));
        assert_eq!(matched.from, (-0.04, 0.0));
        let fast = plate_cut(&cut, Side::Enter, (5.0, 0.0), (1920.0, 1080.0));
        assert_eq!(fast.from, (-MATCH_CAP, 0.0));
        // The outgoing side of a velocity match does nothing: it keeps
        // drifting the way it was, which is the whole trick.
        let outgoing = plate_cut(&cut, Side::Exit, drift, (1920.0, 1080.0));
        assert_eq!(outgoing.from, (0.0, 0.0));
    }
}
