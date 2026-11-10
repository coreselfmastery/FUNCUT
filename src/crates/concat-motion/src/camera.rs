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
    /// How finely the move is baked: the keys a second it is sampled at.
    #[serde(default = "default_keys")]
    pub keys: u32,
    /// `(node id, depth)`: how near each root layer stands, 0.1..=3.0.
    /// Unnamed layers fly the camera's own plane, `1.0`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depths: Vec<(String, f64)>,
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
        }
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
    if camera.zoom.is_none() && camera.pan.is_none() && camera.rotation.is_none() {
        return Err(fail(
            "a camera move needs a zoom, a pan or a rotation".into(),
            "name at least one: zoom (1.0, 1.12) pushes in, pan (dx, dy) drifts, rotation rolls",
        ));
    }
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
    let zoom_of = |c: f64| -> f64 {
        let (from, to) = camera.zoom.unwrap_or((1.0, 1.0));
        from + (to - from) * c
    };
    let pan_of = |c: f64| -> Point {
        let (across, down) = camera.pan.unwrap_or((0.0, 0.0));
        Point::new(across * c, down * c)
    };
    let roll_of = |c: f64| -> f64 {
        let (from, to) = camera.rotation.unwrap_or((0.0, 0.0));
        from + (to - from) * c
    };
    // Where the camera stands at a fraction of the move: the eased
    // progress drives every axis together, so zoom, pan and roll arrive
    // as one move and not three.
    let at = |fraction: f64, depth: f64| -> (f64, Point, f64) {
        let c = camera.curve.apply(fraction.clamp(0.0, 1.0));
        let zoom = zoom_of(c).powf(depth);
        let pan = Point::new(pan_of(c).x * depth, pan_of(c).y * depth);
        (zoom, pan, roll_of(c))
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
            let (zoom, pan, roll) = at(fraction, depth);
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
}
