// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The camera: a move baked into the layers it photographs.
//!
//! A comp has no camera node - the document is layers, not a lens. What a
//! director means by "push in" is still a real thing: every root layer
//! scaled up about the frame's centre, panned by the same drift, turned by
//! the same roll. This module bakes exactly that into each root node's own
//! tracks, key by key, so it renders anywhere the comp renders - and it
//! *composes*: a node that already carries its own motion keeps it, the
//! camera folding around it at every key either set.
//!
//! Depth is the 2.5D of it. A `depths` entry names a node and how near it
//! stands: `1.0` is the plane the camera flies (the default for every
//! layer), a background at `0.4` drifts a little and zooms less, a
//! foreground title at `1.6` is pushed harder. That is parallax without a
//! third axis - the reel layer's own `parallax` rate, as one op a comp can
//! carry on its own.
//!
//! What the camera will not reach, honestly: it moves root nodes. A
//! group's children ride the group's *base* position - the timeline folds
//! a parent's resting place into its children, not its animated track - so
//! a camera over grouped layers moves the group and leaves the children's
//! own places alone. Put camera-moved layers at the root, or name the
//! children in `depths` yourself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::comp::{Comp, Point};
use crate::curve::Curve;
use crate::engine::MotionError;
use crate::track::{Key, Track, Value};

/// The least and most a depth may say.
const DEPTH_RANGE: (f64, f64) = (0.1, 3.0);

/// A camera move an `set-camera` operation bakes into the comp's root
/// layers.
///
/// `zoom` is a multiplier pair (from, to) - `(1.0, 1.12)` is a push-in,
/// `(1.1, 1.0)` a settle; `pan` is the whole drift in pixels (across,
/// down); `rotation` is degrees (from, to), rolled about the frame's
/// centre. All three travel together over `seconds`, in the `curve`.
///
/// A pair is the two-waypoint case of a `path`: three or more waypoints
/// name a camera *path* - push, hold, drift - each axis keyed by the
/// waypoints that name it and held between the ones that do not, every
/// segment eased by the curve its leaving waypoint carries (linear when
/// it carries none, so a path flows instead of stopping at every stop).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CameraMove {
    /// The zoom pair, multiplier to multiplier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<(f64, f64)>,
    /// The pan, `(across, down)` pixels, travelled over the move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<(f64, f64)>,
    /// The roll, degrees about the frame's centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<(f64, f64)>,
    /// How long the move takes.
    #[serde(default = "default_seconds")]
    pub seconds: f64,
    /// The move's shape.
    #[serde(default = "default_curve")]
    pub curve: Curve,
    /// How finely the move is baked: the samples the whole move is keyed
    /// at, its own keys and every root node's riding between them.
    #[serde(default = "default_keys")]
    pub keys: u32,
    /// `(node id, depth)`: how near each root layer stands, 0.1..=3.0.
    /// Unnamed layers fly the camera's own plane, `1.0`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depths: Vec<(String, f64)>,
    /// The waypoints of a camera path, in time order: three or more name
    /// a path, and a path and a pair (any of `zoom`, `pan`, `rotation`)
    /// are refused - the pairs are the two-waypoint path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<Waypoint>,
}

/// One stop on a camera [`path`](CameraMove::path): where the camera
/// stands at `at` seconds into the move, on every axis it names. An axis
/// a waypoint leaves out is keyed by the waypoints that name it and held
/// between them, so `zoom` can push while `pan` waits.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Waypoint {
    /// When the camera stands here, seconds into the move. Times must
    /// strictly increase; the last waypoint is the move's own end.
    pub at: f64,
    /// The zoom multiplier at this stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    /// The pan, `(across, down)` pixels from the resting frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<(f64, f64)>,
    /// The roll, degrees about the frame's centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// How the segment leaving this stop is eased; `None` is linear, so
    /// a path of unnamed segments flows instead of stopping at every
    /// stop. The curve a two-waypoint move carries shapes the whole move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}

fn default_seconds() -> f64 {
    1.0
}

fn default_curve() -> Curve {
    Curve::EASE_IN_OUT
}

fn default_keys() -> u32 {
    24
}

impl Default for CameraMove {
    /// The same move a document without the fields parses: nothing said,
    /// so nothing moved - a move has to name a zoom, a pan or a roll.
    fn default() -> Self {
        CameraMove {
            zoom: None,
            pan: None,
            rotation: None,
            seconds: default_seconds(),
            curve: default_curve(),
            keys: default_keys(),
            depths: Vec::new(),
            path: Vec::new(),
        }
    }
}

