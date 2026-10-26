// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Authoring: the operations an agent applies to a comp.
//!
//! A language model should not be asked to emit a whole scene graph - it gets
//! a field name wrong, drops a node and the picture goes blank. It *can*
//! reliably say "add a lower third at 3.5 s, set the name, slide it in" - so
//! this module is the vocabulary for that: small, validated operations that
//! each do one thing and either succeed or explain why not.
//!
//! Every operation runs through [`Comp::validate`] when asked to, and the
//! result is a new document rather than a mutation, so a failed batch leaves
//! the comp exactly as it was. The pair - `apply` then `validate` - is what the
//! MCP tool `motion.edit` exposes.
//!
//! ```
//! use concat_motion::authoring::{apply, Op};
//! use concat_motion::presets;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let comp = presets::build("funcut.lower-third.glass").expect("a preset");
//! let edited = apply(&comp, &[
//!     Op::SetSlot { slot: "Name".into(), value: "Grace Hopper".into() },
//!     Op::Move { node: "card".into(), start: 3.5 },
//! ])?;
//! assert_eq!(edited.duration, comp.duration);
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::comp::{
    Blend, Comp, Length, Node, NodeKind, Paint, Point, Rgba, Slot, SlotField, Stroke, TextAlign,
    TextNode, TextStyle, Transform, Word,
};
use crate::curve::Curve;
use crate::engine::MotionError;
use crate::track::{Key, Track, Value};

/// One authoring operation, as JSON: `{"op": "move", "node": "card", "start": 3.5}`.
///
/// The same vocabulary is what an MCP tool, the editor's scripting pane and a
/// saved macro all speak, so there is exactly one way to say "slide it in".
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Op {
    /// Fill a template slot with a value, found by its label or its node id.
    SetSlot {
        /// The slot's label, or the node it sits on.
        slot: String,
        /// What to put there. Text for a text slot, an asset id for an image.
        value: String,
    },
    /// Replace a text node's words, clearing any word timing with them.
    SetText {
        /// The node.
        node: String,
        /// The words.
        text: String,
    },
    /// Give a text node word timing, which is what makes captions karaoke.
    SetWords {
        /// The node.
        node: String,
        /// `(word, start, end)` in seconds; `None` holds until the next word.
        words: Vec<(String, f64, Option<f64>)>,
    },
    /// When a node starts, and how long it runs (`0` runs to the end).
    Move {
        /// The node.
        node: String,
        /// Seconds from the comp's start.
        start: f64,
    },
    /// Change how long a node runs; `0` runs to the end of the comp.
    Retime {
        /// The node.
        node: String,
        /// Seconds.
        duration: f64,
    },
    /// Add or replace a track on a node.
    ///
    /// The track is written as `(at, value, curve)` triples, `at` a fraction
    /// of the node's span - the shape a model can produce from a sentence
    /// like "slide in over the first half second and settle".
    SetTrack {
        /// The node.
        node: String,
        /// `position`, `scale`, `rotation`, `opacity`, `blur`, `dash`, or
        /// `param:<key>`.
        property: String,
        /// The keys.
        keys: Vec<(f64, f64, Option<Curve>)>,
    },
    /// Fade a node in over the first `seconds` of its span.
    FadeIn {
        /// The node.
        node: String,
        /// How long the fade takes.
        seconds: f64,
        /// How the opacity travels.
        curve: Curve,
    },
    /// Slide a node to a place over `seconds`, settled with a spring.
    SlideTo {
        /// The node.
        node: String,
        /// The destination, in frame pixels (the node's centre).
        x: f64,
        /// The destination.
        y: f64,
        /// How long it takes.
        seconds: f64,
    },
    /// Pop a node in: scale from `from` to 1 over `seconds`.
    Pop {
        /// The node.
        node: String,
        /// The scale it starts at.
        from: f64,
        /// How long it takes.
        seconds: f64,
    },
    /// Add an empty group, which children can then hang off.
    AddGroup {
        /// Its id.
        id: String,
        /// Where it sits, in frame pixels.
        x: f64,
        /// Where it sits.
        y: f64,
    },
    /// Add a rectangle, filled or outlined.
    AddRect {
        /// Its id.
        id: String,
        /// Width in pixels.
        width: f64,
        /// Height in pixels.
        height: f64,
        /// Fill colour as `#rrggbb` or `#rrggbbaa`; empty for none.
        fill: String,
        /// Corner radius.
        radius: f64,
        /// Where its centre sits.
        x: f64,
        /// Where its centre sits.
        y: f64,
    },
    /// Add a text node with plain words.
    AddText {
        /// Its id.
        id: String,
        /// The words.
        text: String,
        /// Size in pixels.
        size: f64,
        /// Weight, 100 to 900.
        weight: u32,
        /// Colour as hex.
        color: String,
        /// Where its centre sits.
        x: f64,
        /// Where its centre sits.
        y: f64,
    },
    /// Delete a node and every node whose parent it was.
    Delete {
        /// The node.
        node: String,
    },
    /// Set a node's blend mode.
    SetBlend {
        /// The node.
        node: String,
        /// The mode.
        blend: Blend,
    },
    /// Set a node's opacity, before any `opacity` track.
    SetOpacity {
        /// The node.
        node: String,
        /// 0 to 1.
        opacity: f64,
    },
    /// Set the comp's own length.
    SetDuration {
        /// Seconds.
        seconds: f64,
    },
    /// Set the comp's clear colour: `#00000000` is transparent.
    SetBackground {
        /// Colour as hex.
        color: String,
    },
}

