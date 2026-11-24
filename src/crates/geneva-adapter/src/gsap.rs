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
//! `duration` and `delay` in seconds, `stagger` between array targets (a
//! number, or an object that staggers from the center, the edges, a
//! seeded shuffle or a grid's cells), `repeat` (a count; `-1` cycles
//! until the node's span ends), `yoyo`, morphSVG onto a path's own
//! morph, drawSVG onto its trim window, and splitText as a text node's
//! own timed words, and
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

use concat_motion::comp::{Comp, Node, NodeKind, PathCommand, Point, Word};
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
        let offsets = stagger_offsets(tween, targets.len(), index, &mut notes);
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
                + offsets[place.min(offsets.len().saturating_sub(1))];
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
    // A repeat of -1 is GSAP's forever: a comp node's span is finite,
    // so forever becomes "until the span ends" - the cycles that fit,
    // and a note that says exactly that.
    let forever = tween.get("repeat").and_then(Json::as_f64) == Some(-1.0);
    let repeat = match tween.get("repeat") {
        None => 0,
        Some(value) if forever => u64::MAX,
        Some(value) => value.as_u64().unwrap_or(0),
    };
    let yoyo = tween.get("yoyo").and_then(Json::as_bool).unwrap_or(false);
    let total = delay + duration * (repeat.min(u64::MAX / 2) as f64 + 1.0);
    if delay >= span {
        notes.push(format!(
            "skipped: tween {index} on node {target:?} starts at {delay:.2}s, past the node's {span:.2}s span"
        ));
        return;
    }
    if forever {
        notes.push(format!(
            "approximated: tween {index} on node {target:?} repeats forever; it cycles until the node's {span:.2}s span ends"
        ));
    } else if total > span {
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

    // MorphSVG: the `d` the path becomes, walked 0 to 1 over the tween.
    if let Some(morph) = to.get("morphSVG") {
        let d = morph
            .as_str()
            .or_else(|| morph.get("d").and_then(Json::as_str));
        match d {
            Some(d) => match svg_path(d) {
                Some(commands) if !commands.is_empty() => {
                    if let NodeKind::Path {
                        morph: into, trim, ..
                    } = &mut node.kind
                    {
                        *into = Some(commands);
                        let _ = trim;
                        write_track(
                            node,
                            "morph",
                            delay,
                            duration,
                            repeat,
                            yoyo,
                            span,
                            Value::number,
                            0.0,
                            1.0,
                            ease.clone(),
                            target,
                            index,
                            notes,
                        );
                    } else {
                        notes.push(format!(
                            "skipped: tween {index} morphs node {target:?}, which is not a path"
                        ));
                    }
                }
                _ => notes.push(format!(
                    "skipped: tween {index} on node {target:?} carries a morphSVG path the adapter cannot read"
                )),
            },
            None => notes.push(format!(
                "skipped: tween {index} on node {target:?} carries a morphSVG with no `d`"
            )),
        }
    }

    // DrawSVG: the window of the path that is drawn, on over the tween.
    if let Some(draw) = to.get("drawSVG") {
        let window = match draw {
            Json::Bool(true) => Some((0.0, 1.0)),
            Json::String(said) => parse_window(said),
            Json::Object(fields) => {
                let start = fields
                    .get("start")
                    .and_then(Json::as_f64)
                    .or_else(|| fields.get("from").and_then(Json::as_f64));
                let end = fields
                    .get("end")
                    .and_then(Json::as_f64)
                    .or_else(|| fields.get("to").and_then(Json::as_f64));
                match (start, end) {
                    (Some(start), Some(end)) => Some((start, end)),
                    (_, Some(end)) => Some((0.0, end)),
                    _ => None,
                }
            }
            _ => None,
        };
        match window {
            Some(arrival) if arrival.1 > arrival.0 => {
                if let NodeKind::Path { trim, .. } = &mut node.kind {
                    let departure = (0.0, 0.0);
                    *trim = Some(departure);
                    write_track(
                        node,
                        "trim",
                        delay,
                        duration,
                        repeat,
                        yoyo,
                        span,
                        |pair: (f64, f64)| Value::Point {
                            x: pair.0,
                            y: pair.1,
                        },
                        departure,
                        arrival,
                        ease.clone(),
                        target,
                        index,
                        notes,
                    );
                } else {
                    notes.push(format!(
                        "skipped: tween {index} draws node {target:?}, which is not a path"
                    ));
                }
            }
            _ => notes.push(format!(
                "skipped: tween {index} on node {target:?} carries a drawSVG window the adapter cannot read"
            )),
        }
    }

    // SplitText: the words a text node's own timing carries, staggered
    // the tween says. Chars are timed per word - the comp's word clock
    // is a caption's - and said.
    if let Some(how) = tween.get("splitText").and_then(Json::as_str) {
        let each = tween
            .get("stagger")
            .and_then(Json::as_f64)
            .filter(|v| *v >= 0.0)
            .unwrap_or(0.08);
        if let NodeKind::Text(inner) = &mut node.kind {
            let words: Vec<&str> = inner.text.split_whitespace().collect();
            if words.is_empty() {
                notes.push(format!(
                    "skipped: tween {index} splits node {target:?}, whose text has no words"
                ));
            } else {
                if how == "chars" {
                    notes.push(format!(
                        "approximated: tween {index} splits node {target:?} into chars; the comp's word clock times them per word"
                    ));
                }
                inner.words = words
                    .iter()
                    .enumerate()
                    .map(|(place, word)| Word {
                        text: (*word).to_string(),
                        start: delay + each * place as f64,
                        end: None,
                        confidence: None,
                    })
                    .collect();
            }
        } else {
            notes.push(format!(
                "skipped: tween {index} splits node {target:?}, which is not a text node"
            ));
        }
    }

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
    let fits = (((span - delay) / duration).floor() as u64).clamp(1, repeat.saturating_add(1));
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

/// The per-target delays a stagger names: a plain number staggers each
/// target after the last, and an object staggers by the distances its
/// `from` says - the middle target first from `center`, the outermost
/// from `edges`, a seeded shuffle from `random`, an index from a number -
/// with a grid staggering by its cells' distance from the same answers.
/// The notes say every approximation the arithmetic makes.
fn stagger_offsets(tween: &Json, count: usize, index: usize, notes: &mut Vec<String>) -> Vec<f64> {
    let each = match tween.get("stagger") {
        Some(Json::Number(number)) => number.as_f64().unwrap_or(0.0).max(0.0),
        Some(Json::Object(object)) => object
            .get("each")
            .and_then(Json::as_f64)
            .unwrap_or(0.0)
            .max(0.0),
        _ => 0.0,
    };
    let base = |ranks: Vec<f64>| -> Vec<f64> {
        ranks
            .into_iter()
            .map(|rank| each * rank)
            .collect::<Vec<f64>>()
    };
    match tween.get("stagger") {
        // A plain number: one after the next, the default.
        None | Some(Json::Number(_)) => base((0..count).map(|i| i as f64).collect()),
        Some(Json::Object(object)) => {
            let from = object.get("from");
            let name = |said: &str| format!("skipped: tween {index} staggers from {said:?}");
            // The grid the targets sit in, when they sit in one.
            let (rows, cols) = match object.get("grid").and_then(Json::as_object) {
                Some(grid) => {
                    let cols = grid.get("cols").and_then(Json::as_u64).unwrap_or(1).max(1);
                    // A supplied row count can be stale or malformed.
                    // Every target must still occupy a cell inside the grid:
                    // otherwise "edges" subtracts an out-of-range row.
                    let required_rows = (count as u64).div_ceil(cols).max(1);
                    let rows = grid
                        .get("rows")
                        .and_then(Json::as_u64)
                        .unwrap_or(required_rows)
                        .max(required_rows);
                    (rows, cols)
                }
                None => (1, count as u64),
            };
            let cell_of = |i: usize| (i as u64 / cols, i as u64 % cols);
            let ranks: Option<Vec<f64>> = match from {
                None => Some((0..count).map(|i| i as f64).collect()),
                Some(Json::String(said)) if said == "start" || said.is_empty() => {
                    Some((0..count).map(|i| i as f64).collect())
                }
                Some(Json::String(said)) if said == "end" => {
                    Some((0..count).rev().map(|i| i as f64).collect())
                }
                Some(Json::String(said)) if said == "center" => {
                    if rows == 1 {
                        let mid = (count as f64 - 1.0) / 2.0;
                        Some((0..count).map(|i| (i as f64 - mid).abs()).collect())
                    } else {
                        let (cr, cc) = ((rows - 1) as f64 / 2.0, (cols - 1) as f64 / 2.0);
                        Some(
                            (0..count)
                                .map(|i| {
                                    let (r, c) = cell_of(i);
                                    ((r as f64 - cr).hypot(c as f64 - cc)).round()
                                })
                                .collect(),
                        )
                    }
                }
                Some(Json::String(said)) if said == "edges" => {
                    if rows == 1 {
                        Some((0..count).map(|i| (i.min(count - 1 - i)) as f64).collect())
                    } else {
                        Some(
                            (0..count)
                                .map(|i| {
                                    let (r, c) = cell_of(i);
                                    (r.min(rows - 1 - r)).min(c.min(cols - 1 - c)) as f64
                                })
                                .collect(),
                        )
                    }
                }
                Some(Json::String(said)) if said == "random" => {
                    // A seeded shuffle: the same timeline staggers the
                    // same order every time it is applied.
                    let seed = index
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        .wrapping_add(count.wrapping_mul(0x517C_C1B7_2722_0A95));
                    notes.push(format!(
                        "approximated: tween {index} staggers from random; the order is seeded, so the same timeline staggers the same way every time"
                    ));
                    let mut order: Vec<usize> = (0..count).collect();
                    let mut state = seed | 1;
                    for i in (1..order.len()).rev() {
                        state = state
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1);
                        let at = (state >> 33) % (i + 1);
                        order.swap(i, at);
                    }
                    let mut ranks = vec![0.0_f64; count];
                    for (place, target) in order.iter().enumerate() {
                        ranks[*target] = place as f64;
                    }
                    Some(ranks)
                }
                Some(Json::Number(number)) => {
                    let from = number.as_f64().unwrap_or(0.0);
                    if rows == 1 {
                        Some((0..count).map(|i| (i as f64 - from).abs()).collect())
                    } else {
                        // "from" names a target index, not an arbitrary
                        // empty cell. Clamp it to the targets we actually have;
                        // multiplying caller-supplied dimensions can overflow.
                        let from_index = (from.max(0.0) as usize).min(count - 1);
                        let (fr, fc) = cell_of(from_index);
                        Some(
                            (0..count)
                                .map(|i| {
                                    let (r, c) = cell_of(i);
                                    ((r as f64 - fr as f64).hypot(c as f64 - fc as f64)).round()
                                })
                                .collect(),
                        )
                    }
                }
                other => {
                    notes.push(name(&format!("{other:?}")));
                    Some((0..count).map(|i| i as f64).collect())
                }
            };
            if rows > 1 {
                notes.push(format!(
                    "approximated: tween {index} staggers a grid by its cells' euclidean distance, rounded to whole steps"
                ));
            }
            base(ranks.unwrap_or_else(|| (0..count).map(|i| i as f64).collect()))
        }
        Some(_) => base((0..count).map(|i| i as f64).collect()),
    }
}