/// One axis of a camera path: the stops that name it, in time order,
/// each carrying the curve of the segment that leaves it.
#[derive(Clone, Debug)]
struct Axis {
    stops: Vec<(f64, f64, Option<Curve>)>,
}

impl Axis {
    /// Where the axis stands at `t` seconds: the bracketing stops eased
    /// by the leaving stop's curve, the first stop held from the move's
    /// own start (the same rest a track's first key gives itself), the
    /// last held to the end. An axis without stops is `None`'s absence:
    /// the caller falls back to the pair, or to nothing moving.
    fn at(&self, t: f64) -> f64 {
        let first = self.stops.first().expect("an axis holds a stop");
        if t <= first.0 {
            return first.1;
        }
        for (index, stop) in self.stops.iter().enumerate() {
            if t <= stop.0 {
                let from = &self.stops[index - 1];
                let span = stop.0 - from.0;
                let progress = if span > 1e-9 {
                    (t - from.0) / span
                } else {
                    1.0
                };
                let shaped = from
                    .2
                    .clone()
                    .unwrap_or(Curve::Linear)
                    .apply(progress.clamp(0.0, 1.0));
                return from.1 + (stop.1 - from.1) * shaped;
            }
        }
        self.stops.last().expect("an axis holds a stop").1
    }
}