/// Apply a batch of operations, all or nothing.
///
/// The comp is validated as the batch is built: the first operation that would
/// leave an invalid document stops the batch and its index is in the message,
/// so an agent can retry just that step instead of re-planning.
pub fn apply(comp: &Comp, ops: &[Op]) -> Result<Comp, MotionError> {
    let mut next = comp.clone();
    for (index, op) in ops.iter().enumerate() {
        next = one(next, op).map_err(|error| match error {
            MotionError::Engine { code, message, help } => MotionError::Engine {
                code,
                message: format!("operation {index}: {message}"),
                help,
            },
            other => other,
        })?;
    }
    let problems = next.validate();
    if !problems.is_empty() {
        return Err(MotionError::Invalid { diagnostics: problems });
    }
    Ok(next)
}

/// Apply a JSON batch: `{"ops": [...]}` or a bare array of operations.
///
/// The shape an agent sends. A malformed batch is refused before anything is
/// applied, so a bad op index is the only thing the caller has to fix.
pub fn apply_json(comp: &Comp, batch: &serde_json::Value) -> Result<Comp, MotionError> {
    let list = match batch.get("ops") {
        Some(list) => list.clone(),
        None => batch.clone(),
    };
    let ops: Vec<Op> = serde_json::from_value(list).map_err(|error| fail(
        format!("the batch is not a list of motion operations: {error}"),
        "each entry is {\"op\": \"set-slot\" | \"move\" | \"fade-in\" | ...}; see the motion documentation",
    ))?;
    apply(comp, &ops)
}