/// A drawSVG window: "50%" is 0 to a half, "20% 60%" is its two ends,
/// and anything else is nothing.
fn parse_window(said: &str) -> Option<(f64, f64)> {
    let percent = |token: &str| -> Option<f64> {
        let value = token.strip_suffix('%')?.trim().parse::<f64>().ok()?;
        (value.is_finite() && (0.0..=100.0).contains(&value)).then_some(value / 100.0)
    };
    let mut tokens = said.split_whitespace();
    let first = percent(tokens.next()?)?;
    let window = match (tokens.next(), tokens.next()) {
        (None, None) => (0.0, first),
        (Some(second), None) => (first, percent(second)?),
        // Extra tokens are malformed, not an invitation to silently
        // interpret only the prefix of the user's drawSVG value.
        _ => return None,
    };
    (window.1 > window.0).then_some(window)
}

/// An SVG path's `d` string as the comp's commands: M/L/C/H/V and their
/// lowercase relatives, quadratics lifted to cubics, Z closing. Anything
/// the reader does not know stops the read, because a half-parsed path
/// is a wrong shape, not a nearly right one.
/// Reads `count` numbers off the token stream, or nothing when the
/// stream runs short or carries something that is not a number.
fn take_numbers<I: Iterator<Item = String>>(
    tokens: &mut std::iter::Peekable<I>,
    count: usize,
) -> Option<Vec<f64>> {
    let mut read = Vec::with_capacity(count);
    for _ in 0..count {
        read.push(tokens.next()?.parse::<f64>().ok()?);
    }
    Some(read)
}

