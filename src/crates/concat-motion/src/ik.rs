//! 2D inverse kinematics: the bone chain a rig carries.
//!
//! A rig is a chain of bones, root first, each a length and a rest
//! direction with optional joint limits. The solver poses the chain so
//! its end reaches a target: the last two bones solve analytically by
//! the law of cosines - exact, closed-form, no iteration to converge -
//! and the bones before them sit at their rest. A pole picks the side
//! the middle joint bends to, the way a knee knows forward from
//! backward. Out of reach, the chain stretches straight at the target
//! rather than falling short of it at an odd angle.

use crate::comp::Point;
use serde::{Deserialize, Serialize};

/// One bone of a rig: a segment with a rest direction, the joint
/// limits it keeps, and the node it carries.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Bone {
    /// The bone's length in the node's own pixels.
    pub length: f64,
    /// The bone's resting direction, degrees clockwise from east on
    /// the screen, when the chain is not solving.
    pub rest: f64,
    /// The joint's limits, `(min, max)` degrees measured from the
    /// parent bone's own direction - the first bone's from its rest.
    /// `None` turns freely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<(f64, f64)>,
    /// The node placed at this bone's tip, carried by it: the node's
    /// own transform composes onto the bone's, so the artwork rides
    /// the limb. `None` carries nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach: Option<String>,
}

/// One posed bone: where its joint sits and where its tip lands, in
/// the node's own pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pose {
    /// The joint the bone swings from.
    pub joint: Point,
    /// Where the bone's tip lands.
    pub tip: Point,
}

/// The direction from one point to another, degrees clockwise from
/// east on the screen.
fn direction(from: Point, to: Point) -> f64 {
    (to.y - from.y).atan2(to.x - from.x).to_degrees()
}

/// A point at `distance` from `from`, in `degrees`.
fn step(from: Point, degrees: f64, distance: f64) -> Point {
    let radians = degrees.to_radians();
    Point::new(
        from.x + radians.cos() * distance,
        from.y + radians.sin() * distance,
    )
}

/// Clamps an angle into `(min, max)`, taking the shortest way around
/// the circle. A window may wrap past north (`min > max`).
fn clamp_degrees(angle: f64, (min, max): (f64, f64)) -> f64 {
    let min = min.rem_euclid(360.0);
    let max = max.rem_euclid(360.0);
    let angle = angle.rem_euclid(360.0);
    if min <= max {
        if angle < min {
            // Below the floor: nearer the floor, or wrapped past the
            // ceiling.
            if min - angle <= angle + 360.0 - max {
                min
            } else {
                max
            }
        } else if angle > max {
            if angle - max <= min + 360.0 - angle {
                max
            } else {
                min
            }
        } else {
            angle
        }
    } else if angle >= min || angle <= max {
        angle
    } else if angle - max <= min - angle {
        max
    } else {
        min
    }
}