/// Apply one operation.
pub fn one(mut comp: Comp, op: &Op) -> Result<Comp, MotionError> {
    match op {
        Op::SetSlot { slot, value } => {
            let target = comp
                .slots
                .iter()
                .find(|candidate| {
                    candidate.label.eq_ignore_ascii_case(slot) || candidate.node.eq_ignore_ascii_case(slot)
                })
                .map(|candidate| (candidate.node.clone(), candidate.field))
                .ok_or_else(|| fail(format!("no slot called {slot:?}"), "the comp lists its slots under `slots`"))?;
            let sample = comp
                .slots
                .iter()
                .find(|candidate| candidate.node == target.0)
                .map(|candidate| candidate.sample.clone())
                .unwrap_or_default();
            match target.1 {
                SlotField::Text => {
                    set_text_in(&mut comp, &target.0, &sample, value)?;
                    Some(())
                }
                SlotField::Image => {
                    let node = node_mut(&mut comp, &target.0)?;
                    if let NodeKind::Image { asset, .. } = &mut node.kind {
                        *asset = value.clone();
                    }
                    Some(())
                }
            };
            Ok(comp)
        }
        Op::SetText { node, text } => {
            set_text(&mut comp, node, text)?;
            Ok(comp)
        }
        Op::SetWords { node, words } => {
            let span = comp.duration;
            let entry = node_mut(&mut comp, node)?;
            if let NodeKind::Text(text) = &mut entry.kind {
                text.words = words
                    .iter()
                    .map(|(word, start, end)| Word {
                        text: word.clone(),
                        start: *start,
                        end: *end,
                    })
                    .collect();
                // An open-ended last word is the author's way of saying "hold
                // it"; the format wants an end, so it is closed here.
                if let Some(last) = text.words.last_mut()
                    && last.end.is_none()
                {
                    last.end = Some(span.max(last.start + 0.1));
                }
                text.text.clear();
            } else {
                return Err(not_text(node));
            }
            Ok(comp)
        }
        Op::Move { node, start } => {
            let entry = node_mut(&mut comp, node)?;
            entry.start = start.max(0.0);
            Ok(comp)
        }
        Op::Retime { node, duration } => {
            let entry = node_mut(&mut comp, node)?;
            entry.duration = duration.max(0.0);
            Ok(comp)
        }
        Op::SetTrack { node, property, keys } => {
            if !documented_property(property) {
                return Err(fail(
                    format!("{property:?} is not a property this format animates"),
                    "use position, scale, rotation, opacity, blur, dash, or param:<key> for an effect parameter",
                ));
            }
            let mut track = Track::default();
            for (at, value, curve) in keys {
                let value = if property == "position" {
                    Value::Point { x: *value, y: *value }
                } else {
                    Value::Number { value: *value }
                };
                track.keys.push(match curve {
                    Some(curve) => Key::eased(*at, value, curve.clone()),
                    None => Key::new(*at, value),
                });
            }
            track.keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert(property.clone(), track);
            Ok(comp)
        }
        Op::FadeIn { node, seconds, curve } => {
            let span = span_of(&comp, node)?;
            let at = (seconds / span).clamp(0.001, 1.0);
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert(
                "opacity".into(),
                Track::new(vec![
                    Key::eased(0.0, Value::number(0.0), curve.clone()),
                    Key::new(at, Value::number(1.0)),
                ]),
            );
            Ok(comp)
        }
        Op::SlideTo { node, x, y, seconds } => {
            let span = span_of(&comp, node)?;
            let at = (seconds / span).clamp(0.001, 1.0);
            let entry = node_mut(&mut comp, node)?;
            let from = entry.transform.position.unwrap_or(Point::new(0.0, 0.0));
            entry.tracks.insert(
                "position".into(),
                Track::new(vec![
                    Key::eased(0.0, Value::Point { x: from.x, y: from.y }, Curve::SPRING),
                    Key::new(at, Value::Point { x: *x, y: *y }),
                    Key::new(1.0, Value::Point { x: *x, y: *y }),
                ]),
            );
            entry.transform.position = Some(Point::new(*x, *y));
            Ok(comp)
        }
        Op::Pop { node, from, seconds } => {
            let span = span_of(&comp, node)?;
            let at = (seconds / span).clamp(0.001, 1.0);
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert(
                "scale".into(),
                Track::new(vec![
                    Key::eased(0.0, Value::number(*from), Curve::SPRING_POP),
                    Key::new(at, Value::number(1.0)),
                ]),
            );
            Ok(comp)
        }
        Op::AddGroup { id, x, y } => {
            push(&mut comp, id)?;
            comp.nodes.push(Node {
                id: id.clone(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform { position: Some(Point::new(*x, *y)), ..Transform::default() },
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                effects: Vec::new(),
                tracks: BTreeMap::new(),
                kind: NodeKind::Group { children: Vec::new() },
            });
            Ok(comp)
        }
        Op::AddRect { id, width, height, fill, radius, x, y } => {
            push(&mut comp, id)?;
            comp.nodes.push(Node {
                id: id.clone(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform { position: Some(Point::new(*x, *y)), ..Transform::default() },
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                effects: Vec::new(),
                tracks: BTreeMap::new(),
                kind: NodeKind::Rect {
                    width: Length::Px(*width),
                    height: Length::Px(*height),
                    fill: (!fill.is_empty()).then(|| Paint::Solid { color: Rgba::hex(fill) }),
                    radius: *radius,
                },
            });
            Ok(comp)
        }
        Op::AddText { id, text, size, weight, color, x, y } => {
            push(&mut comp, id)?;
            comp.nodes.push(Node {
                id: id.clone(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform { position: Some(Point::new(*x, *y)), ..Transform::default() },
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                effects: Vec::new(),
                tracks: BTreeMap::new(),
                kind: NodeKind::Text(Box::new(TextNode {
                    text: text.clone(),
                    words: Vec::new(),
                    highlight: None,
                    font: "Inter".into(),
                    size: *size,
                    weight: *weight,
                    italic: false,
                    letter_spacing: 0.0,
                    line_height: 1.2,
                    align: TextAlign::Center,
                    max_width: None,
                    padding: 0.0,
                    background: Rgba::new(0.0, 0.0, 0.0, 0.0),
                    radius: 0.0,
                    color: Rgba::hex(color),
                    fill: None,
                    outline: None,
                    shadow: None,
                })),
            });
            Ok(comp)
        }
        Op::Delete { node } => {
            let mut doomed = vec![node.clone()];
            let mut index = 0;
            // Everything downstream of a deleted node goes with it: a child
            // whose parent is gone would inherit a transform from nothing.
            while index < doomed.len() {
                let parent = doomed[index].clone();
                for candidate in &comp.nodes {
                    if candidate.parent.as_deref() == Some(parent.as_str()) {
                        doomed.push(candidate.id.clone());
                    }
                }
                index += 1;
            }
            comp.nodes.retain(|entry| !doomed.contains(&entry.id));
            comp.slots.retain(|slot| !doomed.contains(&slot.node));
            Ok(comp)
        }
        Op::SetBlend { node, blend } => {
            node_mut(&mut comp, node)?.blend = *blend;
            Ok(comp)
        }
        Op::SetOpacity { node, opacity } => {
            node_mut(&mut comp, node)?.opacity = opacity.clamp(0.0, 1.0);
            Ok(comp)
        }
        Op::SetDuration { seconds } => {
            comp.duration = seconds.max(0.1);
            Ok(comp)
        }
        Op::SetBackground { color } => {
            comp.background = Rgba::hex(color);
            Ok(comp)
        }
    }
}

/// Turn words into a [`TextNode`]'s text, keeping its style.
fn set_text(comp: &mut Comp, id: &str, text: &str) -> Result<(), MotionError> {
    set_text_in(comp, id, "", text)
}

/// The same, but in a markup node it replaces the run that reads `sample`.
///
/// A slot's sample is the words the preset drew there, so it is also the
/// needle: "Ada Lovelace" finds the name and not the timecode beside it.
fn set_text_in(comp: &mut Comp, id: &str, sample: &str, text: &str) -> Result<(), MotionError> {
    let entry = node_mut(comp, id)?;
    match &mut entry.kind {
        NodeKind::Text(node) => {
            node.text = text.to_string();
            node.words.clear();
            Ok(())
        }
        NodeKind::Html { markup, .. } => {
            // A markup comp's words live in its element tree; the slot's
            // sample says which run is the one to change.
            *markup = replace_text_run(markup, sample, text);
            Ok(())
        }
        _ => Err(MotionError::Unsupported {
            node: id.into(),
            what: "text".into(),
            engine: crate::engine::EngineKind::Native,
        }),
    }
}

/// Replace a text run in a markup string: the one reading `sample`, or the
/// last one when there is no such run.
///
/// Not a parser: a comp's markup is the preset's own, and the edit an agent
/// asks for ("make the name Grace Hopper") is always a whole run. Matching by
/// the run's own words is what keeps a name from landing on the timecode next
/// to it. A document with no text run is left alone.
pub fn replace_text_run(markup: &str, sample: &str, value: &str) -> String {
    let sample = sample.trim();
    let mut runs: Vec<(usize, usize, &str)> = Vec::new();
    let bytes = markup.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'>' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'<' {
                end += 1;
            }
            let run = markup[start..end].trim();
            if !run.is_empty() && !run.contains('{') {
                runs.push((start, end, run));
            }
            i = end;
            continue;
        }
        i += 1;
    }
    let chosen = runs
        .iter()
        .find(|(_, _, run)| !sample.is_empty() && *run == sample)
        .or_else(|| runs.last());
    match chosen {
        Some((start, end, _)) => {
            let mut out = String::with_capacity(markup.len());
            out.push_str(&markup[..*start]);
            out.push_str(value);
            out.push_str(&markup[*end..]);
            out
        }
        None => markup.to_string(),
    }
}

/// Kept for callers that only want *some* run replaced; see
/// [`replace_text_run`] for the one that respects a slot's sample.
#[doc(hidden)]
pub fn replace_last_text_run(markup: &str, value: &str) -> String {
    replace_text_run(markup, "", value)
}

/// A property name the format animates.
fn documented_property(name: &str) -> bool {
    matches!(name, "position" | "scale" | "rotation" | "opacity" | "blur" | "dash")
        || name.starts_with("param:")
}

fn node_mut<'a>(comp: &'a mut Comp, id: &str) -> Result<&'a mut Node, MotionError> {
    let index = comp
        .nodes
        .iter()
        .position(|node| node.id == id)
        .ok_or_else(|| fail(format!("no node called {id:?}"), "list the comp first: motion.get returns its nodes"))?;
    Ok(&mut comp.nodes[index])
}