/// The tokens of an SVG `d` string: one letter a command, the digits
/// between separators one number - so "M0 0" and "M 0,0" read the same,
/// and a number's exponent is not mistaken for a command.
fn tokens_of(d: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut number = String::new();
    let flush = |number: &mut String, tokens: &mut Vec<String>| {
        if !number.is_empty() {
            tokens.push(std::mem::take(number));
        }
    };
    let chars: Vec<char> = d.chars().collect();
    for (i, c) in chars.iter().copied().enumerate() {
        if c.is_whitespace() || c == ',' {
            flush(&mut number, &mut tokens);
        } else if c.is_ascii_alphabetic() {
            let exponent = (c == 'e' || c == 'E')
                && !number.is_empty()
                && chars
                    .get(i + 1)
                    .is_some_and(|next| next.is_ascii_digit() || *next == '-' || *next == '+');
            if exponent {
                number.push(c);
            } else {
                flush(&mut number, &mut tokens);
                tokens.push(c.to_string());
            }
        } else if c == '-' || c == '+' {
            flush(&mut number, &mut tokens);
            number.push(c);
        } else {
            number.push(c);
        }
    }
    flush(&mut number, &mut tokens);
    tokens
}

fn svg_path(d: &str) -> Option<Vec<PathCommand>> {
    let mut commands = Vec::new();
    let mut cursor = Point::new(0.0, 0.0);
    let mut start = Point::new(0.0, 0.0);
    let mut tokens = tokens_of(d).into_iter().peekable();
    while let Some(token) = tokens.next() {
        let relative = token.chars().all(|c| c.is_lowercase());
        let command = token.chars().last()?;
        match command {
            'M' | 'm' => {
                let read = take_numbers(&mut tokens, 2)?;
                let point = if relative {
                    Point::new(cursor.x + read[0], cursor.y + read[1])
                } else {
                    Point::new(read[0], read[1])
                };
                start = point;
                cursor = point;
                commands.push(PathCommand::MoveTo { point });
                // A run of pairs after M is line-tos, the SVG rule.
                while let Some(next) = tokens.peek().and_then(|t| t.parse::<f64>().ok()) {
                    let _ = next;
                    let read = take_numbers(&mut tokens, 2)?;
                    let point = if relative {
                        Point::new(cursor.x + read[0], cursor.y + read[1])
                    } else {
                        Point::new(read[0], read[1])
                    };
                    cursor = point;
                    commands.push(PathCommand::LineTo { point });
                }
            }
            'L' | 'l' => {
                // SVG permits multiple coordinate pairs after one command.
                while tokens.peek().and_then(|t| t.parse::<f64>().ok()).is_some() {
                    let read = take_numbers(&mut tokens, 2)?;
                    let point = if relative {
                        Point::new(cursor.x + read[0], cursor.y + read[1])
                    } else {
                        Point::new(read[0], read[1])
                    };
                    cursor = point;
                    commands.push(PathCommand::LineTo { point });
                }
            }
            'H' | 'h' => {
                while tokens.peek().and_then(|t| t.parse::<f64>().ok()).is_some() {
                    let read = take_numbers(&mut tokens, 1)?;
                    cursor = Point::new(
                        if relative {
                            cursor.x + read[0]
                        } else {
                            read[0]
                        },
                        cursor.y,
                    );
                    commands.push(PathCommand::LineTo { point: cursor });
                }
            }
            'V' | 'v' => {
                while tokens.peek().and_then(|t| t.parse::<f64>().ok()).is_some() {
                    let read = take_numbers(&mut tokens, 1)?;
                    cursor = Point::new(
                        cursor.x,
                        if relative {
                            cursor.y + read[0]
                        } else {
                            read[0]
                        },
                    );
                    commands.push(PathCommand::LineTo { point: cursor });
                }
            }
            'C' | 'c' => {
                while tokens.peek().and_then(|t| t.parse::<f64>().ok()).is_some() {
                    let read = take_numbers(&mut tokens, 6)?;
                    let (c1, c2, point) = if relative {
                        (
                            Point::new(cursor.x + read[0], cursor.y + read[1]),
                            Point::new(cursor.x + read[2], cursor.y + read[3]),
                            Point::new(cursor.x + read[4], cursor.y + read[5]),
                        )
                    } else {
                        (
                            Point::new(read[0], read[1]),
                            Point::new(read[2], read[3]),
                            Point::new(read[4], read[5]),
                        )
                    };
                    cursor = point;
                    commands.push(PathCommand::CubicTo { c1, c2, point });
                }
            }
            'Q' | 'q' => {
                while tokens.peek().and_then(|t| t.parse::<f64>().ok()).is_some() {
                    let read = take_numbers(&mut tokens, 4)?;
                    let (control, point) = if relative {
                        (
                            Point::new(cursor.x + read[0], cursor.y + read[1]),
                            Point::new(cursor.x + read[2], cursor.y + read[3]),
                        )
                    } else {
                        (Point::new(read[0], read[1]), Point::new(read[2], read[3]))
                    };
                    // The one cubic that draws the same line as this quad.
                    let c1 = Point::new(
                        cursor.x + 2.0 / 3.0 * (control.x - cursor.x),
                        cursor.y + 2.0 / 3.0 * (control.y - cursor.y),
                    );
                    let c2 = Point::new(
                        point.x + 2.0 / 3.0 * (control.x - point.x),
                        point.y + 2.0 / 3.0 * (control.y - point.y),
                    );
                    cursor = point;
                    commands.push(PathCommand::CubicTo { c1, c2, point });
                }
            }
            'Z' | 'z' => {
                cursor = start;
                commands.push(PathCommand::Close);
            }
            _ => return None,
        }
    }
    Some(commands)
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

    /// A repeat of -1 cycles until the node's span ends: the keys fill
    /// the span and the note says what forever became.
    #[test]
    fn a_forever_repeat_fills_the_span() {
        let (made, notes) = apply(
            &comp(),
            &json!([
                { "targets": "card", "to": { "opacity": 0.4 }, "duration": 0.75, "repeat": -1 }
            ]),
        )
        .expect("applies");
        let track = made.nodes[0]
            .tracks
            .get("opacity")
            .expect("the forever tween wrote its track");
        // A 3s span at 0.75s a cycle: four cycles fit.
        assert!(track.keys.len() == 8, "{} keys", track.keys.len());
        assert!((track.keys.last().unwrap().at - 1.0).abs() < 1e-9);
        assert!(
            notes.iter().any(|note| note.contains("until the node's")),
            "{notes:?}"
        );
    }

    /// A stagger object staggers by its `from`: the center target first
    /// from `center`, the outermost first from `edges`, and a seeded
    /// shuffle from `random` that shuffles the same way every time.
    #[test]
    fn stagger_objects_name_their_from() {
        let mut many = comp();
        for extra in 2..=5 {
            let mut clone = many.nodes[0].clone();
            clone.id = format!("card-{extra}");
            many.nodes.push(clone);
        }
        let centre = apply(
            &many,
            &json!([{
                "targets": ["card", "card-2", "card-3", "card-4", "card-5"],
                "to": { "opacity": 0.5 }, "duration": 0.3,
                "stagger": { "each": 0.2, "from": "center" },
            }]),
        )
        .expect("applies");
        let starts: Vec<Option<f64>> = ["card", "card-2", "card-3", "card-4", "card-5"]
            .iter()
            .map(|id| {
                centre
                    .0
                    .nodes
                    .iter()
                    .find(|node| node.id == *id)
                    .and_then(|node| node.tracks.get("opacity"))
                    .and_then(|track| track.keys.first().map(|key| key.at * 3.0))
            })
            .collect();
        let at = |id: usize| starts[id].unwrap();
        // The middle target (index 2 of 5) goes first; the ends last.
        assert!(at(2) < at(1) && at(2) < at(3), "{starts:?}");
        assert!(at(1) < at(0) && at(3) < at(4), "{starts:?}");
        assert!((at(0) - 0.4).abs() < 1e-6, "{starts:?}");
        assert!((at(2) - 0.0).abs() < 1e-6, "{starts:?}");

        let edges = apply(
            &many,
            &json!([{
                "targets": ["card", "card-2", "card-3", "card-4", "card-5"],
                "to": { "opacity": 0.5 }, "duration": 0.3,
                "stagger": { "each": 0.2, "from": "edges" },
            }]),
        )
        .expect("applies");
        let starts: Vec<Option<f64>> = ["card", "card-2", "card-3", "card-4", "card-5"]
            .iter()
            .map(|id| {
                edges
                    .0
                    .nodes
                    .iter()
                    .find(|node| node.id == *id)
                    .and_then(|node| node.tracks.get("opacity"))
                    .and_then(|track| track.keys.first().map(|key| key.at * 3.0))
            })
            .collect();
        let at = |id: usize| starts[id].unwrap();
        assert!(
            (at(0) - 0.0).abs() < 1e-6 && (at(4) - 0.0).abs() < 1e-6,
            "{starts:?}"
        );
        assert!(at(2) > at(1) && at(1) > at(0), "{starts:?}");

        // A seeded shuffle staggers the same order twice.
        let twice: Vec<Vec<Option<f64>>> = (0..2)
            .map(|_| {
                let (shuffled, notes) = apply(
                    &many,
                    &json!([{
                        "targets": ["card", "card-2", "card-3", "card-4", "card-5"],
                        "to": { "opacity": 0.5 }, "duration": 0.3,
                        "stagger": { "each": 0.2, "from": "random" },
                    }]),
                )
                .expect("applies");
                assert!(
                    notes.iter().any(|note| note.contains("seeded")),
                    "{notes:?}"
                );
                ["card", "card-2", "card-3", "card-4", "card-5"]
                    .iter()
                    .map(|id| {
                        shuffled
                            .nodes
                            .iter()
                            .find(|node| node.id == *id)
                            .and_then(|node| node.tracks.get("opacity"))
                            .and_then(|track| track.keys.first().map(|key| key.at * 3.0))
                    })
                    .collect()
            })
            .collect();
        assert_eq!(twice[0], twice[1], "the shuffle is seeded");
    }

    /// A short supplied row count must not put targets outside the grid:
    /// the edge-distance calculation still covers every target safely.
    #[test]
    fn grid_edges_expand_rows_to_cover_all_targets() {
        let mut many = comp();
        for extra in 2..=9 {
            let mut clone = many.nodes[0].clone();
            clone.id = format!("card-{extra}");
            many.nodes.push(clone);
        }
        let (made, _) = apply(
            &many,
            &json!([{
                "targets": [
                    "card", "card-2", "card-3", "card-4", "card-5",
                    "card-6", "card-7", "card-8", "card-9"
                ],
                "to": { "opacity": 0.5 },
                "duration": 0.3,
                "stagger": {
                    "each": 0.2,
                    "from": "edges",
                    "grid": { "rows": 1, "cols": 3 }
                }
            }]),
        )
        .expect("applies");
        let starts: Vec<f64> = made
            .nodes
            .iter()
            .map(|node| {
                node.tracks
                    .get("opacity")
                    .and_then(|track| track.keys.first())
                    .expect("every target is animated")
                    .at
                    * node.duration
            })
            .collect();
        assert_eq!(starts.len(), 9);
        assert!((starts[4] - 0.2).abs() < 1e-9, "{starts:?}");
        assert!(
            starts
                .iter()
                .enumerate()
                .all(|(index, start)| index == 4 || start.abs() < 1e-9),
            "{starts:?}"
        );
    }

    /// An extreme grid dimension cannot overflow while clamping a numeric
    /// origin: "from" is bounded by the actual target list.
    #[test]
    fn grid_numeric_origin_is_clamped_to_existing_targets() {
        let mut many = comp();
        for extra in 2..=3 {
            let mut clone = many.nodes[0].clone();
            clone.id = format!("card-{extra}");
            many.nodes.push(clone);
        }
        let (made, _) = apply(
            &many,
            &json!([{
                "targets": ["card", "card-2", "card-3"],
                "to": { "opacity": 0.5 },
                "duration": 0.3,
                "stagger": {
                    "each": 0.1,
                    "from": 99999,
                    "grid": { "rows": 18446744073709551615u64, "cols": 18446744073709551615u64 }
                }
            }]),
        )
        .expect("applies");
        let starts: Vec<f64> = made
            .nodes
            .iter()
            .map(|node| {
                node.tracks
                    .get("opacity")
                    .and_then(|track| track.keys.first())
                    .expect("every target is animated")
                    .at
                    * node.duration
            })
            .collect();
        assert!((starts[0] - 0.2).abs() < 1e-9, "{starts:?}");
        assert!((starts[1] - 0.1).abs() < 1e-9, "{starts:?}");
        assert!(starts[2].abs() < 1e-9, "{starts:?}");
    }

    /// morphSVG walks a path's own morph and drawSVG its trim window.
    #[test]
    fn morph_and_draw_walk_the_path() {
        let document = json!({
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 90, "w": 512, "h": 512,
            "layers": [
                { "ind": 1, "ty": 4, "nm": "mark", "ip": 0, "op": 90, "st": 0,
                  "ks": {
                      "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                      "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                      "s": {"a": 0, "k": [100, 100, 100]},
                  },
                  "shapes": [
                    { "ty": "gr", "it": [
                        { "ty": "sh", "ks": {"a": 0, "k": {"c": false,
                            "v": [[0,0],[100,0]], "i": [[0,0],[0,0]], "o": [[0,0],[0,0]]}} },
                        { "ty": "st", "c": {"a": 0, "k": [1, 0, 0, 1]}, "o": {"a": 0, "k": 100},
                          "w": {"a": 0, "k": 4} },
                        { "ty": "tr", "p": {"a": 0, "k": [0, 0]}, "a": {"a": 0, "k": [0, 0]},
                          "s": {"a": 0, "k": [100, 100]}, "r": {"a": 0, "k": 0}, "o": {"a": 0, "k": 100} },
                    ] },
                  ],
                },
            ],
        });
        let comp = lottie::import(&document)
            .expect("the fixture converts")
            .comp;
        let path_id = comp
            .nodes
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Path { .. }))
            .expect("the fixture has a path")
            .id
            .clone();
        let morphed = apply(
            &comp,
            &json!([{
                "targets": path_id, "duration": 1.0,
                "to": { "morphSVG": "M 0 40 L 100 40" },
            }]),
        )
        .expect("applies");
        let node = morphed
            .0
            .nodes
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Path { .. }))
            .expect("the path node");
        let NodeKind::Path { morph, .. } = &node.kind else {
            unreachable!();
        };
        let target = morph.as_ref().expect("the morph target wrote");
        assert_eq!(target.len(), 2, "{target:?}");
        let track = node.tracks.get("morph").expect("the morph track");
        assert_eq!(track.keys.len(), 2);
        let Value::Number { value: last } = &track.keys[1].value else {
            panic!("a number");
        };
        assert!((last - 1.0).abs() < 1e-9);

        let drawn = apply(
            &comp,
            &json!([{
                "targets": path_id, "duration": 1.0,
                "to": { "drawSVG": "0% 60%" },
            }]),
        )
        .expect("applies");
        let node = drawn
            .0
            .nodes
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Path { .. }))
            .expect("the path node");
        let NodeKind::Path { trim, .. } = &node.kind else {
            unreachable!();
        };
        assert_eq!(*trim, Some((0.0, 0.0)));
        let track = node.tracks.get("trim").expect("the trim track");
        let Value::Point { x, y } = &track.keys[1].value else {
            panic!("a point");
        };
        assert!(
            (*x - 0.0).abs() < 1e-9 && (*y - 0.6).abs() < 1e-9,
            "{x} {y}"
        );
    }

    /// splitText times a text node's own words, staggered.
    #[test]
    fn split_text_times_the_words() {
        let document = json!({
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 90, "w": 512, "h": 512,
            "layers": [
                { "ind": 1, "ty": 5, "nm": "title", "ip": 0, "op": 90, "st": 0,
                  "ks": {
                      "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                      "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                      "s": {"a": 0, "k": [100, 100, 100]},
                  },
                  "t": { "d": { "k": [{ "s": { "s": 72, "f": "Archivo", "t": "one two three" }, "t": 0 }] },
                         "p": {}, "m": "g", "a": [] },
                },
            ],
        });
        let comp = lottie::import(&document)
            .expect("the fixture converts")
            .comp;
        let (made, _) = apply(
            &comp,
            &json!([{
                "targets": "title", "duration": 0.6, "delay": 0.2,
                "splitText": "words", "stagger": 0.25,
                "to": { "opacity": 1.0 },
            }]),
        )
        .expect("applies");
        let node = made
            .nodes
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Text(_)))
            .expect("the text node");
        let NodeKind::Text(inner) = &node.kind else {
            unreachable!();
        };
        assert_eq!(inner.words.len(), 3, "{:?}", inner.words);
        assert_eq!(inner.words[0].text, "one");
        assert!(
            (inner.words[0].start - 0.2).abs() < 1e-9
                && (inner.words[1].start - 0.45).abs() < 1e-9
                && (inner.words[2].start - 0.7).abs() < 1e-9,
            "{:?}",
            inner.words
        );
    }

    /// The SVG `d` reader: lines and cubics, relatives and quadratics,
    /// and a refusal of anything it does not know.
    #[test]
    fn draw_svg_windows_reject_malformed_or_out_of_range_values() {
        assert_eq!(parse_window("50%"), Some((0.0, 0.5)));
        assert_eq!(parse_window("20% 60%"), Some((0.2, 0.6)));
        assert_eq!(parse_window("0% 100%"), Some((0.0, 1.0)));
        assert_eq!(parse_window("20% 60% 80%"), None);
        assert_eq!(parse_window("20% garbage"), None);
        assert_eq!(parse_window("-1% 60%"), None);
        assert_eq!(parse_window("20% 101%"), None);
        assert_eq!(parse_window("60% 20%"), None);
        assert_eq!(parse_window("NaN%"), None);
        assert_eq!(parse_window("inf%"), None);
    }

    #[test]
    fn the_svg_d_reader_reads() {
        let commands = svg_path("M 10 10 L 90 10 L 90 90 Z").expect("reads");
        assert_eq!(commands.len(), 4, "{commands:?}");
        let commands = svg_path("m 10 10 l 80 0 l 0 80 z").expect("reads");
        assert_eq!(commands.len(), 4, "{commands:?}");
        let commands = svg_path("M0 0 Q 50 100 100 0").expect("reads");
        assert!(
            matches!(commands[1], PathCommand::CubicTo { .. }),
            "{commands:?}"
        );
        let commands = svg_path("M0 0 H 50 V 50").expect("reads");
        assert_eq!(commands.len(), 3, "{commands:?}");
        // SVG allows repeated parameter groups without repeating the command.
        let commands = svg_path("M0 0 L10 10 20 20 H30 40 V50 60").expect("reads");
        assert_eq!(commands.len(), 7, "{commands:?}");
        let commands = svg_path("M0 0 C1 2 3 4 5 6 7 8 9 10 11 12").expect("reads");
        assert_eq!(commands.len(), 3, "{commands:?}");
        let commands = svg_path("M0 0 Q10 20 20 0 30 -20 40 0").expect("reads");
        assert_eq!(commands.len(), 3, "{commands:?}");
        let commands = svg_path("M0 0 l10 0 10 0 h10 10 v10 10").expect("reads");
        assert_eq!(commands.len(), 7, "{commands:?}");
        assert!(
            svg_path("M0 0 A 50 50 0 0 1 100 0").is_none(),
            "arcs are refused"
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
