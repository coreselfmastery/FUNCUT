// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The GSAP adapter: a timeline a web build would recognise, written onto
//! a comp's nodes as tracks.
//!
//! The document is an array of tweens, the shape a `gsap.to()` call would
//! leave behind:
//!
//! ```json
//! [
//!   { "targets": ["headline", "sub"],
//!     "from": { "opacity": 0, "y": 24 },
//!     "to":   { "opacity": 1, "y": 0 },
//!     "duration": 0.6, "ease": "power2.out", "stagger": 0.08 },
//!   { "targets": "mark", "to": { "rotation": 8 },
//!     "duration": 0.4, "delay": 0.2, "repeat": 1, "yoyo": true }
//! ]
//! ```
//!
//! What a tween may carry: `targets` (one node id or an array), `from` and
//! `to` objects over `x`, `y`, `scale`, `rotation`, `opacity` and `blur`,
//! `duration` and `delay` in seconds, `stagger` between array targets,
//! `repeat` (a count; `-1` is refused rather than guessed at), `yoyo`, and
//! an `ease` by name. The named eases are the families the comp's curves
//! answer to - `power1..4`, `sine`, `expo`, `circ` and `back`, each as
//! `.in`, `.out` and `.inOut`, plus `none`, `linear` and `steps(n)`;
//! `elastic.out` is read as the library's pop spring and says so, because
//! a damped spring is what an elastic ease is.
//!
//! The semantics are the comp's, stated plainly: a tween's window is its
//! `delay` to `delay + duration × (repeat + 1)` in seconds, keyed as
//! fractions of the target node's own span; `x` and `y` are one position
//! track, written together the way a position moves; a tween on a property
//! replaces the track that property carried; and the value the tween
//! starts from is the `from` value when there is one, the node's static
//! value when there is not. A tween whose window falls outside a node's
//! span is skipped with a note naming the node, because a track that never
//! samples is not an animation - it is a mistake wearing one.

use std::collections::BTreeMap;

use concat_motion::comp::{Comp, Node};
use concat_motion::curve::{Curve, StepPosition};
use concat_motion::track::{Key, Track, Value};
use serde_json::Value as Json;

use crate::AdapterError;

/// Apply a GSAP-style timeline to a comp.
///
/// The comp comes back with the tweens written onto their targets' tracks,
/// and the notes say every skip and approximation - the same contract as
/// the Lottie adapter's [`Import`](crate::Import), returned as a pair
/// because the comp is the caller's own document rather than one the
/// adapter built.
pub fn apply(comp: &Comp, timeline: &Json) -> Result<(Comp, Vec<String>), AdapterError> {
    let tweens = timeline
        .as_array()
        .ok_or_else(|| AdapterError::NotTheFormat {
            format: "GSAP-style timeline",
            said: "the timeline is an array of tweens".into(),
        })?;
    let mut made = comp.clone();
    let mut notes = Vec::new();
    let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
    for (index, node) in made.nodes.iter().enumerate() {
        by_id.insert(node.id.clone(), index);
    }

    for (index, tween) in tweens.iter().enumerate() {
        let targets = targets_of(tween);
        if targets.is_empty() {
            notes.push(format!(
                "skipped: tween {index} names no target the comp carries"
            ));
            continue;
        }
        let stagger = tween
            .get("stagger")
            .and_then(Json::as_f64)
            .filter(|v| *v >= 0.0)
            .unwrap_or(0.0);
        for (place, target) in targets.iter().enumerate() {
            let Some(&at) = by_id.get(target) else {
                notes.push(format!(
                    "skipped: tween {index} targets node {target:?}, which the comp does not carry"
                ));
                continue;
            };
            let delay = tween
                .get("delay")
                .and_then(Json::as_f64)
                .filter(|v| *v >= 0.0)
                .unwrap_or(0.0)
                + stagger * place as f64;
            apply_one(&mut made.nodes[at], tween, delay, target, index, &mut notes);
        }
    }
    let problems = made
        .validate()
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect::<Vec<String>>();
    if !problems.is_empty() {
        return Err(AdapterError::Invalid { problems });
    }
    Ok((made, notes))
}

