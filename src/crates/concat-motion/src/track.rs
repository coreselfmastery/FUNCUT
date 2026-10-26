// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Tracks: one animatable property, keyed over a clip's own length.
//!
//! A comp's animation is a map of property name to [`Track`]. Keys are placed
//! at a *fraction* of the node's span, not at an absolute time, so a comp
//! retimed, trimmed or nested inside another comp keeps its motion without
//! anything being re-keyed - the same rule `concat-core`'s clip animations
//! follow, extended from five hard-coded properties to any name.
//!
//! Values are deliberately few. A motion comp needs numbers, points, colours
//! and (for text) strings; anything else is a shape the renderer knows better
//! than the track does.

use serde::{Deserialize, Serialize};

use crate::curve::Curve;

/// What a track carries.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Value {
    /// One number: opacity, rotation, blur radius, a shader parameter.
    Number {
        /// The value.
        value: f64,
    },
    /// A point, in frame pixels: a position, an anchor.
    Point {
        /// The point.
        x: f64,
        /// The point.
        y: f64,
    },
    /// A colour, straight sRGB with alpha, mixed in linear light by the
    /// compositor rather than here.
    Color {
        /// Red, 0 to 1.
        r: f64,
        /// Green, 0 to 1.
        g: f64,
        /// Blue, 0 to 1.
        b: f64,
        /// Alpha, 0 to 1.
        a: f64,
    },
    /// A size: two numbers that interpolate independently.
    Size {
        /// Width.
        width: f64,
        /// Height.
        height: f64,
    },
}

impl Value {
    /// A number as a value.
    pub fn number(value: f64) -> Value {
        Value::Number { value }
    }

    /// The number, if this is one.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number { value } => Some(*value),
            _ => None,
        }
    }

    /// Mix two values. Values of different kinds step at the halfway point
    /// rather than guessing a conversion - the same rule CSS uses for
    /// mismatched keyframe types.
    pub fn mix(from: &Value, to: &Value, u: f64) -> Value {
        match (from, to) {
            (Value::Number { value: a }, Value::Number { value: b }) => {
                Value::number(a + (b - a) * u)
            }
            (Value::Point { x: ax, y: ay }, Value::Point { x: bx, y: by }) => Value::Point {
                x: ax + (bx - ax) * u,
                y: ay + (by - ay) * u,
            },
            (
                Value::Size {
                    width: aw,
                    height: ah,
                },
                Value::Size {
                    width: bw,
                    height: bh,
                },
            ) => Value::Size {
                width: aw + (bw - aw) * u,
                height: ah + (bh - ah) * u,
            },
            (
                Value::Color {
                    r: ar,
                    g: ag,
                    b: ab,
                    a: aa,
                },
                Value::Color {
                    r: br,
                    g: bg,
                    b: bb,
                    a: ba,
                },
            ) => Value::Color {
                r: ar + (br - ar) * u,
                g: ag + (bg - ag) * u,
                b: ab + (bb - ab) * u,
                a: aa + (ba - aa) * u,
            },
            _ => {
                if u < 0.5 {
                    from.clone()
                } else {
                    to.clone()
                }
            }
        }
    }
}

/// One key.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Key {
    /// Where in the node's span, `0..=1`.
    pub at: f64,
    /// What it holds.
    pub value: Value,
    /// How the segment starting here is shaped. `None` is linear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}

impl Key {
    /// A key at `at` holding `value`, linear to the next one.
    pub fn new(at: f64, value: Value) -> Key {
        Key {
            at,
            value,
            curve: None,
        }
    }

    /// The same key, approached with `curve`.
    pub fn eased(at: f64, value: Value, curve: Curve) -> Key {
        Key {
            at,
            value,
            curve: Some(curve),
        }
    }
}

/// A property's keys, in order.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Track {
    /// The keys, sorted by `at` when sampled.
    pub keys: Vec<Key>,
}

impl Track {
    /// A track from keys.
    pub fn new(keys: Vec<Key>) -> Track {
        Track { keys }
    }