/// Bake a camera move into the comp's root layers, and say how many it
/// moved.
///
/// Every root node gets the move in its own tracks - position, scale and
/// rotation - merged with whatever motion it already carries, key by key:
/// the union of the node's own keys and the camera's samples, the camera
/// folded around each. Applying a camera twice moves twice, like every
/// authoring op; a move meant to be re-flown is a move applied to the comp
/// before it.
///
/// # Errors
///
/// A move that moves nothing, a zoom that would shrink through zero, a
/// depth outside 0.1..=3.0 or a depth naming a node the comp does not
/// carry, seconds or keys outside their range - each refused with the fix
/// in the message. Nothing is clamped silently.
pub fn apply_camera(comp: &mut Comp, camera: &CameraMove) -> Result<usize, MotionError> {
    // A path and a pair are the same thing said twice; refuse the
    // doubling rather than guessing which half wins.
    let path_is_named = !camera.path.is_empty();
    let pair_is_named = camera.zoom.is_some() || camera.pan.is_some() || camera.rotation.is_some();
    if path_is_named && pair_is_named {
        return Err(fail(
            "a camera move names both a path and a zoom, pan or rotation pair".into(),
            "the pairs are the two-waypoint path; a path names its own ends",
        ));
    }
    if !path_is_named && !pair_is_named {
        return Err(fail(
            "a camera move needs a zoom, a pan or a rotation".into(),
            "name at least one: zoom (1.0, 1.12) pushes in, pan (dx, dy) drifts, rotation rolls",
        ));
    }

    // The path, as axes: each one keyed by the stops that name it. The
    // stops' times must run strictly uphill and end the move; an axis
    // nobody keys is the pair's own, or still.
    let (zoom_axis, pan_x_axis, pan_y_axis, rotation_axis) = if path_is_named {
        let mut zoom = Axis { stops: Vec::new() };
        let mut pan_x = Axis { stops: Vec::new() };
        let mut pan_y = Axis { stops: Vec::new() };
        let mut rotation = Axis { stops: Vec::new() };
        let mut last_at = 0.0f64;
        let mut first = true;
        for waypoint in &camera.path {
            if !waypoint.at.is_finite() || waypoint.at < 0.0 {
                return Err(fail(
                    format!(
                        "a waypoint at {}s is not a time a move can carry",
                        waypoint.at
                    ),
                    "times are seconds into the move, 0 to the move's own length",
                ));
            }
            if !first && waypoint.at <= last_at {
                return Err(fail(
                    format!(
                        "a waypoint at {}s does not follow the one at {last_at}s",
                        waypoint.at
                    ),
                    "the path visits its stops in time order; a hold is a stop with no move",
                ));
            }
            if let Some(value) = waypoint.zoom {
                if !value.is_finite() || value <= 0.0 {
                    return Err(fail(
                        format!("the zoom {value} is not a positive multiplier"),
                        "zoom is a multiplier of the frame: 1.0 rests, 1.12 pushes in",
                    ));
                }
                zoom.stops
                    .push((waypoint.at, value, waypoint.curve.clone()));
            }
            if let Some((across, down)) = waypoint.pan {
                if !across.is_finite() || !down.is_finite() {
                    return Err(fail(
                        format!("the pan ({across}, {down}) is not a pair of numbers"),
                        "pan is pixels from the resting frame: (across, down)",
                    ));
                }
                pan_x
                    .stops
                    .push((waypoint.at, across, waypoint.curve.clone()));
                pan_y
                    .stops
                    .push((waypoint.at, down, waypoint.curve.clone()));
            }
            if let Some(value) = waypoint.rotation {
                if !value.is_finite() {
                    return Err(fail(
                        format!("the rotation {value} is not a number"),
                        "rotation is degrees about the frame's centre",
                    ));
                }
                rotation
                    .stops
                    .push((waypoint.at, value, waypoint.curve.clone()));
            }
            last_at = waypoint.at;
            first = false;
        }
        if last_at < camera.seconds - 1e-9 {
            return Err(fail(
                format!(
                    "the path's last stop is at {last_at}s, short of the move's {}s",
                    camera.seconds
                ),
                "a path runs to its own end: the last waypoint names the move's length",
            ));
        }
        if zoom.stops.len() < 2 && pan_x.stops.is_empty() && rotation.stops.is_empty() {
            return Err(fail(
                "the path keys no axis twice, so nothing moves".into(),
                "at least one axis needs a start and an end: zoom, pan or rotation",
            ));
        }
        if pan_x.stops.len() < 2 && zoom.stops.is_empty() && rotation.stops.is_empty() {
            return Err(fail(
                "the path keys only the pan, and once, so nothing moves".into(),
                "at least one axis needs a start and an end: zoom, pan or rotation",
            ));
        }
        if rotation.stops.len() < 2 && zoom.stops.is_empty() && pan_x.stops.is_empty() {
            return Err(fail(
                "the path keys only the rotation, and once, so nothing moves".into(),
                "at least one axis needs a start and an end: zoom, pan or rotation",
            ));
        }
        (Some(zoom), Some(pan_x), Some(pan_y), Some(rotation))
    } else {
        (None, None, None, None)
    };

    if let Some((from, to)) = camera.zoom
        && (!from.is_finite() || !to.is_finite() || from <= 0.0 || to <= 0.0)
    {
        return Err(fail(
            format!("the zoom pair ({from}, {to}) is not positive multipliers"),
            "zoom is a multiplier of the frame: (1.0, 1.12) pushes in, (0.9, 1.0) settles",
        ));
    }
    if let Some((across, down)) = camera.pan
        && (!across.is_finite() || !down.is_finite())
    {
        return Err(fail(
            format!("the pan ({across}, {down}) is not a pair of numbers"),
            "pan is the whole drift in pixels: (across, down)",
        ));
    }
    if let Some((from, to)) = camera.rotation
        && (!from.is_finite() || !to.is_finite())
    {
        return Err(fail(
            format!("the rotation pair ({from}, {to}) is not a pair of numbers"),
            "rotation is degrees about the frame's centre",
        ));
    }
    if !camera.seconds.is_finite() || camera.seconds <= 0.0 || camera.seconds > comp.duration {
        return Err(fail(
            format!(
                "{} seconds is not a move this {}s comp can carry",
                camera.seconds, comp.duration
            ),
            "the move runs inside the comp's own span",
        ));
    }
    if camera.keys < 4 || camera.keys > 96 {
        return Err(fail(
            format!("{} keys is outside 4..=96", camera.keys),
            "that is the bake's fineness: 24 reads as a move, 96 reads as film",
        ));
    }
    let mut depth_of: BTreeMap<&str, f64> = BTreeMap::new();
    for (id, depth) in &camera.depths {
        if !depth.is_finite() || !(DEPTH_RANGE.0..=DEPTH_RANGE.1).contains(depth) {
            return Err(fail(
                format!("the depth {depth} for {id:?} is outside 0.1..=3.0"),
                "1.0 flies the camera's plane; less is background, more is foreground",
            ));
        }
        if comp.node(id).is_none() {
            return Err(fail(
                format!("the depths name {id:?}, which the comp does not carry"),
                "depths name root nodes: list the comp first",
            ));
        }
        depth_of.insert(id.as_str(), *depth);
    }

    let centre = Point::new(f64::from(comp.width) / 2.0, f64::from(comp.height) / 2.0);
    let zoom_of = |t: f64| -> f64 {
        if let Some(axis) = &zoom_axis
            && !axis.stops.is_empty()
        {
            return axis.at(t);
        }
        let (from, to) = camera.zoom.unwrap_or((1.0, 1.0));
        from + (to - from) * camera.curve.apply((t / camera.seconds).clamp(0.0, 1.0))
    };
    let pan_of = |t: f64| -> Point {
        if let Some(axis) = &pan_x_axis
            && !axis.stops.is_empty()
        {
            // Every waypoint names the whole pair, so both components
            // ride axes keyed at the same times - the interpolation is
            // the pair's own, component by component.
            let x = axis.at(t);
            let y = pan_y_axis.as_ref().map(|a| a.at(t)).unwrap_or(0.0);
            return Point::new(x, y);
        }
        let (across, down) = camera.pan.unwrap_or((0.0, 0.0));
        let c = camera.curve.apply((t / camera.seconds).clamp(0.0, 1.0));
        Point::new(across * c, down * c)
    };
    let roll_of = |t: f64| -> f64 {
        if let Some(axis) = &rotation_axis
            && !axis.stops.is_empty()
        {
            return axis.at(t);
        }
        let (from, to) = camera.rotation.unwrap_or((0.0, 0.0));
        from + (to - from) * camera.curve.apply((t / camera.seconds).clamp(0.0, 1.0))
    };
    // Where the camera stands at a time into the move: every axis reads
    // its own stops, so zoom can push while pan waits. Depth scales the
    // drift and the push both, the way it always did.
    let at = |t: f64, depth: f64| -> (f64, Point, f64) {
        let t = t.clamp(0.0, camera.seconds);
        let zoom = zoom_of(t).powf(depth);
        let pan = Point::new(pan_of(t).x * depth, pan_of(t).y * depth);
        (zoom, pan, roll_of(t))
    };

    // The move's own samples, and every root node's own keys, folded into
    // one set of fractions: the node's keys land where they were, and the
    // camera is sampled at its own fineness between them.
    let mut moved = 0;
    let indices: Vec<usize> = comp
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.parent.is_none())
        .map(|(i, _)| i)
        .collect();
    if indices.is_empty() {
        return Err(fail(
            "the comp carries no root layers for a camera to move".into(),
            "a camera moves the root; a comp of grouped children rides the groups' own places",
        ));
    }
    for index in indices {
        let node = &mut comp.nodes[index];
        let depth = depth_of.get(node.id.as_str()).copied().unwrap_or(1.0);
        let mut fractions: Vec<f64> = (0..=camera.keys)
            .map(|k| f64::from(k) / f64::from(camera.keys))
            .collect();
        for name in ["position", "scale", "rotation"] {
            if let Some(track) = node.tracks.get(name) {
                fractions.extend(track.keys.iter().map(|key| key.at));
            }
        }
        fractions.retain(|f| (0.0..=1.0).contains(f));
        fractions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        fractions.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);

        let (base_position, base_scale, base_rotation) = (
            node.transform.position.unwrap_or(centre),
            node.transform.scale,
            node.transform.rotation,
        );
        let mut position = Track::default();
        let mut scale = Track::default();
        let mut rotation = Track::default();
        for fraction in fractions {
            let (zoom, pan, roll) = at(fraction * camera.seconds, depth);
            // The node's own motion, sampled where it stands: the camera
            // folds around it rather than replacing it.
            let own_position = node
                .tracks
                .get("position")
                .and_then(|track| track.sample(fraction))
                .map(|value| match value {
                    Value::Point { x, y } => Point { x, y },
                    _ => base_position,
                })
                .unwrap_or(base_position);
            let own_scale = node
                .tracks
                .get("scale")
                .and_then(|track| track.sample(fraction))
                .and_then(|value| value.as_number())
                .unwrap_or(base_scale);
            let own_rotation = node
                .tracks
                .get("rotation")
                .and_then(|track| track.sample(fraction))
                .and_then(|value| value.as_number())
                .unwrap_or(base_rotation);
            // The camera's own transform of the layer: pushed out from the
            // centre by the zoom, rolled about it by the roll, drifted by
            // the pan - depth scaling the drift and the push both.
            let lifted = Point::new(
                (own_position.x - centre.x) * zoom,
                (own_position.y - centre.y) * zoom,
            );
            let rolled = roll.to_radians();
            let spun = Point::new(
                lifted.x * rolled.cos() - lifted.y * rolled.sin(),
                lifted.x * rolled.sin() + lifted.y * rolled.cos(),
            );
            position.keys.push(Key::new(
                (fraction * 1000.0).round() / 1000.0,
                Value::Point {
                    x: centre.x + spun.x + pan.x,
                    y: centre.y + spun.y + pan.y,
                },
            ));
            scale.keys.push(Key::new(
                (fraction * 1000.0).round() / 1000.0,
                Value::number(own_scale * zoom),
            ));
            rotation.keys.push(Key::new(
                (fraction * 1000.0).round() / 1000.0,
                Value::number(own_rotation + roll),
            ));
        }
        node.tracks.insert("position".into(), position);
        node.tracks.insert("scale".into(), scale);
        node.tracks.insert("rotation".into(), rotation);
        moved += 1;
    }
    Ok(moved)
}