fn span_of(comp: &Comp, id: &str) -> Result<f64, MotionError> {
    comp.node(id)
        .map(|node| {
            let end = node.end(comp.duration);
            (end - node.start).max(0.1)
        })
        .ok_or_else(|| fail(format!("no node called {id:?}"), "list the comp first: motion.get returns its nodes"))
}

fn push(comp: &mut Comp, id: &str) -> Result<(), MotionError> {
    if comp.node(id).is_some() {
        return Err(fail(
            format!("a node called {id:?} already exists"),
            "ids are what tracks and slots point at, so a new node needs a new id",
        ));
    }
    Ok(())
}

fn not_text(node: &str) -> MotionError {
    fail(format!("{node:?} is not a text node"), "only a text node carries words")
}

fn fail(message: String, help: impl Into<String>) -> MotionError {
    MotionError::Engine { code: None, message, help: Some(help.into()) }
}

/// A description of what a node is, for a tool result a model can read back.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct NodeSummary {
    /// Its id.
    pub id: String,
    /// Its kind, as a word: `group`, `rect`, `ellipse`, `path`, `text`,
    /// `html`, `image`, `lottie`, `composition`.
    pub kind: String,
    /// When it starts.
    pub start: f64,
    /// How long it runs; `0` means to the end of the comp.
    pub duration: f64,
    /// The properties it animates.
    pub tracks: Vec<String>,
    /// Its first words, for a text or markup node.
    pub text: Option<String>,
}