    /// A track holding one value for the whole span.
    pub fn constant(value: Value) -> Track {
        Track {
            keys: vec![Key::new(0.0, value)],
        }
    }

    /// Whether there is nothing to sample.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The value at `at`, a fraction of the node's span.
    ///
    /// Before the first key the first value holds; after the last, the last -
    /// `animation-fill-mode: both`, which is what a designer means when they
    /// key two frames of an entrance.
    pub fn sample(&self, at: f64) -> Option<Value> {
        if self.keys.is_empty() {
            return None;
        }
        let mut keys: Vec<&Key> = self.keys.iter().collect();
        keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        if at <= keys[0].at {
            return Some(keys[0].value.clone());
        }
        for pair in keys.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if at <= b.at {
                let span = b.at - a.at;
                if span.abs() < 1e-12 {
                    return Some(b.value.clone());
                }
                let linear = (at - a.at) / span;
                let eased = a.curve.as_ref().unwrap_or(&Curve::Linear).apply(linear);
                return Some(Value::mix(&a.value, &b.value, eased));
            }
        }
        Some(keys[keys.len() - 1].value.clone())
    }
}

/// The tracks a node carries, by property name.
///
/// The names are the ones the editor and the MCP tools speak: `position`,
/// `scale`, `rotation`, `opacity`, `blur`, `dash`, and `param:<key>` for a
/// filter or shader parameter. An engine that does not know a name reports it
/// instead of dropping it silently.
pub type Tracks = std::collections::BTreeMap<String, Track>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    #[test]
    fn a_track_holds_its_ends() {
        let track = Track::new(vec![
            Key::new(0.25, Value::number(0.0)),
            Key::new(0.75, Value::number(1.0)),
        ]);
        assert_eq!(track.sample(0.0).unwrap().as_number(), Some(0.0));
        assert_eq!(track.sample(0.9).unwrap().as_number(), Some(1.0));
    }

    #[test]
    fn a_track_mixes_between_its_keys() {
        let track = Track::new(vec![
            Key::new(0.0, Value::number(0.0)),
            Key::new(1.0, Value::number(10.0)),
        ]);
        assert!((track.sample(0.5).unwrap().as_number().unwrap() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_curve_shapes_the_segment_it_starts() {
        let eased = Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(1.0, Value::number(1.0)),
        ]);
        assert!(eased.sample(0.25).unwrap().as_number().unwrap() > 0.25);
    }

    #[test]
    fn a_spring_overshoots_the_value_it_arrives_at() {
        let popped = Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::SPRING_POP),
            Key::new(1.0, Value::number(1.0)),
        ]);
        let peak = (0..100)
            .map(|i| {
                popped
                    .sample(i as f64 / 100.0)
                    .unwrap()
                    .as_number()
                    .unwrap()
            })
            .fold(0.0_f64, f64::max);
        assert!(peak > 1.0, "the overshoot is the point: peak {peak}");
    }

    #[test]
    fn a_point_mixes_on_both_axes() {
        let track = Track::new(vec![
            Key::new(0.0, Value::Point { x: 0.0, y: 100.0 }),
            Key::new(1.0, Value::Point { x: 10.0, y: 0.0 }),
        ]);
        match track.sample(0.5).unwrap() {
            Value::Point { x, y } => {
                assert!((x - 5.0).abs() < 1e-9);
                assert!((y - 50.0).abs() < 1e-9);
            }
            other => panic!("expected a point, got {other:?}"),
        }
    }

    #[test]
    fn mismatched_kinds_step_at_halfway() {
        let track = Track::new(vec![
            Key::new(0.0, Value::number(0.0)),
            Key::new(1.0, Value::Point { x: 4.0, y: 4.0 }),
        ]);
        assert_eq!(track.sample(0.4).unwrap(), Value::number(0.0));
        assert_eq!(track.sample(0.6).unwrap(), Value::Point { x: 4.0, y: 4.0 });
    }

    #[test]
    fn an_empty_track_samples_nothing() {
        assert!(Track::default().sample(0.5).is_none());
    }
}