/// One tween against one node.
fn apply_one(
    node: &mut Node,
    tween: &Json,
    delay: f64,
    target: &str,
    index: usize,
    notes: &mut Vec<String>,
) {
    let span = if node.duration > 0.0 {
        node.duration
    } else {
        notes.push(format!(
            "skipped: tween {index} on node {target:?}, which has no span of its own to key against"
        ));
        return;
    };

    let duration = tween
        .get("duration")
        .and_then(Json::as_f64)
        .filter(|v| *v > 0.0)
        .unwrap_or(0.5);
    let repeat = match tween.get("repeat") {
        None => 0,
        Some(value) if value.as_f64() == Some(-1.0) => {
            notes.push(format!(
                "skipped: tween {index} on node {target:?} repeats forever; a comp node's span is finite"
            ));
            return;
        }
        Some(value) => value.as_u64().unwrap_or(0),
    };
    let yoyo = tween.get("yoyo").and_then(Json::as_bool).unwrap_or(false);
    let total = delay + duration * (repeat as f64 + 1.0);
    if delay >= span {
        notes.push(format!(
            "skipped: tween {index} on node {target:?} starts at {delay:.2}s, past the node's {span:.2}s span"
        ));
        return;
    }
    if total > span {
        notes.push(format!(
            "clamped: tween {index} on node {target:?} runs to {total:.2}s, past the node's {span:.2}s span; the cycles that do not fit are dropped"
        ));
    }
    let Some(ease) = ease_of(tween, notes, index, target) else {
        return;
    };

    let from = tween.get("from");
    let Some(to) = tween
        .get("to")
        .or_else(|| tween.get("toVars"))
        .and_then(Json::as_object)
    else {
        notes.push(format!(
            "skipped: tween {index} on node {target:?} has no `to` (or `toVars`) object to arrive at"
        ));
        return;
    };

    // x and y are one position: gathered together, keyed together, so a
    // tween that moves a node diagonally arrives diagonally.
    let x_to = number_in(to, "x");
    let y_to = number_in(to, "y");
    if x_to.is_some() || y_to.is_some() {
        let base = node
            .transform
            .position
            .map(|point| (point.x, point.y))
            .unwrap_or((0.0, 0.0));
        let from_pair = from.and_then(Json::as_object).map(|from| {
            (
                number_in(from, "x").unwrap_or(base.0),
                number_in(from, "y").unwrap_or(base.1),
            )
        });
        let to_pair = (x_to.unwrap_or(base.0), y_to.unwrap_or(base.1));
        write_track(
            node,
            "position",
            delay,
            duration,
            repeat,
            yoyo,
            span,
            move |leaves| Value::Point {
                x: leaves.0,
                y: leaves.1,
            },
            from_pair.unwrap_or(base),
            to_pair,
            ease.clone(),
            target,
            index,
            notes,
        );
    }

    // The scalar properties, each its own track.
    for property in ["scale", "rotation", "opacity", "blur"] {
        let Some(arrival) = number_in(to, property) else {
            continue;
        };
        let base = property_base(node, property);
        let departure = from
            .and_then(Json::as_object)
            .and_then(|from| number_in(from, property))
            .unwrap_or(base);
        write_track(
            node,
            property,
            delay,
            duration,
            repeat,
            yoyo,
            span,
            Value::number,
            departure,
            arrival,
            ease.clone(),
            target,
            index,
            notes,
        );
    }
}