/// Summarise a comp for a model: what is in it, what moves, what can be filled.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CompSummary {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Length in seconds.
    pub duration: f64,
    /// The nodes, in paint order.
    pub nodes: Vec<NodeSummary>,
    /// The slots a project can fill: `(label, node, field)`.
    pub slots: Vec<(String, String, String)>,
    /// Whether a markup engine is needed for anything in it.
    pub needs_markup: bool,
}

impl CompSummary {
    /// Summarise `comp`.
    pub fn of(comp: &Comp) -> CompSummary {
        CompSummary {
            id: comp.id.clone(),
            name: comp.name.clone(),
            width: comp.width,
            height: comp.height,
            duration: comp.duration,
            nodes: comp
                .paint_order()
                .into_iter()
                .map(|node| {
                    let (kind, text) = match &node.kind {
                        NodeKind::Group { .. } => ("group", None),
                        NodeKind::Rect { .. } => ("rect", None),
                        NodeKind::Ellipse { .. } => ("ellipse", None),
                        NodeKind::Path { .. } => ("path", None),
                        NodeKind::Text(text) => ("text", Some(first_words(text))),
                        NodeKind::Html { markup, .. } => ("html", Some(markup.lines().next().unwrap_or_default().trim().to_string())),
                        NodeKind::Image { .. } => ("image", None),
                        NodeKind::Lottie { .. } => ("lottie", None),
                        NodeKind::Composition { .. } => ("composition", None),
                    };
                    NodeSummary {
                        id: node.id.clone(),
                        kind: kind.into(),
                        start: node.start,
                        duration: node.duration,
                        tracks: node.tracks.keys().cloned().collect(),
                        text,
                    }
                })
                .collect(),
            slots: comp
                .slots
                .iter()
                .map(|slot: &Slot| {
                    (
                        slot.label.clone(),
                        slot.node.clone(),
                        match slot.field {
                            SlotField::Text => "text".to_string(),
                            SlotField::Image => "image".to_string(),
                        },
                    )
                })
                .collect(),
            needs_markup: comp.needs_markup(),
        }
    }
}