/// Poses the chain: forward kinematics from rest for every bone before
/// the last two, then the last two solve for the target, then limits
/// clamp each joint against its parent's solved direction. The root
/// sits at `root`; `pole` is the direction the middle joints prefer,
/// degrees, so a knee can know forward from backward.
///
/// An empty chain poses nothing; a chain whose bones are all zero
/// length still poses, every joint on the root.
pub fn pose(bones: &[Bone], root: Point, target: Point, pole: f64) -> Vec<Pose> {
    let mut poses: Vec<Pose> = Vec::with_capacity(bones.len());
    let mut joint = root;
    let mut parent_direction = bones.first().map_or(0.0, |bone| bone.rest);
    for (i, bone) in bones.iter().enumerate() {
        let length = bone.length.max(0.0);
        let solve_pair = i + 2 == bones.len();
        if !solve_pair {
            // Rest, clamped by the limits the joint keeps.
            let mut angle = if i == 0 {
                bone.rest
            } else {
                parent_direction + bone.rest
            };
            if let Some(limits) = bone.limits {
                angle = clamp_degrees(
                    angle,
                    (parent_direction + limits.0, parent_direction + limits.1),
                );
            }
            let tip = step(joint, angle, length);
            poses.push(Pose { joint, tip });
            joint = tip;
            parent_direction = angle;
            continue;
        }
        // The solving pair: this bone and the next reach the target by
        // the law of cosines. The pair's available reach is the two
        // lengths; a target beyond it stretches the pair straight.
        let next = &bones[i + 1];
        let (a, b) = (length, next.length.max(0.0));
        let span = (target.x - joint.x, target.y - joint.y);
        let distance = span.0.hypot(span.1);
        let reach = a + b;
        if reach < 1e-9 {
            let tip = joint;
            poses.push(Pose { joint, tip });
            poses.push(Pose {
                joint: tip,
                tip: target,
            });
            return poses;
        }
        // The distance the triangle actually solves at: clamped into
        // the pair's reach, so a far target is reached at, exactly.
        let d = distance.min(reach).max((a - b).abs());
        let to_target = direction(joint, target);
        let mut upper = to_target;
        let mut lower;
        if d > 1e-9 {
            // The angle at the joint between the reach direction and
            // the upper bone: acos((a^2 + d^2 - b^2) / (2 a d)).
            let cos_upper = ((a * a + d * d - b * b) / (2.0 * a * d)).clamp(-1.0, 1.0);
            let offset = cos_upper.acos().to_degrees();
            // The pole picks the side the joint bends to: the elbow
            // direction nearest the pole.
            let bend = step(joint, to_target + offset, a);
            let anti = step(joint, to_target - offset, a);
            let pole_gap = |point: Point| {
                (direction(joint, point) - pole)
                    .rem_euclid(360.0)
                    .min((pole - direction(joint, point)).rem_euclid(360.0))
            };
            if pole_gap(bend) <= pole_gap(anti) {
                upper = to_target + offset;
            } else {
                upper = to_target - offset;
            }
            lower = direction(step(joint, upper, a), target);
        } else {
            lower = to_target;
        }
        // The limits each joint of the pair keeps, against the parent
        // the walk has already solved.
        if let Some(limits) = bone.limits {
            upper = clamp_degrees(
                upper,
                (parent_direction + limits.0, parent_direction + limits.1),
            );
        }
        let elbow = step(joint, upper, a);
        if let Some(limits) = next.limits {
            lower = clamp_degrees(lower, (upper + limits.0, upper + limits.1));
        }
        let tip = step(elbow, lower, b);
        poses.push(Pose { joint, tip: elbow });
        poses.push(Pose { joint: elbow, tip });
        return poses;
    }
    // A single bone: it turns to point at the target, clamped by its
    // limits; the chain that ran out of bones before a solving pair.
    if bones.len() == 1 {
        let bone = &bones[0];
        let mut angle = direction(root, target);
        if let Some(limits) = bone.limits {
            angle = clamp_degrees(angle, (bone.rest + limits.0, bone.rest + limits.1));
        }
        let tip = step(root, angle, bone.length.max(0.0));
        return vec![Pose { joint: root, tip }];
    }
    poses
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(length: f64, rest: f64) -> Bone {
        Bone {
            length,
            rest,
            limits: None,
            attach: None,
        }
    }

    fn chain(lengths: &[f64]) -> Vec<Bone> {
        lengths.iter().map(|&l| bone(l, 0.0)).collect()
    }

    /// A reachable target is reached exactly, at either knee side.
    #[test]
    fn a_two_bone_chain_reaches() {
        let bones = chain(&[50.0, 50.0]);
        let target = Point::new(60.0, 0.0);
        for pole in [90.0, -90.0] {
            let posed = pose(&bones, Point::new(0.0, 0.0), target, pole);
            assert_eq!(posed.len(), 2, "{posed:?}");
            let tip = posed[1].tip;
            assert!(
                (tip.x - 60.0).abs() < 1e-6 && tip.y.abs() < 1e-6,
                "pole {pole}: {posed:?}"
            );
        }
    }

    /// The pole picks the knee's side: one elbow above the chain, one
    /// below, for the same target.
    #[test]
    fn the_pole_picks_the_knee() {
        let bones = chain(&[50.0, 50.0]);
        let target = Point::new(60.0, 0.0);
        let knee_down = pose(&bones, Point::new(0.0, 0.0), target, 90.0)[0].tip;
        let knee_up = pose(&bones, Point::new(0.0, 0.0), target, -90.0)[0].tip;
        assert!(knee_down.y > 0.0, "{knee_down:?}");
        assert!(knee_up.y < 0.0, "{knee_up:?}");
    }

    /// A target beyond the pair's reach stretches it straight at the
    /// target: the tip lands on the ray, at the reach's own end.
    #[test]
    fn an_unreachable_target_stretches() {
        let bones = chain(&[30.0, 30.0]);
        let posed = pose(&bones, Point::new(0.0, 0.0), Point::new(100.0, 0.0), 90.0);
        let tip = posed[1].tip;
        assert!(
            (tip.x - 60.0).abs() < 1e-6 && tip.y.abs() < 1e-6,
            "{posed:?}"
        );
        // Straight: both bones on the same ray.
        assert!((posed[0].tip.y - 0.0).abs() < 1e-6, "{posed:?}");
    }

    /// A limit keeps the knee from bending backward: the clamped pose
    /// respects the joint's window even off the target.
    #[test]
    fn a_limit_keeps_the_knee() {
        let mut bones = chain(&[50.0, 50.0]);
        bones[1].limits = Some((-90.0, 90.0));
        // The elbow must stay within a half-plane of the upper bone.
        let posed = pose(&bones, Point::new(0.0, 0.0), Point::new(60.0, 0.0), 180.0);
        let (upper, lower) = (
            direction(posed[0].joint, posed[0].tip),
            direction(posed[1].joint, posed[1].tip),
        );
        let bend = (lower - upper).rem_euclid(360.0);
        assert!(bend <= 90.0 || bend >= 270.0, "{posed:?} bend {bend}");
    }

    /// Three bones: the first sits at rest, the last two solve.
    #[test]
    fn three_bones_pose_the_pair() {
        let mut bones = chain(&[40.0, 30.0, 30.0]);
        bones[0].rest = -90.0;
        bones[1].rest = 90.0;
        bones[2].rest = 90.0;
        let target = Point::new(-20.0, 100.0);
        let posed = pose(&bones, Point::new(0.0, 0.0), target, 0.0);
        assert_eq!(posed.len(), 3, "{posed:?}");
        // The first bone sits at its rest, pointing up the screen.
        assert!((posed[0].tip.x - 0.0).abs() < 1e-6, "{posed:?}");
        assert!((posed[0].tip.y - -40.0).abs() < 1e-6, "{posed:?}");
        // And the pair after it reaches.
        let tip = posed[2].tip;
        assert!(
            ((tip.x - target.x).hypot(tip.y - target.y)) < 1e-6,
            "{posed:?}"
        );
    }

    /// A single bone turns to its target, inside its limits.
    #[test]
    fn a_single_bone_turns() {
        let mut bones = chain(&[40.0]);
        bones[0].rest = 0.0;
        bones[0].limits = Some((-45.0, 45.0));
        let posed = pose(&bones, Point::new(0.0, 0.0), Point::new(0.0, -40.0), 0.0);
        // Straight up is -90 degrees, outside the window: the bone
        // clamps to its ceiling at -45.
        let angle = direction(posed[0].joint, posed[0].tip);
        assert!((angle - -45.0).abs() < 1e-6, "{posed:?}");
    }

    /// An empty chain poses nothing.
    #[test]
    fn an_empty_chain_poses_nothing() {
        assert!(pose(&[], Point::new(0.0, 0.0), Point::new(1.0, 1.0), 0.0).is_empty());
    }
}