/// Write one property's track from the tween's window: pairs of keys per
/// cycle, the ease on each segment's start, yoyo reversing every second
/// cycle, and the cycles that outrun the node dropped.
#[allow(clippy::too_many_arguments)]
fn write_track<T: Clone>(
    node: &mut Node,
    name: &str,
    delay: f64,
    duration: f64,
    repeat: u64,
    yoyo: bool,
    span: f64,
    make: impl Fn(T) -> Value,
    departure: T,
    arrival: T,
    ease: Curve,
    target: &str,
    index: usize,
    notes: &mut Vec<String>,
) {
    let fits = (((span - delay) / duration).floor() as u64).clamp(1, repeat + 1);
    let mut keys: Vec<Key> = Vec::new();
    for cycle in 0..fits {
        let start = (delay + cycle as f64 * duration).clamp(0.0, span) / span;
        let end = (delay + (cycle as f64 + 1.0) * duration).clamp(0.0, span) / span;
        if (end - start).abs() < 1e-9 {
            continue;
        }
        let (leaves, arrives) = if yoyo && cycle % 2 == 1 {
            (arrival.clone(), departure.clone())
        } else {
            (departure.clone(), arrival.clone())
        };
        keys.push(Key {
            at: start,
            value: make(leaves),
            curve: Some(ease.clone()),
        });
        keys.push(Key {
            at: end,
            value: make(arrives),
            curve: None,
        });
    }
    if keys.len() < 2 {
        notes.push(format!(
            "skipped: tween {index} on node {target:?} has no time to move its {:?}",
            name
        ));
        return;
    }
    if node.tracks.contains_key(name) {
        notes.push(format!(
            "approximated: tween {index} on node {target:?} replaces the {:?} track the node already carried",
            name
        ));
    }
    node.tracks.insert(name.into(), Track::new(keys));
}