fn first_words(text: &TextNode) -> String {
    if text.words.is_empty() {
        text.text.chars().take(60).collect()
    } else {
        text.words
            .iter()
            .take(6)
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A highlight style for a word: the caption look, in one call.
pub fn karaoke_highlight(color: &str, weight: u32) -> TextStyle {
    TextStyle { color: Some(Rgba::hex(color)), fill: None, weight: Some(weight), scale: None }
}

/// A stroke of `width` pixels in `color`.
pub fn stroke(color: &str, width: f64) -> Stroke {
    Stroke { color: Rgba::hex(color), width }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets;

    fn lower_third() -> Comp {
        presets::build("funcut.lower-third.glass").expect("a preset")
    }

    #[test]
    fn a_batch_applies_in_order_and_validates() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[
                Op::SetSlot { slot: "Name".into(), value: "Grace Hopper".into() },
                Op::Move { node: "card".into(), start: 3.5 },
                Op::Retime { node: "card".into(), duration: 2.5 },
            ],
        )
        .expect("the batch applies");
        let card = edited.node("card").expect("the card");
        assert_eq!(card.start, 3.5);
        assert_eq!(card.duration, 2.5);
        assert!(edited.validate().is_empty());
    }

    #[test]
    fn filling_a_slot_by_label_rewrites_the_markup_words() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[Op::SetSlot { slot: "Name".into(), value: "Grace Hopper".into() }],
        )
        .expect("applies");
        let card = edited.node("card").expect("the card");
        match &card.kind {
            NodeKind::Html { markup, .. } => {
                assert!(markup.contains("Grace Hopper"), "{markup}");
                assert!(!markup.contains("Ada Lovelace"), "{markup}");
            }
            other => panic!("expected markup, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_node_names_itself_in_the_error() {
        let comp = lower_third();
        let error = apply(&comp, &[Op::Move { node: "nope".into(), start: 1.0 }]).expect_err("refused");
        assert!(error.to_string().contains("nope"), "{error}");
    }

    #[test]
    fn an_unknown_slot_is_refused_rather_than_silently_dropped() {
        let comp = lower_third();
        let error = apply(&comp, &[Op::SetSlot { slot: "Role".into(), value: "x".into() }]).expect_err("refused");
        assert!(error.to_string().contains("Role"), "{error}");
    }

    #[test]
    fn a_fade_becomes_an_opacity_track_relative_to_the_span() {
        let comp = lower_third();
        let edited = apply(&comp, &[Op::FadeIn { node: "card".into(), seconds: 1.5, curve: Curve::EASE_OUT }])
            .expect("applies");
        let card = edited.node("card").expect("the card");
        let track = card.tracks.get("opacity").expect("an opacity track");
        assert_eq!(track.sample(1.0).unwrap().as_number(), Some(1.0));
        let at = track.keys[0].at;
        assert!((at - 0.0).abs() < 1e-9);
    }

    #[test]
    fn slide_to_ends_where_it_was_told() {
        let comp = lower_third();
        let edited = apply(&comp, &[Op::SlideTo { node: "card".into(), x: 1920.0, y: 400.0, seconds: 0.8 }])
            .expect("applies");
        let card = edited.node("card").expect("the card");
        assert_eq!(card.transform.position, Some(Point::new(1920.0, 400.0)));
        assert_eq!(
            card.tracks["position"].sample(1.0).unwrap(),
            Value::Point { x: 1920.0, y: 400.0 }
        );
    }

    #[test]
    fn deleting_a_parent_takes_its_children() {
        let comp = lower_third();
        let mut with_children = apply(
            &comp,
            &[
                Op::AddGroup { id: "kid".into(), x: 10.0, y: 10.0 },
                Op::AddGroup { id: "other".into(), x: 20.0, y: 20.0 },
            ],
        )
        .expect("applies");
        let index = with_children.nodes.iter().position(|node| node.id == "kid").expect("the group");
        with_children.nodes[index].parent = Some("card".into());
        let pruned = apply(&with_children, &[Op::Delete { node: "card".into() }]).expect("applies");
        assert!(pruned.node("card").is_none());
        assert!(pruned.node("kid").is_none(), "the child went with its parent");
        assert!(pruned.node("other").is_some(), "an unrelated node stays");
    }

    #[test]
    fn a_new_node_with_a_taken_id_is_refused() {
        let comp = lower_third();
        let error = apply(&comp, &[Op::AddRect {
            id: "card".into(),
            width: 10.0,
            height: 10.0,
            fill: "#fff".into(),
            radius: 0.0,
            x: 0.0,
            y: 0.0,
        }])
        .expect_err("refused");
        assert!(error.to_string().contains("already exists"), "{error}");
    }

    #[test]
    fn an_undocumented_property_is_refused_with_the_list() {
        let comp = lower_third();
        let error = apply(
            &comp,
            &[Op::SetTrack { node: "card".into(), property: "colour".into(), keys: vec![(0.0, 1.0, None)] }],
        )
        .expect_err("refused");
        assert!(error.to_string().contains("position"), "{error}");
    }

    #[test]
    fn words_replace_a_text_nodes_content_and_close_the_last_cue() {
        let comp = presets::build("funcut.caption.kinetic").expect("a preset");
        let edited = apply(
            &comp,
            &[Op::SetWords {
                node: "caption".into(),
                words: vec![("hello".into(), 0.0, None), ("world".into(), 0.6, None)],
            }],
        )
        .expect("applies");
        let node = edited.node("caption").expect("the caption");
        match &node.kind {
            NodeKind::Text(text) => {
                assert_eq!(text.words.len(), 2);
                assert_eq!(text.words[1].end, Some(edited.duration), "the last cue is closed");
                assert!(text.text.is_empty());
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn the_summary_says_what_a_model_needs_to_know() {
        let summary = CompSummary::of(&lower_third());
        assert_eq!(summary.width, 1920);
        assert!(summary.needs_markup);
        assert_eq!(summary.nodes.len(), 1);
        assert_eq!(summary.nodes[0].kind, "html");
        assert!(summary.nodes[0].tracks.contains(&"position".to_string()));
        assert_eq!(summary.slots[0].0, "Name");
    }

    #[test]
    fn an_operation_batch_round_trips_through_json() {
        let batch = serde_json::json!({
            "ops": [
                { "op": "set-slot", "slot": "Name", "value": "Grace Hopper" },
                { "op": "move", "node": "card", "start": 2.0 },
                { "op": "fade-in", "node": "card", "seconds": 0.8,
                  "curve": { "kind": "bezier", "x1": 0.16, "y1": 1.0, "x2": 0.3, "y2": 1.0 } }
            ]
        });
        let edited = apply_json(&lower_third(), &batch).expect("the batch applies");
        let card = edited.node("card").expect("the card");
        assert_eq!(card.start, 2.0);
        assert!(card.tracks.contains_key("opacity"));
    }

    #[test]
    fn a_bare_array_is_a_batch_too() {
        let batch = serde_json::json!([{ "op": "set-duration", "seconds": 3.0 }]);
        let edited = apply_json(&lower_third(), &batch).expect("applies");
        assert_eq!(edited.duration, 3.0);
    }

    #[test]
    fn a_malformed_batch_names_the_shape_it_wanted() {
        let error = apply_json(&lower_third(), &serde_json::json!({ "ops": [{ "op": "fly" }] }))
            .expect_err("refused");
        assert!(error.to_string().contains("motion operations"), "{error}");
    }

    #[test]
    fn the_summary_serialises_for_a_tool_result() {
        let json = serde_json::to_string(&CompSummary::of(&lower_third())).expect("serialises");
        assert!(json.contains("\"slots\""));
        assert!(json.contains("\"needs_markup\":true"));
    }

    /// The batch printed in `docs/MOTION.md` and in the motion skill must keep
    /// parsing and applying - documentation that has drifted from the code is
    /// worse than none, and an agent copies these verbatim.
    #[test]
    fn the_documented_batch_parses_and_applies() {
        let batch = serde_json::json!({ "ops": [
          { "op": "set-slot", "slot": "Name", "value": "Grace Hopper" },
          { "op": "set-words", "node": "caption",
            "words": [["Make", 0, 0.45], ["it", 0.45, 0.85], ["move.", 0.85, null]] },
          { "op": "move", "node": "card", "start": 3.5 },
          { "op": "fade-in", "node": "card", "seconds": 0.8,
            "curve": { "kind": "bezier", "x1": 0.16, "y1": 1, "x2": 0.3, "y2": 1 } },
          { "op": "slide-to", "node": "card", "x": 960, "y": 540, "seconds": 0.7 },
          { "op": "pop", "node": "card", "from": 0.9, "seconds": 0.5 },
          { "op": "set-track", "node": "card", "property": "scale",
            "keys": [[0, 0.9, { "kind": "spring", "stiffness": 260, "damping": 15, "mass": 1 }],
                     [0.6, 1.0, null]] },
          { "op": "add-text", "id": "kicker", "text": "CHAPTER 02", "size": 28, "weight": 700,
            "color": "#22d3c5", "x": 240, "y": 140 }
        ]});

        // Every shape parses, with no leniency from serde.
        let ops: Vec<Op> = serde_json::from_value(batch["ops"].clone()).expect("the batch parses");
        assert_eq!(ops.len(), 8);

        // The caption op belongs to the caption comp, the rest to the lower third.
        let captured = presets::build("funcut.caption.kinetic").expect("a preset");
        let captioned = apply(&captured, &[ops[1].clone()]).expect("word timing applies");
        let caption = captioned.node("caption").expect("the caption");
        match &caption.kind {
            NodeKind::Text(text) => {
                assert_eq!(text.words.len(), 3);
                assert_eq!(text.words[0].text, "Make");
                // An open-ended last word is "hold it", so it is closed at the
                // comp's length here; a `None` in the middle is left open and
                // the engine holds it until the next word.
                assert_eq!(text.words[2].end, Some(captured.duration), "the last word holds");
            }
            other => panic!("expected words, got {other:?}"),
        }

        let rest: Vec<Op> = ops.into_iter().enumerate().filter(|(i, _)| *i != 1).map(|(_, op)| op).collect();
        let edited = apply(&lower_third(), &rest).expect("the rest applies");
        assert_eq!(edited.node("card").expect("the card").start, 3.5);
        assert!(edited.node("kicker").is_some(), "add-text landed");
        assert!(edited.validate().is_empty());
    }

    #[test]
    fn the_run_that_reads_the_sample_is_the_one_replaced() {
        let markup = "<div><b>Ada</b><i>Engineer</i><s>12:00</s></div>";
        assert_eq!(
            replace_text_run(markup, "Ada", "Grace"),
            "<div><b>Grace</b><i>Engineer</i><s>12:00</s></div>"
        );
        assert_eq!(
            replace_last_text_run(markup, "Grace"),
            "<div><b>Ada</b><i>Engineer</i><s>Grace</s></div>"
        );
        assert_eq!(replace_text_run("<div class='x'></div>", "", "Grace"), "<div class='x'></div>");
    }
}