fn fail(message: String, help: impl Into<String>) -> MotionError {
    MotionError::Engine {
        code: None,
        message,
        path: None,
        help: Some(help.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Length, Node, NodeKind, Paint, Rgba};

    fn comp_with(nodes: Vec<Node>) -> Comp {
        Comp {
            format: crate::comp::FORMAT,
            id: "test.camera".into(),
            name: "Camera".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 6.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            nodes,
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    fn plate(id: &str, x: f64, y: f64) -> Node {
        Node {
            id: id.into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: crate::comp::Transform {
                position: Some(Point::new(x, y)),
                ..Default::default()
            },
            opacity: 1.0,
            blend: crate::comp::Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind: NodeKind::Rect {
                width: Length::px(400.0),
                height: Length::px(300.0),
                fill: Some(Paint::Solid {
                    color: Rgba::hex("#d9a441"),
                }),
                radius: 8.0,
            },
        }
    }

    /// A push-in doubles the distance from the centre: a layer 100px right
    /// of it lands 200px right at zoom 2.
    #[test]
    fn a_push_in_lifts_layers_off_the_centre() {
        let mut comp = comp_with(vec![plate("a", 1060.0, 540.0)]);
        let moved = apply_camera(
            &mut comp,
            &CameraMove {
                zoom: Some((1.0, 2.0)),
                ..CameraMove::default()
            },
        )
        .expect("moves");
        assert_eq!(moved, 1);
        let node = comp.node("a").expect("there");
        let position = node.tracks.get("position").expect("the track");
        let start = position.sample(0.0).expect("keyed");
        let end = position.sample(1.0).expect("keyed");
        match (start, end) {
            (Value::Point { x: a, .. }, Value::Point { x: b, .. }) => {
                assert!((a - 1060.0).abs() < 1e-6, "starts where it sits: {a}");
                assert!((b - 1160.0).abs() < 1e-6, "100px out doubled: {b}");
            }
            _ => panic!("a position track holds points"),
        }
        let scale = node.tracks.get("scale").expect("the track");
        assert_eq!(scale.sample(1.0), Some(Value::number(2.0)));
    }

    /// Depth is the parallax: the background drifts less, the foreground
    /// more, the same pan naming both.
    #[test]
    fn depth_scales_the_drift() {
        let mut comp = comp_with(vec![
            plate("back", 960.0, 540.0),
            plate("front", 960.0, 640.0),
        ]);
        apply_camera(
            &mut comp,
            &CameraMove {
                pan: Some((200.0, 0.0)),
                depths: vec![("back".into(), 0.5), ("front".into(), 1.5)],
                ..CameraMove::default()
            },
        )
        .expect("moves");
        let back = comp
            .node("back")
            .and_then(|n| n.tracks.get("position"))
            .and_then(|t| t.sample(1.0))
            .expect("keyed");
        let front = comp
            .node("front")
            .and_then(|n| n.tracks.get("position"))
            .and_then(|t| t.sample(1.0))
            .expect("keyed");
        match (back, front) {
            (Value::Point { x: bx, .. }, Value::Point { x: fx, .. }) => {
                assert!((bx - (960.0 + 100.0)).abs() < 1e-6, "half the drift: {bx}");
                assert!(
                    (fx - (960.0 + 300.0)).abs() < 1e-6,
                    "more than all of it: {fx}"
                );
            }
            _ => panic!("a position track holds points"),
        }
    }

    /// A node that already moves keeps its motion: the camera folds around
    /// it key by key, its own keys landing where they were.
    #[test]
    fn the_camera_folds_around_a_nodes_own_motion() {
        let mut moving = plate("m", 960.0, 540.0);
        moving.tracks.insert(
            "position".into(),
            Track::new(vec![
                Key::new(0.0, Value::Point { x: 960.0, y: 540.0 }),
                Key::new(
                    1.0,
                    Value::Point {
                        x: 1160.0,
                        y: 540.0,
                    },
                ),
            ]),
        );
        let mut comp = comp_with(vec![moving]);
        apply_camera(
            &mut comp,
            &CameraMove {
                pan: Some((100.0, 0.0)),
                keys: 8,
                ..CameraMove::default()
            },
        )
        .expect("moves");
        let track = comp
            .node("m")
            .unwrap()
            .tracks
            .get("position")
            .expect("there");
        let start = track.sample(0.0).expect("keyed");
        let end = track.sample(1.0).expect("keyed");
        match (start, end) {
            (Value::Point { x: a, .. }, Value::Point { x: b, .. }) => {
                assert!(
                    (a - 960.0).abs() < 1e-6,
                    "its own place, pan not yet begun: {a}"
                );
                assert!(
                    (b - 1260.0).abs() < 1e-6,
                    "its own landing plus the whole pan: {b}"
                );
            }
            _ => panic!("a position track holds points"),
        }
        // Its own key is still a key: the fold did not resample it away.
        assert!(
            track.keys.iter().any(|key| (key.at - 1.0).abs() < 1e-9),
            "the node's own key survives: {:?}",
            track.keys
        );
    }

    /// A roll turns the frame about its centre: a layer to the right of it
    /// swings down when the camera rolls clockwise.
    #[test]
    fn a_roll_swings_layers_about_the_centre() {
        let mut comp = comp_with(vec![plate("a", 1560.0, 540.0)]);
        apply_camera(
            &mut comp,
            &CameraMove {
                rotation: Some((0.0, 90.0)),
                ..CameraMove::default()
            },
        )
        .expect("moves");
        let end = comp
            .node("a")
            .and_then(|n| n.tracks.get("position"))
            .and_then(|t| t.sample(1.0))
            .expect("keyed");
        match end {
            Value::Point { x, y } => {
                assert!((x - 960.0).abs() < 1e-6, "rolled onto the centre line: {x}");
                assert!(y > 540.0, "clockwise swings the right side down: {y}");
            }
            _ => panic!("a position track holds points"),
        }
    }

    /// Children of groups are named honestly: the camera moves roots, and
    /// the refusal for a comp without roots says so.
    #[test]
    fn a_comp_without_root_layers_is_refused_with_the_reason() {
        let mut child = plate("child", 0.0, 0.0);
        child.parent = Some("group".into());
        let mut comp = comp_with(vec![plate("group", 0.0, 0.0), child]);
        comp.nodes.retain(|node| node.id != "group");
        // "child" now points at a parent that is gone; validation would
        // say so - the camera refuses first, on its own grounds.
        let moved = apply_camera(
            &mut comp,
            &CameraMove {
                zoom: Some((1.0, 1.2)),
                ..CameraMove::default()
            },
        );
        assert!(moved.is_err());
    }

    /// Every impossible ask is refused with the fix: nothing is clamped
    /// silently, because a silently-clamped camera is a wrong one.
    #[test]
    fn impossible_moves_are_refused_with_the_fix() {
        let mut comp = comp_with(vec![plate("a", 960.0, 540.0)]);
        let still = CameraMove::default();
        assert!(apply_camera(&mut comp, &still).is_err());
        let through_zero = CameraMove {
            zoom: Some((0.0, 1.0)),
            ..CameraMove::default()
        };
        assert!(apply_camera(&mut comp, &through_zero).is_err());
        let beyond_depth = CameraMove {
            pan: Some((10.0, 0.0)),
            depths: vec![("a".into(), 9.0)],
            ..CameraMove::default()
        };
        assert!(apply_camera(&mut comp, &beyond_depth).is_err());
        let ghost = CameraMove {
            pan: Some((10.0, 0.0)),
            depths: vec![("nope".into(), 1.0)],
            ..CameraMove::default()
        };
        assert!(apply_camera(&mut comp, &ghost).is_err());
        let too_long = CameraMove {
            pan: Some((10.0, 0.0)),
            seconds: 60.0,
            ..CameraMove::default()
        };
        assert!(apply_camera(&mut comp, &too_long).is_err());
    }

    /// A path pushes, holds and pushes again: three waypoints, the zoom
    /// resting on the middle one for the whole of its span.
    #[test]
    fn a_path_pushes_holds_and_pushes_again() {
        let mut comp = comp_with(vec![plate("a", 1060.0, 540.0)]);
        let moved = apply_camera(
            &mut comp,
            &CameraMove {
                seconds: 4.0,
                keys: 12,
                path: vec![
                    Waypoint {
                        at: 0.0,
                        zoom: Some(1.0),
                        ..Waypoint::default()
                    },
                    Waypoint {
                        at: 2.0,
                        zoom: Some(1.2),
                        ..Waypoint::default()
                    },
                    Waypoint {
                        at: 4.0,
                        zoom: Some(1.2),
                        ..Waypoint::default()
                    },
                ],
                ..CameraMove::default()
            },
        )
        .expect("moves");
        assert_eq!(moved, 1);
        let scale = comp
            .node("a")
            .and_then(|n| n.tracks.get("scale"))
            .expect("the track");
        let middle = scale.sample(0.5).expect("keyed");
        let end = scale.sample(1.0).expect("keyed");
        assert_eq!(middle, Value::number(1.2), "held from its stop on");
        assert_eq!(end, Value::number(1.2), "the last stop holds to the end");
        let start = scale.sample(0.0).expect("keyed");
        assert_eq!(start, Value::number(1.0), "the first stop is the rest");
        // The move reaches its middle exactly: half of 4 s is the second
        // stop, and the sampling keyed there.
        let half = scale
            .keys
            .iter()
            .map(|key| key.at)
            .any(|at| (at - 0.5).abs() < 1e-9);
        assert!(half, "the stop at 2s is a key: {:?}", scale.keys);
    }

    /// The axes of a path are keyed independently: the pan can wait while
    /// the zoom pushes, each reading only the stops that name it.
    #[test]
    fn the_axes_of_a_path_are_keyed_independently() {
        let mut comp = comp_with(vec![plate("a", 960.0, 540.0)]);
        apply_camera(
            &mut comp,
            &CameraMove {
                seconds: 4.0,
                keys: 16,
                path: vec![
                    Waypoint {
                        at: 0.0,
                        zoom: Some(1.0),
                        pan: Some((0.0, 0.0)),
                        ..Waypoint::default()
                    },
                    Waypoint {
                        at: 2.0,
                        zoom: Some(1.1),
                        pan: Some((0.0, 0.0)),
                        ..Waypoint::default()
                    },
                    Waypoint {
                        at: 4.0,
                        zoom: Some(1.1),
                        pan: Some((200.0, 40.0)),
                        ..Waypoint::default()
                    },
                ],
                ..CameraMove::default()
            },
        )
        .expect("moves");
        let node = comp.node("a").expect("there");
        let scale = node.tracks.get("scale").expect("the track");
        let position = node.tracks.get("position").expect("the track");
        // The zoom is done at half the move; the pan held its rest there
        // (both stops so far name (0, 0)) and drifts only in the last leg.
        let zoom_at_half = scale.sample(0.5).expect("keyed");
        assert_eq!(zoom_at_half, Value::number(1.1), "the push is done");
        let pan_at_half = position.sample(0.5).expect("keyed");
        match pan_at_half {
            Value::Point { x, y } => {
                assert!((x - 960.0).abs() < 1e-6, "the pan waits: {x}");
                assert!((y - 540.0).abs() < 1e-6, "on both axes: {y}");
            }
            _ => panic!("a position track holds points"),
        }
        // And the pan is whole at the end.
        let pan_at_end = position.sample(1.0).expect("keyed");
        match pan_at_end {
            Value::Point { x, y } => {
                assert!((x - 1160.0).abs() < 1e-6, "the whole drift: {x}");
                assert!((y - 580.0).abs() < 1e-6, "down too: {y}");
            }
            _ => panic!("a position track holds points"),
        }
    }

    /// A waypoint's own curve eases the segment that leaves it: a spring
    /// on the first stop shapes the first leg, and the second leg is
    /// linear because nothing named it.
    #[test]
    fn a_waypoint_curve_eases_its_own_segment() {
        let mut comp = comp_with(vec![plate("a", 960.0, 540.0)]);
        let spring = Curve::Spring(crate::curve::Spring::default());
        apply_camera(
            &mut comp,
            &CameraMove {
                seconds: 4.0,
                keys: 16,
                path: vec![
                    Waypoint {
                        at: 0.0,
                        pan: Some((0.0, 0.0)),
                        curve: Some(spring),
                        ..Waypoint::default()
                    },
                    Waypoint {
                        at: 2.0,
                        pan: Some((200.0, 0.0)),
                        ..Waypoint::default()
                    },
                    Waypoint {
                        at: 4.0,
                        pan: Some((400.0, 0.0)),
                        ..Waypoint::default()
                    },
                ],
                ..CameraMove::default()
            },
        )
        .expect("moves");
        let position = comp
            .node("a")
            .and_then(|n| n.tracks.get("position"))
            .expect("the track");
        // A spring overshoots: somewhere in the first leg the drift runs
        // past where a straight line would have it. Linear never would.
        let mut overshot = false;
        for key in &position.keys {
            if key.at > 0.05
                && key.at < 0.45
                && let Value::Point { x, .. } = &key.value
                && x - 960.0 > (400.0 * key.at) + 1.0
            {
                overshot = true;
            }
        }
        assert!(overshot, "the spring's leg overshoots: {:?}", position.keys);
        // The second leg is linear: at three quarters of the move the
        // drift is halfway through the second leg - 300px of the 400.
        let three_quarters = position.sample(0.75).expect("keyed");
        match three_quarters {
            Value::Point { x, .. } => {
                assert!((x - 1260.0).abs() < 1e-6, "the linear leg is straight: {x}")
            }
            _ => panic!("a position track holds points"),
        }
    }

    /// The path's own refusals: a pair beside it, stops out of order, a
    /// path short of the move, and nothing keyed twice.
    #[test]
    fn impossible_paths_are_refused_with_the_fix() {
        let mut comp = comp_with(vec![plate("a", 960.0, 540.0)]);
        let both = CameraMove {
            seconds: 4.0,
            zoom: Some((1.0, 1.2)),
            path: vec![
                Waypoint {
                    at: 0.0,
                    zoom: Some(1.0),
                    ..Waypoint::default()
                },
                Waypoint {
                    at: 4.0,
                    zoom: Some(1.2),
                    ..Waypoint::default()
                },
            ],
            ..CameraMove::default()
        };
        assert!(
            apply_camera(&mut comp, &both).is_err(),
            "a pair beside a path"
        );
        let backwards = CameraMove {
            seconds: 4.0,
            path: vec![
                Waypoint {
                    at: 2.0,
                    zoom: Some(1.2),
                    ..Waypoint::default()
                },
                Waypoint {
                    at: 1.0,
                    zoom: Some(1.0),
                    ..Waypoint::default()
                },
            ],
            ..CameraMove::default()
        };
        assert!(
            apply_camera(&mut comp, &backwards).is_err(),
            "stops out of order"
        );
        let short = CameraMove {
            seconds: 4.0,
            path: vec![
                Waypoint {
                    at: 0.0,
                    zoom: Some(1.0),
                    ..Waypoint::default()
                },
                Waypoint {
                    at: 2.0,
                    zoom: Some(1.2),
                    ..Waypoint::default()
                },
            ],
            ..CameraMove::default()
        };
        assert!(
            apply_camera(&mut comp, &short).is_err(),
            "short of the move"
        );
        let once = CameraMove {
            seconds: 4.0,
            path: vec![
                Waypoint {
                    at: 0.0,
                    zoom: Some(1.2),
                    ..Waypoint::default()
                },
                Waypoint {
                    at: 4.0,
                    ..Waypoint::default()
                },
            ],
            ..CameraMove::default()
        };
        assert!(
            apply_camera(&mut comp, &once).is_err(),
            "nothing keyed twice"
        );
    }

    /// The documented path shape parses from JSON, with the pair left out
    /// and the axes named by the stops that carry them.
    #[test]
    fn the_documented_path_shape_parses() {
        let move_json: CameraMove = serde_json::from_str(
            r##"{"seconds": 4, "keys": 24,
                "path": [
                  {"at": 0, "zoom": 1.0, "pan": [0, 0]},
                  {"at": 2, "zoom": 1.15},
                  {"at": 4, "zoom": 1.15, "pan": [180, -30]}
                ],
                "depths": [["plate", 0.4]]}"##,
        )
        .expect("the documented path parses");
        assert_eq!(move_json.seconds, 4.0);
        assert_eq!(move_json.path.len(), 3);
        assert_eq!(move_json.path[1].zoom, Some(1.15));
        assert_eq!(move_json.path[1].pan, None);
        assert_eq!(move_json.path[2].pan, Some((180.0, -30.0)));
        assert!(move_json.zoom.is_none() && move_json.pan.is_none());
    }
}