/// The base value a property starts from when the tween does not say: the
/// node's own static value, so `gsap.to(node, {opacity: 1})` on a node
/// that is already at 0.4 fades from 0.4 exactly as gsap would.
fn property_base(node: &Node, property: &str) -> f64 {
    match property {
        "scale" => node.transform.scale,
        "rotation" => node.transform.rotation,
        "opacity" => node.opacity,
        "blur" => node
            .effects
            .iter()
            .find_map(|effect| match effect {
                concat_motion::comp::Effect::Blur { radius } => Some(*radius),
                _ => None,
            })
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

/// A finite number a `from` or `to` object names, when it names one.
fn number_in(object: &serde_json::Map<String, Json>, property: &str) -> Option<f64> {
    object
        .get(property)
        .and_then(crate::number_or_first)
        .filter(|v| v.is_finite())
}

/// The tween's ease, by name, as the comp's curve that answers to it.
fn ease_of(tween: &Json, notes: &mut Vec<String>, index: usize, target: &str) -> Option<Curve> {
    let name = tween
        .get("ease")
        .and_then(Json::as_str)
        .unwrap_or("power1.out");
    let curve = named_ease(name);
    if name.starts_with("elastic") && curve.is_some() {
        notes.push(format!(
            "approximated: tween {index} on node {target:?} eases {name:?}; the comp reads it as the library's pop spring"
        ));
    }
    if curve.is_none() {
        notes.push(format!(
            "skipped: tween {index} on node {target:?} eases {name:?}, which the named-curve map does not carry"
        ));
    }
    curve
}

/// A named ease as the comp's curve, or `None` when the map does not carry
/// it. The numbers are the CSS-standard beziers each family publishes.
fn named_ease(name: &str) -> Option<Curve> {
    let bezier = |x1: f64, y1: f64, x2: f64, y2: f64| Some(Curve::Bezier { x1, y1, x2, y2 });
    match name {
        "none" | "linear" => Some(Curve::Linear),
        "power1.in" | "quad.in" => bezier(0.11, 0.0, 0.5, 0.0),
        "power1.out" | "quad.out" => bezier(0.5, 1.0, 0.89, 1.0),
        "power1.inOut" | "quad.inOut" => bezier(0.45, 0.0, 0.55, 1.0),
        "power2.in" | "cubic.in" => bezier(0.32, 0.0, 0.67, 0.0),
        "power2.out" | "cubic.out" => bezier(0.33, 1.0, 0.68, 1.0),
        "power2.inOut" | "cubic.inOut" => bezier(0.65, 0.0, 0.35, 1.0),
        "power3.in" | "quart.in" => bezier(0.5, 0.0, 0.75, 0.0),
        "power3.out" | "quart.out" => bezier(0.25, 1.0, 0.5, 1.0),
        "power3.inOut" | "quart.inOut" => bezier(0.76, 0.0, 0.24, 1.0),
        "power4.in" | "quint.in" => bezier(0.64, 0.0, 0.78, 0.0),
        "power4.out" | "quint.out" => bezier(0.22, 1.0, 0.36, 1.0),
        "power4.inOut" | "quint.inOut" => bezier(0.83, 0.0, 0.17, 1.0),
        "sine.in" => bezier(0.12, 0.0, 0.39, 0.0),
        "sine.out" => bezier(0.61, 1.0, 0.88, 1.0),
        "sine.inOut" => bezier(0.37, 0.0, 0.63, 1.0),
        "expo.in" => bezier(0.7, 0.0, 0.84, 0.0),
        "expo.out" => bezier(0.16, 1.0, 0.3, 1.0),
        "expo.inOut" => bezier(0.87, 0.0, 0.13, 1.0),
        "circ.in" => bezier(0.55, 0.0, 1.0, 0.45),
        "circ.out" => bezier(0.0, 0.55, 0.45, 1.0),
        "circ.inOut" => bezier(0.85, 0.0, 0.15, 1.0),
        "back.in" => bezier(0.36, 0.0, 0.66, -0.56),
        "back.out" => bezier(0.34, 1.56, 0.64, 1.0),
        "back.inOut" => bezier(0.68, -0.6, 0.32, 1.6),
        "elastic.out" => Some(Curve::SPRING_POP),
        other if other.starts_with("steps(") => other
            .trim_start_matches("steps(")
            .trim_end_matches(')')
            .parse::<u32>()
            .ok()
            .filter(|count| *count > 0)
            .map(|count| Curve::Steps {
                count,
                position: StepPosition::JumpEnd,
            }),
        _ => None,
    }
}

/// The tween's targets: one id, or an array of them.
fn targets_of(tween: &Json) -> Vec<String> {
    match tween.get("targets").or_else(|| tween.get("target")) {
        Some(Json::String(name)) => vec![name.clone()],
        Some(Json::Array(names)) => names
            .iter()
            .filter_map(Json::as_str)
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lottie;
    use concat_motion::comp::{Length, NodeKind, Paint};
    use serde_json::json;

    /// A comp with one rect node three seconds long, as the fixtures below
    /// animate it - built through the Lottie adapter so both adapters are
    /// proven against each other's output.
    fn comp() -> Comp {
        let document = json!({
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 90, "w": 512, "h": 512,
            "layers": [
                { "ind": 1, "ty": 1, "nm": "card", "ip": 0, "op": 90, "st": 0,
                  "sw": 200, "sh": 100, "sc": "#4488cc",
                  "ks": {
                      "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                      "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [100, 50, 0]},
                      "s": {"a": 0, "k": [100, 100, 100]},
                  } },
            ],
        });
        lottie::import(&document)
            .expect("the fixture converts")
            .comp
    }

    #[test]
    fn a_to_tween_writes_an_eased_track_from_the_static_value() {
        let (made, notes) = apply(
            &comp(),
            &json!([{ "targets": "card", "to": { "opacity": 1 }, "duration": 0.6 }]),
        )
        .expect("applies");
        assert!(notes.is_empty(), "{notes:?}");
        let node = &made.nodes[0];
        // The node's static opacity is 1 already, so the tween keys 1 to 1 -
        // and the test pins that the departure is the node's own value.
        let track = node.tracks.get("opacity").expect("an opacity track");
        assert_eq!(track.keys.len(), 2);
        assert_eq!(track.keys[0].at, 0.0);
        assert!((track.keys[1].at - 0.2).abs() < 1e-9, "0.6s of a 3s span");
        let Value::Number { value } = track.keys[0].value else {
            panic!("an opacity key is a number");
        };
        assert_eq!(value, 1.0);
        // The default ease is gsap's own default: power1.out.
        assert!(matches!(&track.keys[0].curve, Some(Curve::Bezier { .. })));
    }

    #[test]
    fn a_from_to_pair_keys_departure_and_arrival() {
        let (made, _) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "from": { "opacity": 0.0, "y": -24.0 },
                "to": { "opacity": 1.0, "y": 0.0 },
                "duration": 0.9,
                "ease": "power2.out",
            }]),
        )
        .expect("applies");
        let node = &made.nodes[0];
        let opacity = node.tracks.get("opacity").expect("an opacity track");
        let Value::Number { value } = opacity.keys[0].value else {
            panic!("an opacity key is a number");
        };
        assert_eq!(value, 0.0);
        let position = node.tracks.get("position").expect("a position track");
        let Value::Point { x, y } = &position.keys[0].value else {
            panic!("a position key is a point");
        };
        // The x the node already carried, unchanged; the y the tween named.
        assert_eq!((*x, *y), (256.0, -24.0));
        let Value::Point { x, y } = &position.keys[1].value else {
            panic!("a position key is a point");
        };
        assert_eq!((*x, *y), (256.0, 0.0));
    }

    #[test]
    fn x_and_y_move_together_in_one_track() {
        let (made, _) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "from": { "x": 100.0, "y": 100.0 },
                "to": { "x": 300.0, "y": 200.0 },
                "duration": 1.5,
            }]),
        )
        .expect("applies");
        let track = made.nodes[0]
            .tracks
            .get("position")
            .expect("one position track");
        assert_eq!(track.keys.len(), 2);
        let Value::Point { x, y } = &track.keys[1].value else {
            panic!("a position key is a point");
        };
        assert_eq!((*x, *y), (300.0, 200.0));
    }

    #[test]
    fn stagger_offsets_each_target() {
        let mut two = comp();
        two.nodes.push(concat_motion::comp::Node {
            id: "card-2".into(),
            parent: None,
            start: 0.0,
            duration: 3.0,
            transform: Default::default(),
            opacity: 1.0,
            blend: Default::default(),
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind: NodeKind::Ellipse {
                width: Length::Px(10.0),
                height: Length::Px(10.0),
                fill: Some(Paint::Solid {
                    color: concat_motion::comp::Rgba::new(1.0, 1.0, 1.0, 1.0),
                }),
            },
        });
        let (made, _) = apply(
            &two,
            &json!([{
                "targets": ["card", "card-2"],
                "to": { "opacity": 0.4 },
                "duration": 0.3,
                "stagger": 0.3,
            }]),
        )
        .expect("applies");
        let first = made.nodes[0].tracks.get("opacity").expect("card animated");
        let second = made.nodes[1]
            .tracks
            .get("opacity")
            .expect("card-2 animated");
        assert_eq!(first.keys[0].at, 0.0);
        assert!(
            (second.keys[0].at - 0.1).abs() < 1e-9,
            "0.3s stagger of a 3s span"
        );
    }

    #[test]
    fn repeat_and_yoyo_walk_back_and_forth() {
        let (made, _) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "to": { "rotation": 8.0 },
                "duration": 0.75,
                "repeat": 1,
                "yoyo": true,
            }]),
        )
        .expect("applies");
        let track = made.nodes[0]
            .tracks
            .get("rotation")
            .expect("a rotation track");
        // Two cycles, two keys each: out to 8, then the yoyo walks back
        // to where it began.
        assert_eq!(track.keys.len(), 4);
        let values: Vec<f64> = track
            .keys
            .iter()
            .map(|key| key.value.as_number().unwrap())
            .collect();
        assert_eq!(values, vec![0.0, 8.0, 8.0, 0.0]);
        let ats: Vec<f64> = track.keys.iter().map(|key| key.at).collect();
        assert_eq!(ats, vec![0.0, 0.25, 0.25, 0.5]);
    }

    #[test]
    fn a_forever_repeat_is_refused() {
        let (_, notes) = apply(
            &comp(),
            &json!([{ "targets": "card", "to": { "opacity": 0.0 }, "repeat": -1 }]),
        )
        .expect("applies");
        assert!(
            notes.iter().any(|note| note.contains("repeats forever")),
            "{notes:?}"
        );
    }

    #[test]
    fn a_tween_past_the_span_is_clamped_and_said() {
        let (_, notes) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "to": { "opacity": 0.0 },
                "duration": 2.0,
                "delay": 2.0,
                "repeat": 3,
            }]),
        )
        .expect("applies");
        assert!(
            notes.iter().any(|note| note.starts_with("clamped: ")),
            "{notes:?}"
        );
    }

    #[test]
    fn a_tween_outside_the_span_is_skipped() {
        let (made, notes) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "to": { "opacity": 0.0 },
                "duration": 0.5,
                "delay": 4.0,
            }]),
        )
        .expect("applies");
        assert!(made.nodes[0].tracks.is_empty());
        assert!(
            notes.iter().any(|note| note.starts_with("skipped: ")),
            "{notes:?}"
        );
    }

    #[test]
    fn an_unnamed_target_is_said() {
        let (_, notes) = apply(
            &comp(),
            &json!([{ "targets": "ghost", "to": { "opacity": 0.0 } }]),
        )
        .expect("applies");
        assert!(notes.iter().any(|note| note.contains("ghost")), "{notes:?}");
    }

    #[test]
    fn an_unmapped_ease_skips_the_whole_tween() {
        let (made, notes) = apply(
            &comp(),
            &json!([{ "targets": "card", "to": { "opacity": 0.0 }, "ease": "warp(9)" }]),
        )
        .expect("applies");
        assert!(made.nodes[0].tracks.is_empty());
        assert!(
            notes.iter().any(|note| note.contains("warp(9)")),
            "{notes:?}"
        );
    }

    #[test]
    fn an_elastic_ease_reads_as_the_pop_spring() {
        let (made, notes) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "from": { "scale": 0.6 },
                "to": { "scale": 1.0 },
                "duration": 0.5,
                "ease": "elastic.out",
            }]),
        )
        .expect("applies");
        let track = made.nodes[0].tracks.get("scale").expect("a scale track");
        assert!(matches!(&track.keys[0].curve, Some(Curve::Spring(_))));
        assert!(
            notes
                .iter()
                .any(|note| note.starts_with("approximated: ") && note.contains("elastic")),
            "{notes:?}"
        );
    }

    #[test]
    fn steps_parse_their_count() {
        let (made, _) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "to": { "opacity": 1.0 },
                "duration": 0.8,
                "ease": "steps(4)",
            }]),
        )
        .expect("applies");
        let track = made.nodes[0]
            .tracks
            .get("opacity")
            .expect("an opacity track");
        assert!(matches!(
            &track.keys[0].curve,
            Some(Curve::Steps {
                count: 4,
                position: StepPosition::JumpEnd
            })
        ));
    }

    #[test]
    fn a_tween_replaces_the_track_the_node_carried() {
        let once = json!([{ "targets": "card", "to": { "rotation": 5.0 }, "duration": 1.0 }]);
        let (first, _) = apply(&comp(), &once).expect("applies");
        let (second, notes) = apply(
            &first,
            &json!([{ "targets": "card", "to": { "rotation": 10.0 }, "duration": 1.0 }]),
        )
        .expect("applies");
        let track = second.nodes[0]
            .tracks
            .get("rotation")
            .expect("a rotation track");
        let Value::Number { value } = track.keys[1].value else {
            panic!("a rotation key is a number");
        };
        assert_eq!(value, 10.0);
        assert!(
            notes
                .iter()
                .any(|note| note.contains("replaces the \"rotation\" track")),
            "{notes:?}"
        );
    }

    #[test]
    fn the_comp_still_validates_after_the_timeline() {
        let (made, _) = apply(
            &comp(),
            &json!([{
                "targets": "card",
                "from": { "x": 0.0, "opacity": 0.0, "scale": 0.8, "rotation": -4.0 },
                "to": { "x": 256.0, "opacity": 1.0, "scale": 1.0, "rotation": 0.0 },
                "duration": 0.8,
                "ease": "power3.out",
            }]),
        )
        .expect("applies");
        assert!(made.validate().is_empty(), "{:?}", made.validate());
    }

    #[test]
    fn a_timeline_that_is_not_an_array_is_refused() {
        assert!(apply(&comp(), &json!({"targets": "card"})).is_err());
    }
}
