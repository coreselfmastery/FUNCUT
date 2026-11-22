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

use crate::camera::{CameraMove, apply_camera};
use crate::comp::{
    Blend, Comp, Length, Node, NodeKind, Paint, Point, Rgba, Slot, SlotField, Stroke, TextAlign,
    TextNode, TextStyle, Transform, Word,
};
use crate::curve::Curve;
use crate::engine::MotionError;
use crate::graphics::{Chart, Counter};
use crate::particles::ParticleSystem;
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
    /// Replace a markup node's HTML wholesale.
    ///
    /// The op a data-driven comp needs, where the words alone cannot say
    /// it: a chart whose bars carry real heights, a table with real rows.
    /// The stylesheet is the preset's and survives the rewrite, so the
    /// design language does too; a slot whose sample the new markup no
    /// longer carries goes quiet, and filling it then is the error that
    /// says so.
    SetMarkup {
        /// The node.
        node: String,
        /// The markup.
        markup: String,
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
    /// like "slide in over the first half second and settle". For
    /// `position` the one value lands on both axes, so a track written this
    /// way travels the diagonal; [`Op::SetTrackPoints`] carries the two
    /// axes apart.
    SetTrack {
        /// The node.
        node: String,
        /// `position`, `scale`, `rotation`, `opacity`, `blur`, `dash`,
        /// `gradient-offset`, or `param:<key>`.
        property: String,
        /// The keys.
        keys: Vec<(f64, f64, Option<Curve>)>,
    },
    /// Add or replace a `position` track, the two axes carried apart.
    ///
    /// The keys are `(at, x, y, curve)` quadruples, `at` a fraction of the
    /// node's span and the place in frame pixels, which is the shape a
    /// sentence like "arc from bottom-left to centre" produces.
    SetTrackPoints {
        /// The node.
        node: String,
        /// The keys, each `(at, x, y, curve)`.
        keys: Vec<(f64, f64, f64, Option<Curve>)>,
    },
    /// Remap a source through an input range into an output range, sampled
    /// into a target track - the `interpolate` primitive a programmatic
    /// video engine needs.
    ///
    /// `source` is `"time"` (seconds from the node's start) or another
    /// numeric track on the node. The input range is mapped linearly onto
    /// the output range; `clamp` holds values outside it at the ends, and
    /// `curve` shapes each sampled segment. This is the op an emotion read
    /// uses to turn a measured 0..1 signal into a real property range
    /// without hand-writing keys.
    Remap {
        /// The node.
        node: String,
        /// `time`, or a numeric property the node already animates.
        source: String,
        /// The numeric property to write.
        target: String,
        /// The input range.
        input: (f64, f64),
        /// The output range.
        output: (f64, f64),
        /// Hold values outside the input range at the ends.
        #[serde(default)]
        clamp: bool,
        /// How many keys the remap samples; 16 by default.
        #[serde(default = "default_samples")]
        samples: usize,
        /// The curve each sampled segment rides.
        #[serde(default)]
        curve: Option<Curve>,
    },
    /// Add or replace a colour track: `fill` on a shape, `ink` on the
    /// words, `plate` on the plate behind them, or `stroke-color` on a
    /// stroked path.
    ///
    /// The keys carry `#rrggbb`/`#rrggbbaa` strings, mixed key to key
    /// the way the engines mix everything else - straight components,
    /// alpha riding along. This is the op a measured read turns into
    /// motion: a card whose ink shifts from the brand's calm to its
    /// alert as the line lands, a karaoke colour trip, a plate that
    /// cools as the scene settles.
    SetTrackColors {
        /// The node.
        node: String,
        /// `fill`, `ink`, `plate` or `stroke-color`.
        property: String,
        /// The keys, each `(at, colour, curve)`.
        keys: Vec<(f64, String, Option<Curve>)>,
    },
    /// Add or replace a `size` track on a rectangle or an ellipse: the
    /// two dimensions carried apart, as `(at, width, height, curve)`
    /// quadruples in frame pixels - the shape a sentence like "grow the
    /// chip from its label's width to the card's" produces. A bar
    /// filling, a window opening, a reveal wiping by its own box.
    SetTrackSize {
        /// The node.
        node: String,
        /// The keys, each `(at, width, height, curve)`.
        keys: Vec<(f64, f64, f64, Option<Curve>)>,
    },
    /// Give a node organic drift on a seeded noise field: the property
    /// wanders around the value the node already holds, the keys baked
    /// from [`crate::noise::Noise`] as the operation is applied - so the
    /// document carries only numbers, and the render needs no randomness
    /// of any kind. Deterministic by construction: the seed is the whole
    /// of it.
    ///
    /// `position` wanders in pixels (the node must be placed); `scale`,
    /// `rotation` and `opacity` wander in their own units around the
    /// node's own resting value, opacity held inside 0 to 1.
    SetNoiseTrack {
        /// The node.
        node: String,
        /// The property to wander: `position`, `scale`, `rotation` or
        /// `opacity`.
        property: String,
        /// The noise field's seed: the whole of the randomness.
        seed: u64,
        /// How far the wander reaches, each side of the resting value -
        /// pixels for a position, ones for everything else.
        amount: f64,
        /// Keys a second the field is sampled at, one knob for the feel:
        /// a higher rate samples finer *and* wanders faster. Held to
        /// 1..=30 whatever the document says.
        #[serde(default = "default_noise_rate")]
        rate: f64,
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
    /// Add a data-driven chart: any domain's numbers as columns, a trend,
    /// an area under it, shares of a whole around a ring, one measured
    /// value a point on a shared axis, two numbers a category as paired
    /// columns, or a number against its target as a fill grown inside the
    /// track the target draws - every height, slope, share and position
    /// computed from the data, drawn as rectangles, ellipses and text the
    /// default engine renders as they stand. The nodes are the root-level
    /// `{id}.…` family, so a camera, a delete or
    /// a retime reaches them without a group's transform in the way.
    AddChart {
        /// The family's id.
        id: String,
        /// The box's left edge, in frame pixels.
        x: f64,
        /// Its top edge.
        y: f64,
        /// How wide the chart's box is.
        width: f64,
        /// How tall it is.
        height: f64,
        /// When the chart enters, in seconds.
        #[serde(default)]
        at: f64,
        /// The chart: kind, data, formatting and pacing.
        chart: Chart,
    },
    /// Add a counter: a number that climbs from `from` to `to`, step by
    /// eased step - every step a text node that shows for its moment, the
    /// last one holding to the end of the comp. No renderer animates a
    /// number; the count is baked, and reads as one number climbing.
    AddCounter {
        /// The family's id.
        id: String,
        /// Where the number's centre sits.
        x: f64,
        /// Where it sits.
        y: f64,
        /// When the count starts, in seconds.
        #[serde(default)]
        at: f64,
        /// The counter: bounds, formatting, pacing and size.
        counter: Counter,
    },
    /// Add a seeded particle field, baked. The simulation - emitters,
    /// lifetimes, gravity, drag, turbulence - runs here, at authoring
    /// time, on a fixed timestep from the seed; every particle lands as a
    /// node with its own position, scale and opacity tracks, so any
    /// render, in any frame order, draws the same field twice.
    AddParticles {
        /// The family's id.
        id: String,
        /// The field's left edge, in frame pixels: particles are born
        /// inside this box.
        x: f64,
        /// Its top edge.
        y: f64,
        /// How wide the birth box is.
        width: f64,
        /// How tall it is.
        height: f64,
        /// When the field lights up, in seconds.
        #[serde(default)]
        at: f64,
        /// A named field to start from: one of `snow`, `rain`, `embers`,
        /// `sparks`, `dust`, `atmosphere`, `confetti`, `fireflies`,
        /// `energy`, `streaks`, `data`. With no `system` beside it this
        /// is the whole configuration; with one, every field the `system`
        /// names wins.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        /// The system: seed, forces, curves over life - every field
        /// optional here, patching the preset when there is one and the
        /// plain default when there is not. A document that always gave
        /// the full system parses exactly as it did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<crate::particles::ParticlePatch>,
    },
    /// Add a CPU 3D scene: primitives, a camera, lights and a z-buffer,
    /// drawn by the native engine as a full-frame layer. Either a named
    /// preset (`showcase`, `product`) or a complete scene.
    #[serde(rename = "add-scene3d")]
    AddScene3D {
        /// Its id.
        id: String,
        /// A named scene to start from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        /// A complete scene; wins over `preset` when both are given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scene: Option<crate::scene3d::Scene>,
        /// When it enters, in seconds.
        #[serde(default)]
        at: f64,
        /// How long it runs; `0` runs to the end of the comp.
        #[serde(default)]
        duration: f64,
    },
    /// Replace the scene on an existing 3D node.
    #[serde(rename = "set-scene3d")]
    SetScene3D {
        /// The node.
        node: String,
        /// The scene.
        scene: crate::scene3d::Scene,
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
    /// Move the camera: a zoom, a pan or a roll, baked into every root
    /// layer's own tracks over `seconds`, with a depth per layer for
    /// parallax - the 2.5D of a comp the reel layer's own `parallax`
    /// rate gives a storyboard. Composes with the motion each layer
    /// already carries, key by key; applying it twice moves twice.
    SetCamera {
        /// The move: zoom, pan, roll, seconds, curve, depths.
        camera: CameraMove,
    },
    /// Re-ink a text node: the one colour its words draw with, a gradient
    /// over them replaced by it, because an ink is one opinion. The lint's
    /// contrast gate writes these through [`patches`](crate::lint::patches).
    Ink {
        /// The node.
        node: String,
        /// The colour.
        color: Rgba,
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
            MotionError::Engine {
                code,
                message,
                path,
                help,
            } => MotionError::Engine {
                code,
                message: format!("operation {index}: {message}"),
                path,
                help,
            },
            other => other,
        })?;
    }
    let problems = next.validate();
    if !problems.is_empty() {
        return Err(MotionError::Invalid {
            diagnostics: problems,
        });
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
            // The matched slot is also the one whose sample is the needle: a
            // node can carry two slots (a name and a role beside it), and the
            // first slot on the node would replace the wrong one's words.
            let (node, field, sample) = comp
                .slots
                .iter()
                .find(|candidate| {
                    candidate.label.eq_ignore_ascii_case(slot)
                        || candidate.node.eq_ignore_ascii_case(slot)
                })
                .map(|candidate| {
                    (
                        candidate.node.clone(),
                        candidate.field,
                        candidate.sample.clone(),
                    )
                })
                .ok_or_else(|| {
                    fail(
                        format!("no slot called {slot:?}"),
                        "the comp lists its slots under `slots`",
                    )
                })?;
            match field {
                SlotField::Text => {
                    set_text_in(&mut comp, &node, slot, &sample, value)?;
                }
                SlotField::Image => {
                    let entry = node_mut(&mut comp, &node)?;
                    if let NodeKind::Image { asset, .. } = &mut entry.kind {
                        *asset = value.clone();
                    }
                }
            }
            Ok(comp)
        }
        Op::SetText { node, text } => {
            set_text(&mut comp, node, text)?;
            Ok(comp)
        }
        Op::Ink { node, color } => {
            let entry = node_mut(&mut comp, node)?;
            match &mut entry.kind {
                NodeKind::Text(text) => {
                    // The words draw with the one colour; a gradient over
                    // them is replaced by it, because an ink is one opinion.
                    text.color = *color;
                    text.fill = None;
                    Ok(comp)
                }
                _ => Err(not_text(node)),
            }
        }
        Op::SetMarkup { node, markup } => {
            let entry = node_mut(&mut comp, node)?;
            if markup.trim().is_empty() {
                return Err(fail(
                    format!("{node:?} would draw nothing"),
                    "set-markup replaces the words a node draws, not the node; delete it if it should go",
                ));
            }
            match &mut entry.kind {
                NodeKind::Html { markup: drawn, .. } => {
                    *drawn = markup.clone();
                    Ok(comp)
                }
                _ => Err(not_markup(node)),
            }
        }
        Op::SetWords { node, words } => {
            // Word times are relative to the node, so an open-ended last
            // word closes at the node's own end, not the comp's - a node
            // that starts at 2 s in a 6 s comp does not hold its last word
            // for the whole 6 s.
            let span = span_of(&comp, node)?;
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
        Op::SetTrack {
            node,
            property,
            keys,
        } => {
            if !documented_property(property) {
                return Err(fail(
                    format!("{property:?} is not a property this format animates"),
                    "use position, scale, rotation, opacity, blur, dash, or param:<key> for an effect parameter",
                ));
            }
            let mut track = Track::default();
            for (at, value, curve) in keys {
                let value = if property == "position" {
                    Value::Point {
                        x: *value,
                        y: *value,
                    }
                } else {
                    Value::Number { value: *value }
                };
                track.keys.push(match curve {
                    Some(curve) => Key::eased(*at, value, curve.clone()),
                    None => Key::new(*at, value),
                });
            }
            track
                .keys
                .sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert(property.clone(), track);
            Ok(comp)
        }
        Op::SetTrackPoints { node, keys } => {
            let mut track = Track::default();
            for (at, x, y, curve) in keys {
                let value = Value::Point { x: *x, y: *y };
                track.keys.push(match curve {
                    Some(curve) => Key::eased(*at, value, curve.clone()),
                    None => Key::new(*at, value),
                });
            }
            track
                .keys
                .sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert("position".into(), track);
            Ok(comp)
        }
        Op::Remap {
            node,
            source,
            target,
            input,
            output,
            clamp,
            samples,
            curve,
        } => {
            if target == "position" {
                return Err(fail(
                    "remap writes one numeric property, not a position".into(),
                    "use set-track-points for position, or remap scale, rotation, opacity, blur, dash or param:<key>",
                ));
            }
            if !documented_property(target) {
                return Err(fail(
                    format!("{target:?} is not a property this format animates"),
                    "use scale, rotation, opacity, blur, dash, or param:<key> for an effect parameter",
                ));
            }
            if !input.0.is_finite()
                || !input.1.is_finite()
                || !output.0.is_finite()
                || !output.1.is_finite()
            {
                return Err(fail(
                    "remap ranges must be finite".into(),
                    "write two finite numbers for input and two for output",
                ));
            }
            if (input.1 - input.0).abs() < 1e-12 {
                return Err(fail(
                    "remap input range is empty".into(),
                    "write two different numbers for input, for example [0, 1]",
                ));
            }
            let span = span_of(&comp, node)?;
            let source_track = if source == "time" {
                None
            } else {
                Some(
                    comp.tracks_of(node)
                        .and_then(|tracks| tracks.get(source))
                        .cloned()
                        .ok_or_else(|| {
                            fail(
                                format!("node {node:?} has no {source:?} track to remap"),
                                "use \"time\", or a numeric property the node already animates",
                            )
                        })?,
                )
            };
            let samples = (*samples).clamp(2, 120);
            let mut track = Track::default();
            for index in 0..samples {
                let at = index as f64 / (samples - 1) as f64;
                let value = match &source_track {
                    None => at * span,
                    Some(source_track) => source_track
                        .sample(at)
                        .and_then(|value| value.as_number())
                        .ok_or_else(|| {
                            fail(
                                format!("the {source:?} track is not numeric"),
                                "remap reads numbers; use a numeric property",
                            )
                        })?,
                };
                let mut normalized = (value - input.0) / (input.1 - input.0);
                if *clamp {
                    normalized = normalized.clamp(0.0, 1.0);
                }
                let mapped = output.0 + (output.1 - output.0) * normalized;
                track.keys.push(match curve {
                    Some(curve) => Key::eased(at, Value::number(mapped), curve.clone()),
                    None => Key::new(at, Value::number(mapped)),
                });
            }
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert(target.to_string(), track);
            Ok(comp)
        }
        Op::SetTrackColors {
            node,
            property,
            keys,
        } => {
            if !documented_colour_property(property) {
                return Err(fail(
                    format!("{property:?} is not a colour this format animates"),
                    "use fill, ink, plate, or stroke-color",
                ));
            }
            let mut track = Track::default();
            for (at, colour, curve) in keys {
                // The hex reader answers opaque white for anything it
                // cannot read, because a comp must render even when a
                // hand-written document is wrong - but an op that hands
                // it garbage is a caller's mistake worth naming now,
                // before it paints every frame white.
                if Rgba::hex(colour) == Rgba::new(1.0, 1.0, 1.0, 1.0)
                    && !matches!(
                        colour.trim().to_ascii_lowercase().as_str(),
                        "#fff" | "#ffffff" | "#ffffffff" | "fff" | "ffffff" | "ffffffff"
                    )
                {
                    return Err(fail(
                        format!("{colour:?} is not a colour"),
                        "write it as #rrggbb or #rrggbbaa",
                    ));
                }
                let value = Value::Color {
                    r: Rgba::hex(colour).r,
                    g: Rgba::hex(colour).g,
                    b: Rgba::hex(colour).b,
                    a: Rgba::hex(colour).a,
                };
                track.keys.push(match curve {
                    Some(curve) => Key::eased(*at, value, curve.clone()),
                    None => Key::new(*at, value),
                });
            }
            track
                .keys
                .sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert(property.clone(), track);
            Ok(comp)
        }
        Op::SetTrackSize { node, keys } => {
            let mut track = Track::default();
            for (at, width, height, curve) in keys {
                let value = Value::Size {
                    width: *width,
                    height: *height,
                };
                track.keys.push(match curve {
                    Some(curve) => Key::eased(*at, value, curve.clone()),
                    None => Key::new(*at, value),
                });
            }
            track
                .keys
                .sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
            let entry = node_mut(&mut comp, node)?;
            // A size track on a node that is not a box is a mistake the
            // validation pass would rather name than ignore: the kind is
            // checked here so the error points at the op, not the
            // document it produced.
            if !matches!(entry.kind, NodeKind::Rect { .. } | NodeKind::Ellipse { .. }) {
                return Err(fail(
                    format!("{:?} is not a box a size track grows", node),
                    "size tracks ride rectangles and ellipses; scale is every node's",
                ));
            }
            entry.tracks.insert("size".into(), track);
            Ok(comp)
        }
        Op::SetNoiseTrack {
            node,
            property,
            seed,
            amount,
            rate,
        } => {
            if !matches!(
                property.as_str(),
                "position" | "scale" | "rotation" | "opacity"
            ) {
                return Err(fail(
                    format!("{property:?} is not a property the noise wanders"),
                    "use position (pixels, placed nodes only), scale, rotation or opacity",
                ));
            }
            if !amount.is_finite() || *amount <= 0.0 {
                return Err(fail(
                    format!("the amount {amount} is not a positive number"),
                    "name how far the wander reaches, each side of the node's resting value",
                ));
            }
            let span = span_of(&comp, node)?;
            // A node with no position has nowhere to wander from: the
            // transitions hold the same line - inventing one would move
            // the node - and an operation that moved a node by accident
            // would be worse than one that refused.
            if property == "position"
                && comp
                    .node(node)
                    .is_some_and(|node| node.transform.position.is_none())
            {
                return Err(fail(
                    "the node has no position to wander around".to_string(),
                    "place it first (move / set-track-points), then let it drift",
                ));
            }
            let rate = if rate.is_finite() && *rate > 0.0 {
                rate.clamp(1.0, 30.0)
            } else {
                default_noise_rate()
            };
            let noise = crate::noise::Noise::from_seed(*seed);
            // The resting values, read before the node is taken mutably:
            // what the property wanders *around* is the value the node
            // already holds.
            let resting = {
                let base = comp.node(node).ok_or_else(|| {
                    fail(
                        format!("no node called {node:?}"),
                        "list the comp first: motion.get returns its nodes",
                    )
                })?;
                (
                    base.transform.position,
                    base.transform.scale,
                    base.transform.rotation,
                    base.opacity,
                )
            };
            let steps = ((span * rate).ceil().min(600.0)) as usize;
            let mut track = Track::default();
            for step in 0..=steps {
                let fraction = if steps == 0 {
                    0.0
                } else {
                    f64::from(u32::try_from(step).unwrap_or(0))
                        / f64::from(u32::try_from(steps).unwrap_or(1))
                };
                // The field is read at one cycle per ten keys, matching the
                // layer wobble's clock: the same feel at the same rate.
                let (x, y) = noise.drift(fraction * span * rate / 10.0, 0);
                let value = match property.as_str() {
                    "position" => {
                        let placed = resting.0.expect("checked above");
                        Value::Point {
                            x: placed.x + x * amount,
                            y: placed.y + y * amount,
                        }
                    }
                    "scale" => Value::number(resting.1 + x * amount),
                    "rotation" => Value::number(resting.2 + x * amount),
                    _ => Value::number((resting.3 + x * amount).clamp(0.0, 1.0)),
                };
                track
                    .keys
                    .push(Key::new((fraction * 1000.0).round() / 1000.0, value));
            }
            // A wobble's keys are its own: the op replaces the property's
            // track, the way set-track does - an op that meant to keep the
            // old motion can apply both.
            track
                .keys
                .sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
            track.keys.dedup_by(|a, b| (a.at - b.at).abs() <= 1e-9);
            let entry = node_mut(&mut comp, node)?;
            entry.tracks.insert(property.clone(), track);
            Ok(comp)
        }
        Op::FadeIn {
            node,
            seconds,
            curve,
        } => {
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
        Op::SlideTo {
            node,
            x,
            y,
            seconds,
        } => {
            let span = span_of(&comp, node)?;
            let at = (seconds / span).clamp(0.001, 1.0);
            let entry = node_mut(&mut comp, node)?;
            let from = entry.transform.position.unwrap_or(Point::new(0.0, 0.0));
            entry.tracks.insert(
                "position".into(),
                Track::new(vec![
                    Key::eased(
                        0.0,
                        Value::Point {
                            x: from.x,
                            y: from.y,
                        },
                        Curve::SPRING,
                    ),
                    Key::new(at, Value::Point { x: *x, y: *y }),
                    Key::new(1.0, Value::Point { x: *x, y: *y }),
                ]),
            );
            entry.transform.position = Some(Point::new(*x, *y));
            Ok(comp)
        }
        Op::Pop {
            node,
            from,
            seconds,
        } => {
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
                transform: Transform {
                    position: Some(Point::new(*x, *y)),
                    ..Transform::default()
                },
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                matte: None,
                motion_blur: None,
                effects: Vec::new(),
                tracks: BTreeMap::new(),
                kind: NodeKind::Group {
                    children: Vec::new(),
                },
            });
            Ok(comp)
        }
        Op::AddRect {
            id,
            width,
            height,
            fill,
            radius,
            x,
            y,
        } => {
            push(&mut comp, id)?;
            comp.nodes.push(Node {
                id: id.clone(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform {
                    position: Some(Point::new(*x, *y)),
                    ..Transform::default()
                },
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                matte: None,
                motion_blur: None,
                effects: Vec::new(),
                tracks: BTreeMap::new(),
                kind: NodeKind::Rect {
                    width: Length::Px(*width),
                    height: Length::Px(*height),
                    fill: (!fill.is_empty()).then(|| Paint::Solid {
                        color: Rgba::hex(fill),
                    }),
                    radius: *radius,
                },
            });
            Ok(comp)
        }
        Op::AddText {
            id,
            text,
            size,
            weight,
            color,
            x,
            y,
        } => {
            push(&mut comp, id)?;
            comp.nodes.push(Node {
                id: id.clone(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform {
                    position: Some(Point::new(*x, *y)),
                    ..Transform::default()
                },
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                matte: None,
                motion_blur: None,
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
                    fit: None,
                    min_size: None,
                    on_path: None,
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
        Op::AddChart {
            id,
            x,
            y,
            width,
            height,
            at,
            chart,
        } => {
            reserve(&comp, id)?;
            check_at(&comp, *at)?;
            let area = crate::graphics::Area {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            };
            let nodes = crate::graphics::chart_nodes(id, chart, area, *at, comp.duration)?;
            comp.nodes.extend(nodes);
            Ok(comp)
        }
        Op::AddCounter {
            id,
            x,
            y,
            at,
            counter,
        } => {
            reserve(&comp, id)?;
            check_at(&comp, *at)?;
            let nodes = crate::graphics::counter_nodes(id, counter, *x, *y, *at, comp.duration)?;
            comp.nodes.extend(nodes);
            Ok(comp)
        }
        Op::AddParticles {
            id,
            x,
            y,
            width,
            height,
            at,
            preset,
            system,
        } => {
            reserve(&comp, id)?;
            check_at(&comp, *at)?;
            let base = match preset {
                Some(name) => crate::particles::preset(name).ok_or_else(|| {
                    fail(
                        format!("the particle preset '{name}' is not one the library names"),
                        format!(
                            "the names are {}",
                            crate::particles::PRESET_NAMES.join(", ")
                        ),
                    )
                })?,
                None => ParticleSystem::default(),
            };
            let system = system.clone().unwrap_or_default().onto(&base);
            let nodes = crate::particles::particle_nodes(
                id,
                &system,
                *x,
                *y,
                *width,
                *height,
                *at,
                comp.duration,
            )?;
            comp.nodes.extend(nodes);
            Ok(comp)
        }
        Op::AddScene3D {
            id,
            preset,
            scene,
            at,
            duration,
        } => {
            reserve(&comp, id)?;
            check_at(&comp, *at)?;
            let scene = match (scene, preset) {
                (Some(scene), _) => scene.clone(),
                (None, Some(name)) => {
                    crate::scene3d::Scene::preset_at(name, comp.width, comp.height).ok_or_else(
                        || {
                            fail(
                                format!("the 3D preset '{name}' is not one the library names"),
                                format!(
                                    "the names are {}",
                                    crate::scene3d::Scene::PRESET_NAMES.join(", ")
                                ),
                            )
                        },
                    )?
                }
                (None, None) => {
                    return Err(fail(
                        "add-scene3d needs a preset or a scene".into(),
                        format!(
                            "the presets are {}",
                            crate::scene3d::Scene::PRESET_NAMES.join(", ")
                        ),
                    ));
                }
            };
            scene.validate().map_err(|why| {
                fail(
                    format!("the 3D scene does not validate: {why}"),
                    "check the objects, geometry and lights",
                )
            })?;
            comp.nodes.push(Node {
                id: id.clone(),
                parent: None,
                start: *at,
                duration: (*duration).max(0.0),
                transform: Transform::default(),
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                matte: None,
                effects: Vec::new(),
                motion_blur: None,
                tracks: BTreeMap::new(),
                kind: NodeKind::Scene3D { scene },
            });
            Ok(comp)
        }
        Op::SetScene3D { node, scene } => {
            scene.validate().map_err(|why| {
                fail(
                    format!("the 3D scene does not validate: {why}"),
                    "check the objects, geometry and lights",
                )
            })?;
            let target = node_mut(&mut comp, node)?;
            if !matches!(&target.kind, NodeKind::Scene3D { .. }) {
                return Err(fail(
                    format!("{node:?} is not a 3D scene node"),
                    "set-scene3d changes the scene on a node add-scene3d made",
                ));
            }
            target.kind = NodeKind::Scene3D {
                scene: scene.clone(),
            };
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
            // A group naming a deleted child would fail validation after
            // the delete had already gone through: the children lists are
            // pruned with the nodes themselves.
            for entry in &mut comp.nodes {
                if let NodeKind::Group { children } = &mut entry.kind {
                    children.retain(|child| !doomed.contains(child));
                }
            }
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
        Op::SetCamera { camera } => {
            apply_camera(&mut comp, camera)?;
            Ok(comp)
        }
    }
}

/// Turn words into a [`TextNode`]'s text, keeping its style.
fn set_text(comp: &mut Comp, id: &str, text: &str) -> Result<(), MotionError> {
    set_text_in(comp, id, "set-text", "", text)
}

/// The same, but in a markup node it replaces the run that reads `sample`.
///
/// A slot's sample is the words the preset drew there, so it is also the
/// needle: "Ada Lovelace" finds the name and not the timecode beside it.
/// `what` is who was filling - a slot's label, or the op - so the refusal
/// names the ask and not only the needle.
fn set_text_in(
    comp: &mut Comp,
    id: &str,
    what: &str,
    sample: &str,
    text: &str,
) -> Result<(), MotionError> {
    let entry = node_mut(comp, id)?;
    match &mut entry.kind {
        NodeKind::Text(node) => {
            node.text = text.to_string();
            node.words.clear();
            Ok(())
        }
        NodeKind::Html { markup, .. } => {
            // A markup comp's words live in its element tree; the slot's
            // sample says which words are the ones to change. A run first (it
            // keeps the emphasis around the words it does not touch), then a
            // span across inline tags, which is what a headline like
            // `One <em>tap</em> away.` needs. A sample that is nowhere in the
            // markup is a broken template: filling some other word would be a
            // silent, wrong edit, so it is refused instead - and a value
            // equal to the sample is still a fill, not a failure.
            let found = try_replace_run(markup, sample, text)
                .or_else(|| try_replace_span(markup, sample, text));
            match found {
                Some(next) => {
                    *markup = next;
                    Ok(())
                }
                None => Err(MotionError::Engine {
                    code: None,
                    message: format!(
                        "the slot {what} on node {id:?} has the sample {sample:?}, which is not in its markup"
                    ),
                    path: None,
                    help: Some(
                        "a slot's sample is the literal text it stands in for; fix the comp's `slots`"
                            .into(),
                    ),
                }),
            }
        }
        _ => Err(MotionError::Unsupported {
            node: id.into(),
            what: "text".into(),
            engine: crate::engine::EngineKind::Native,
        }),
    }
}

/// The text runs of a markup string: `(start, end, words)` for each stretch of
/// text between one `>` and the next `<`. Not an HTML parser - a comp's markup
/// is its own, and a run is what a slot is filled by.
fn text_runs(markup: &str) -> Vec<(usize, usize, &str)> {
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
                let from = start + markup[start..end].find(run).unwrap_or(0);
                runs.push((from, from + run.len(), run));
            }
            i = end;
            continue;
        }
        i += 1;
    }
    runs
}

/// The run reading exactly `sample`, replaced - or `None` when no run does.
///
/// The value is escaped the way [`replace_text_span`] escapes one, so
/// filling a slot can never inject markup through this path either.
fn try_replace_run(markup: &str, sample: &str, value: &str) -> Option<String> {
    let sample = sample.trim();
    if sample.is_empty() {
        return None;
    }
    let (start, end, _) = text_runs(markup)
        .into_iter()
        .find(|(_, _, run)| *run == sample)?;
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut out = String::with_capacity(markup.len());
    out.push_str(&markup[..start]);
    out.push_str(&escaped);
    out.push_str(&markup[end..]);
    Some(out)
}

/// Replace a text run in a markup string: the one reading `sample`, or the
/// last one when there is no such run.
///
/// Not a parser: a comp's markup is the preset's own, and the edit an agent
/// asks for ("make the name Grace Hopper") is always a whole run. Matching by
/// the run's own words is what keeps a name from landing on the timecode next
/// to it. A document with no text run is left alone.
pub fn replace_text_run(markup: &str, sample: &str, value: &str) -> String {
    if let Some(replaced) = try_replace_run(markup, sample, value) {
        return replaced;
    }
    match text_runs(markup).last() {
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

/// Replace a span of *visible text* in a markup string, even where it crosses
/// inline tags: the sample `One tap away.` finds `<div>One <em>tap</em>
/// away.</div>` and puts `value` in its place, dropping the tags inside the
/// span (a slot's value carries no markup).
///
/// Whitespace is matched loosely - a sample written with single spaces finds
/// text broken across lines by the author - which is how a template's samples
/// stay readable. The value is escaped, so filling a slot can never inject
/// markup into a comp.
pub fn replace_text_span(markup: &str, sample: &str, value: &str) -> String {
    try_replace_span(markup, sample, value).unwrap_or_else(|| markup.to_string())
}

/// [`replace_text_span`], saying whether it found the sample.
fn try_replace_span(markup: &str, sample: &str, value: &str) -> Option<String> {
    let sample: Vec<char> = sample
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect();
    if sample.is_empty() {
        return None;
    }
    // Every visible character, with where it lives in the markup.
    let mut visible: Vec<(char, usize)> = Vec::new();
    let mut i = 0;
    let bytes = markup.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b'<' => {
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                i += 1;
            }
            _ => {
                let ch = markup[i..].chars().next().unwrap_or(' ');
                for (at, part) in ch.to_string().chars().enumerate() {
                    visible.push((part, i + at));
                }
                i += ch.len_utf8();
            }
        }
    }
    // Collapse whitespace so runs broken across lines still match.
    let mut flat: Vec<(char, usize)> = Vec::new();
    for (ch, at) in &visible {
        if ch.is_whitespace() {
            if flat.last().is_some_and(|(last, _)| *last == ' ') {
                continue;
            }
            flat.push((' ', *at));
        } else {
            flat.push((*ch, *at));
        }
    }
    let haystack: Vec<char> = flat.iter().map(|(ch, _)| *ch).collect();
    let start = haystack
        .windows(sample.len())
        .position(|window| window == sample.as_slice())?;
    let from = flat[start].1;
    let last = start + sample.len() - 1;
    let to = flat[last].1 + flat[last].0.len_utf8();
    // Widen to the character's whole utterance, and drop any tag the span
    // starts or ends inside, so the result is still valid markup.
    let mut end = to;
    while end < markup.len() && !markup.is_char_boundary(end) {
        end += 1;
    }
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut out = String::with_capacity(markup.len());
    out.push_str(&markup[..from]);
    out.push_str(&escaped);
    out.push_str(&markup[end..]);
    Some(out)
}

/// Kept for callers that only want *some* run replaced; see
/// [`replace_text_run`] for the one that respects a slot's sample.
#[doc(hidden)]
pub fn replace_last_text_run(markup: &str, value: &str) -> String {
    replace_text_run(markup, "", value)
}

/// A property name the format animates.
fn documented_property(name: &str) -> bool {
    matches!(
        name,
        "position" | "scale" | "rotation" | "opacity" | "blur" | "dash" | "gradient-offset"
    ) || name.starts_with("param:")
}

fn default_samples() -> usize {
    16
}

/// The properties a [`Op::SetTrackColors`] key may drive.
fn documented_colour_property(name: &str) -> bool {
    matches!(name, "fill" | "ink" | "plate" | "stroke-color")
}

fn node_mut<'a>(comp: &'a mut Comp, id: &str) -> Result<&'a mut Node, MotionError> {
    let index = comp
        .nodes
        .iter()
        .position(|node| node.id == id)
        .ok_or_else(|| {
            fail(
                format!("no node called {id:?}"),
                "list the comp first: motion.get returns its nodes",
            )
        })?;
    Ok(&mut comp.nodes[index])
}

fn span_of(comp: &Comp, id: &str) -> Result<f64, MotionError> {
    comp.node(id)
        .map(|node| {
            let end = node.end(comp.duration);
            (end - node.start).max(0.1)
        })
        .ok_or_else(|| {
            fail(
                format!("no node called {id:?}"),
                "list the comp first: motion.get returns its nodes",
            )
        })
}

/// The keys a second a `set-noise-track` operation samples at, when the
/// document does not name a rate: the same default the layer wobble
/// carries, so an unsaid rate reads the same in both places.
fn default_noise_rate() -> f64 {
    8.0
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

/// The id a generated family of nodes hangs off, reserved whole: a chart,
/// a counter or a particle field names its nodes `{id}.…`, so the prefix
/// is the fence - an id that would land inside a family already drawn is
/// refused before the first node is built.
fn reserve(comp: &Comp, id: &str) -> Result<(), MotionError> {
    let prefix = format!("{id}.");
    if comp
        .nodes
        .iter()
        .any(|node| node.id == id || node.id.starts_with(&prefix))
    {
        return Err(fail(
            format!("a node family called {id:?} already exists"),
            "a generated family names its nodes `{id}.title`, `{id}.bar.0` and on; the whole prefix is its id",
        ));
    }
    Ok(())
}

/// A start time inside the comp, checked before anything is built: a
/// family that starts past the end would validate and draw nothing.
fn check_at(comp: &Comp, at: f64) -> Result<(), MotionError> {
    if !at.is_finite() || at < 0.0 || at >= comp.duration {
        return Err(fail(
            format!("{at}s is outside this {}s comp", comp.duration),
            "start it inside the comp's own span",
        ));
    }
    Ok(())
}

fn not_text(node: &str) -> MotionError {
    fail(
        format!("{node:?} is not a text node"),
        "only a text node carries words",
    )
}

fn not_markup(node: &str) -> MotionError {
    fail(
        format!("{node:?} is not a markup node"),
        "set-markup rewrites the HTML a markup node draws; a text node takes set-text",
    )
}

fn fail(message: String, help: impl Into<String>) -> MotionError {
    MotionError::Engine {
        code: None,
        message,
        path: None,
        help: Some(help.into()),
    }
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
                        NodeKind::Adjustment => ("adjustment", None),
                        NodeKind::Text(text) => ("text", Some(first_words(text))),
                        NodeKind::Html { markup, .. } => (
                            "html",
                            Some(markup.lines().next().unwrap_or_default().trim().to_string()),
                        ),
                        NodeKind::Image { .. } => ("image", None),
                        NodeKind::Lottie { .. } => ("lottie", None),
                        NodeKind::Composition { .. } => ("composition", None),
                        NodeKind::Camera { .. } => ("camera", None),
                        NodeKind::Scene3D { .. } => ("scene3d", None),
                        NodeKind::Rig { .. } => ("rig", None),
                        NodeKind::Audio { .. } => ("audio", None),
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
    TextStyle {
        color: Some(Rgba::hex(color)),
        fill: None,
        weight: Some(weight),
        scale: None,
    }
}

/// A stroke of `width` pixels in `color`.
pub fn stroke(color: &str, width: f64) -> Stroke {
    Stroke {
        color: Rgba::hex(color),
        width,
    }
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
                Op::SetSlot {
                    slot: "Name".into(),
                    value: "Grace Hopper".into(),
                },
                Op::Move {
                    node: "card".into(),
                    start: 3.5,
                },
                Op::Retime {
                    node: "card".into(),
                    duration: 2.5,
                },
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
            &[Op::SetSlot {
                slot: "Name".into(),
                value: "Grace Hopper".into(),
            }],
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
        let error = apply(
            &comp,
            &[Op::Move {
                node: "nope".into(),
                start: 1.0,
            }],
        )
        .expect_err("refused");
        assert!(error.to_string().contains("nope"), "{error}");
    }

    #[test]
    fn an_unknown_slot_is_refused_rather_than_silently_dropped() {
        let comp = lower_third();
        let error = apply(
            &comp,
            &[Op::SetSlot {
                slot: "Nickname".into(),
                value: "x".into(),
            }],
        )
        .expect_err("refused");
        assert!(error.to_string().contains("Nickname"), "{error}");
    }

    #[test]
    fn set_markup_hands_a_markup_node_whole_new_html() {
        let comp = presets::build("funcut.chart.bars").expect("a preset");
        // The rewrite carries the chart title's own sample forward, the way a
        // data-driven rewrite keeps the words it did not come to change.
        let rewritten = r#"<div class="sheet"><div class="head"><div class="t">Where the money went</div><div class="s">AFTER 30 YEARS</div></div><div class="plot"><div class="col"><div class="bar" style="height: 26%; background: #22d3c5"></div><div class="lab">YOU $180K</div></div></div></div>"#;
        let edited = apply(
            &comp,
            &[Op::SetMarkup {
                node: "sheet".into(),
                markup: rewritten.into(),
            }],
        )
        .expect("the rewrite applies");
        let sheet = edited.node("sheet").expect("the sheet");
        match &sheet.kind {
            NodeKind::Html { markup, .. } => assert!(markup.contains("YOU $180K"), "{markup}"),
            other => panic!("expected markup, got {other:?}"),
        }
        // A slot whose sample survived the rewrite still fills: the batch is
        // a rewrite, not a lobotomy.
        let edited = apply(
            &edited,
            &[Op::SetSlot {
                slot: "Chart title".into(),
                value: "Who put it there".into(),
            }],
        )
        .expect("the surviving slot fills");
        let sheet = edited.node("sheet").expect("the sheet");
        match &sheet.kind {
            NodeKind::Html { markup, .. } => {
                assert!(markup.contains("Who put it there"), "{markup}")
            }
            other => panic!("expected markup, got {other:?}"),
        }
        // The bar labels' samples are gone from the new markup; filling one
        // is the error that says so, rather than a fill of whatever words
        // happen to be left.
        let error = apply(
            &edited,
            &[Op::SetSlot {
                slot: "Bar 1 label".into(),
                value: "Q1".into(),
            }],
        )
        .expect_err("the sample is gone");
        assert!(error.to_string().contains("Bar 1 label"), "{error}");
    }

    #[test]
    fn set_markup_is_refused_off_a_markup_node_and_when_it_says_nothing() {
        let caption = presets::build("funcut.caption.kinetic").expect("a preset");
        let error = apply(
            &caption,
            &[Op::SetMarkup {
                // The caption preset's node is a text node, not markup.
                node: "caption".into(),
                markup: "<div>no</div>".into(),
            }],
        )
        .expect_err("refused");
        assert!(error.to_string().contains("caption"), "{error}");
        let comp = lower_third();
        let error = apply(
            &comp,
            &[Op::SetMarkup {
                node: "card".into(),
                markup: "   ".into(),
            }],
        )
        .expect_err("refused");
        assert!(error.to_string().contains("nothing"), "{error}");
    }

    #[test]
    fn a_fade_becomes_an_opacity_track_relative_to_the_span() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[Op::FadeIn {
                node: "card".into(),
                seconds: 1.5,
                curve: Curve::EASE_OUT,
            }],
        )
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
        let edited = apply(
            &comp,
            &[Op::SlideTo {
                node: "card".into(),
                x: 1920.0,
                y: 400.0,
                seconds: 0.8,
            }],
        )
        .expect("applies");
        let card = edited.node("card").expect("the card");
        assert_eq!(card.transform.position, Some(Point::new(1920.0, 400.0)));
        assert_eq!(
            card.tracks["position"].sample(1.0).unwrap(),
            Value::Point {
                x: 1920.0,
                y: 400.0
            }
        );
    }

    /// The noise op bakes a seeded field into a track: deterministic, dense
    /// enough to read as motion, and bounded by the amount the operation
    /// named - with the comp it leaves behind still validating.
    #[test]
    fn a_noise_track_bakes_a_seeded_field_into_the_keys() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[Op::SetNoiseTrack {
                node: "card".into(),
                property: "scale".into(),
                seed: 77,
                amount: 0.05,
                rate: 10.0,
            }],
        )
        .expect("applies");
        let card = edited.node("card").expect("the card");
        let track = card.tracks.get("scale").expect("a scale track");
        // Dense: ten keys a second over the node's own span.
        assert!(
            track.keys.len() >= 25,
            "sampled at about the rate: {} keys",
            track.keys.len()
        );
        // Bounded: the wander stays inside the resting value ± the amount.
        for key in &track.keys {
            let value = key.value.as_number().expect("a number");
            assert!(
                (0.95..=1.05).contains(&value),
                "the scale wandered past its amount: {value}"
            );
        }
        // Deterministic: the same seed answers the same track, byte for
        // byte, and a different seed is a different path.
        let again = apply(
            &comp,
            &[Op::SetNoiseTrack {
                node: "card".into(),
                property: "scale".into(),
                seed: 77,
                amount: 0.05,
                rate: 10.0,
            }],
        )
        .expect("applies");
        assert_eq!(
            again.node("card").expect("the card").tracks.get("scale"),
            Some(track),
            "the same seed answers the same keys"
        );
        let other = apply(
            &comp,
            &[Op::SetNoiseTrack {
                node: "card".into(),
                property: "scale".into(),
                seed: 78,
                amount: 0.05,
                rate: 10.0,
            }],
        )
        .expect("applies");
        assert_ne!(
            other.node("card").expect("the card").tracks.get("scale"),
            Some(track),
            "a different seed is a different path"
        );
        assert!(edited.validate().is_empty(), "the wobbled comp validates");
    }

    /// Opacity wanders inside its contract: a value that dipped below
    /// zero or climbed past one would be a comp that cannot render, so
    /// the op holds the wander inside the range whatever the field says.
    #[test]
    fn an_opacity_wander_stays_inside_zero_and_one() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[Op::SetNoiseTrack {
                node: "card".into(),
                property: "opacity".into(),
                seed: 9,
                amount: 2.0,
                rate: 6.0,
            }],
        )
        .expect("applies");
        let card = edited.node("card").expect("the card");
        let track = card.tracks.get("opacity").expect("an opacity track");
        for key in &track.keys {
            let value = key.value.as_number().expect("a number");
            assert!(
                (0.0..=1.0).contains(&value),
                "opacity wandered out of range: {value}"
            );
        }
    }

    /// A position wander needs a position to wander around: an unplaced
    /// node (the lower third's card is laid out by its markup) is refused
    /// with the reason, never silently given a place of the operation's
    /// own choosing.
    #[test]
    fn a_position_wander_needs_a_place_to_wander_around() {
        let comp = lower_third();
        let error = apply(
            &comp,
            &[Op::SetNoiseTrack {
                node: "card".into(),
                property: "position".into(),
                seed: 1,
                amount: 4.0,
                rate: 8.0,
            }],
        )
        .expect_err("refused");
        assert!(
            error.to_string().contains("no position"),
            "the error names what is missing: {error}"
        );
    }

    /// An unknown property is refused with the list, and a non-positive
    /// amount likewise - a wander that reaches nowhere is not a wander.
    #[test]
    fn the_noise_op_refuses_what_it_cannot_say() {
        let comp = lower_third();
        let error = apply(
            &comp,
            &[Op::SetNoiseTrack {
                node: "card".into(),
                property: "blur".into(),
                seed: 1,
                amount: 1.0,
                rate: 8.0,
            }],
        )
        .expect_err("refused");
        assert!(
            error.to_string().contains("blur"),
            "the error names the property: {error}"
        );
        let error = apply(
            &comp,
            &[Op::SetNoiseTrack {
                node: "card".into(),
                property: "scale".into(),
                seed: 1,
                amount: 0.0,
                rate: 8.0,
            }],
        )
        .expect_err("refused");
        assert!(
            error.to_string().contains("positive"),
            "the error names the amount: {error}"
        );
    }

    #[test]
    fn deleting_a_parent_takes_its_children() {
        let comp = lower_third();
        let mut with_children = apply(
            &comp,
            &[
                Op::AddGroup {
                    id: "kid".into(),
                    x: 10.0,
                    y: 10.0,
                },
                Op::AddGroup {
                    id: "other".into(),
                    x: 20.0,
                    y: 20.0,
                },
            ],
        )
        .expect("applies");
        let index = with_children
            .nodes
            .iter()
            .position(|node| node.id == "kid")
            .expect("the group");
        with_children.nodes[index].parent = Some("card".into());
        let pruned = apply(
            &with_children,
            &[Op::Delete {
                node: "card".into(),
            }],
        )
        .expect("applies");
        assert!(pruned.node("card").is_none());
        assert!(
            pruned.node("kid").is_none(),
            "the child went with its parent"
        );
        assert!(pruned.node("other").is_some(), "an unrelated node stays");
    }

    #[test]
    fn a_new_node_with_a_taken_id_is_refused() {
        let comp = lower_third();
        let error = apply(
            &comp,
            &[Op::AddRect {
                id: "card".into(),
                width: 10.0,
                height: 10.0,
                fill: "#fff".into(),
                radius: 0.0,
                x: 0.0,
                y: 0.0,
            }],
        )
        .expect_err("refused");
        assert!(error.to_string().contains("already exists"), "{error}");
    }

    #[test]
    fn an_undocumented_property_is_refused_with_the_list() {
        let comp = lower_third();
        let error = apply(
            &comp,
            &[Op::SetTrack {
                node: "card".into(),
                property: "colour".into(),
                keys: vec![(0.0, 1.0, None)],
            }],
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
                assert_eq!(
                    text.words[1].end,
                    Some(edited.duration),
                    "the last cue is closed"
                );
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
        let error = apply_json(
            &lower_third(),
            &serde_json::json!({ "ops": [{ "op": "fly" }] }),
        )
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
                assert_eq!(
                    text.words[2].end,
                    Some(captured.duration),
                    "the last word holds"
                );
            }
            other => panic!("expected words, got {other:?}"),
        }

        let rest: Vec<Op> = ops
            .into_iter()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, op)| op)
            .collect();
        let edited = apply(&lower_third(), &rest).expect("the rest applies");
        assert_eq!(edited.node("card").expect("the card").start, 3.5);
        assert!(edited.node("kicker").is_some(), "add-text landed");
        assert!(edited.validate().is_empty());
    }

    #[test]
    fn a_sample_across_inline_tags_is_replaced_and_the_value_is_escaped() {
        // The design case: a slot whose sample has an emphasis inside it.
        let markup = "<div class=\"ask\">One <em>tap</em> away.</div>";
        assert_eq!(
            replace_text_span(
                markup,
                "One tap away.",
                "Twelve beats. None of them a browser."
            ),
            "<div class=\"ask\">Twelve beats. None of them a browser.</div>"
        );
        // Text broken across lines by the author still matches a flat sample.
        let multiline = "<p>Make\n  it move.</p>";
        assert_eq!(
            replace_text_span(multiline, "Make it move.", "Ship it."),
            "<p>Ship it.</p>"
        );
        // A value is text, never markup: a comp cannot be injected through a slot.
        assert_eq!(
            replace_text_span("<p>x</p>", "x", "<b>bold</b> & co"),
            "<p>&lt;b&gt;bold&lt;/b&gt; &amp; co</p>"
        );
        // A sample that is not there leaves the document alone.
        assert_eq!(replace_text_span("<p>x</p>", "nowhere", "y"), "<p>x</p>");
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
        assert_eq!(
            replace_text_run("<div class='x'></div>", "", "Grace"),
            "<div class='x'></div>"
        );
    }

    #[test]
    fn a_node_with_two_slots_fills_the_one_asked_for() {
        let mut comp = lower_third();
        comp.slots.push(Slot {
            node: "card".into(),
            field: SlotField::Text,
            sample: "Analytical Engine · London, 1843".into(),
            label: "Role".into(),
        });
        let edited = apply(
            &comp,
            &[Op::SetSlot {
                slot: "Role".into(),
                value: "Countess of Lovelace".into(),
            }],
        )
        .expect("applies");
        let NodeKind::Html { markup, .. } = &edited.node("card").expect("card").kind else {
            panic!("markup");
        };
        // The role's run changed and the name's did not: the matched slot's
        // own sample is the needle, not the first one on the node.
        assert!(markup.contains("Countess of Lovelace"), "{markup}");
        assert!(markup.contains("Ada Lovelace"), "{markup}");
        assert!(
            !markup.contains("Analytical Engine"),
            "the role run was left alone: {markup}"
        );
    }

    #[test]
    fn an_open_last_word_closes_at_the_nodes_own_end() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[
                Op::AddText {
                    id: "line".into(),
                    text: "words".into(),
                    size: 40.0,
                    weight: 700,
                    color: "#ffffff".into(),
                    x: 100.0,
                    y: 100.0,
                },
                Op::Move {
                    node: "line".into(),
                    start: 2.0,
                },
                Op::Retime {
                    node: "line".into(),
                    duration: 1.0,
                },
                Op::SetWords {
                    node: "line".into(),
                    words: vec![("hold".into(), 0.2, None)],
                },
            ],
        )
        .expect("applies");
        let NodeKind::Text(text) = &edited.node("line").expect("line").kind else {
            panic!("text");
        };
        // The node runs 1 s of a 6 s comp: the open word closes at 1 s, the
        // node's own end, and not at the comp's 6 s.
        assert_eq!(text.words.last().and_then(|word| word.end), Some(1.0));
    }

    #[test]
    fn set_track_points_writes_both_axes() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[Op::SetTrackPoints {
                node: "card".into(),
                keys: vec![
                    (0.0, 240.0, 400.0, None),
                    (1.0, 960.0, 540.0, Some(Curve::EASE_OUT)),
                ],
            }],
        )
        .expect("applies");
        let card = edited.node("card").expect("card");
        let track = card.tracks.get("position").expect("a position track");
        assert_eq!(
            track.sample(0.0).unwrap(),
            Value::Point { x: 240.0, y: 400.0 }
        );
        assert_eq!(
            track.sample(1.0).unwrap(),
            Value::Point { x: 960.0, y: 540.0 }
        );
    }

    #[test]
    fn deleting_a_child_leaves_no_dangling_id_in_its_group() {
        let mut comp = lower_third();
        comp.nodes.push(Node {
            id: "row".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform::default(),
            opacity: 1.0,
            blend: Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: BTreeMap::new(),
            kind: NodeKind::Group {
                children: vec!["kicker".into()],
            },
        });
        comp.nodes.push(Node {
            id: "kicker".into(),
            parent: Some("row".into()),
            start: 0.0,
            duration: 1.0,
            transform: Transform::default(),
            opacity: 1.0,
            blend: Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: BTreeMap::new(),
            kind: NodeKind::Rect {
                width: Length::px(10.0),
                height: Length::px(10.0),
                fill: None,
                radius: 0.0,
            },
        });
        let edited = apply(
            &comp,
            &[Op::Delete {
                node: "kicker".into(),
            }],
        )
        .expect("applies");
        // The group no longer names the deleted child, so the document is
        // still valid after the delete went through.
        assert!(edited.validate().is_empty(), "{:?}", edited.validate());
        let NodeKind::Group { children } = &edited.node("row").expect("row").kind else {
            panic!("group");
        };
        assert!(!children.iter().any(|child| child == "kicker"));
    }

    #[test]
    fn a_run_replacement_escapes_the_value_like_a_span_does() {
        assert_eq!(
            replace_text_run("<div>Ada</div>", "Ada", "<b>x</b> & co"),
            "<div>&lt;b&gt;x&lt;/b&gt; &amp; co</div>"
        );
    }

    /// The data graphics arrive as ops like everything else: one batch,
    /// applied all-or-nothing, validated at the end - and the camera folds
    /// around the tracks the chart's own entrances already carry.
    #[test]
    fn a_chart_counter_particles_and_a_camera_arrive_as_one_batch() {
        let comp = presets::build("funcut.lower-third.glass").expect("a preset");
        let edited = apply(
            &comp,
            &[
                Op::AddChart {
                    id: "quarter".into(),
                    x: 200.0,
                    y: 200.0,
                    width: 800.0,
                    height: 500.0,
                    at: 0.0,
                    chart: Chart {
                        kind: crate::graphics::ChartKind::Bar,
                        data: vec![crate::graphics::Datum {
                            label: "Q1".into(),
                            value: 3.0,
                            b: None,
                            color: None,
                            open: None,
                            high: None,
                            low: None,
                        }],
                        ..Chart::default()
                    },
                },
                Op::AddCounter {
                    id: "count".into(),
                    x: 1400.0,
                    y: 300.0,
                    at: 0.5,
                    counter: Counter {
                        from: 0.0,
                        to: 42.0,
                        ..Counter::default()
                    },
                },
                Op::AddParticles {
                    id: "dust".into(),
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                    at: 0.0,
                    preset: None,
                    system: Some(crate::particles::ParticlePatch {
                        seed: Some(7),
                        count: Some(8),
                        ..crate::particles::ParticlePatch::default()
                    }),
                },
                Op::SetCamera {
                    camera: CameraMove {
                        zoom: Some((1.0, 1.05)),
                        depths: vec![("card".into(), 1.3)],
                        ..CameraMove::default()
                    },
                },
            ],
        )
        .expect("the batch applies");
        let problems = edited.validate();
        assert!(problems.is_empty(), "{problems:?}");
        // The generated families are root-level nodes the camera reached:
        // the bar's own entrance track is still there, folded around.
        let bar = edited.node("quarter.bar.0").expect("the bar");
        assert!(
            bar.tracks.contains_key("scale"),
            "the entrance track survives the camera: {:?}",
            bar.tracks.keys().collect::<Vec<_>>()
        );
        let scale = bar.tracks.get("scale").expect("folded");
        let first = scale.sample(0.0).expect("keyed");
        let last = scale.sample(1.0).expect("keyed");
        match (first, last) {
            (Value::Number { value: a }, Value::Number { value: b }) => {
                assert!((a - 0.0).abs() < 1e-9, "still enters from nothing: {a}");
                assert!(
                    (b - 1.05).abs() < 1e-9,
                    "and lands on the camera's zoom: {b}"
                );
            }
            _ => panic!("a scale track holds numbers"),
        }
    }

    /// The JSON the documentation shows is the JSON the operations parse:
    /// the worked examples in MOTION.md are a test, not a wish - the bar
    /// and the counter and the particles and the camera, the paired kinds
    /// a compare and a progress, and the candle and the ledger a
    /// candlestick and a table draw.
    #[test]
    fn the_documented_data_graphics_json_parses_as_ops() {
        let comp = presets::build("funcut.lower-third.glass").expect("a preset");
        let batch = serde_json::json!({"ops": [
            {"op": "add-chart", "id": "chart", "x": 220, "y": 180, "width": 1480, "height": 760,
             "chart": {"kind": "bar", "title": "Books finished, by quarter",
                        "format": {"suffix": " books"},
                        "data": [{"label": "Q1", "value": 12}, {"label": "Q2", "value": 9}]}},
            {"op": "add-counter", "id": "metres", "x": 960, "y": 460,
             "counter": {"from": 0, "to": 42195, "seconds": 1.6, "steps": 32,
                          "format": {"suffix": " m"}, "label": "one marathon"}},
            {"op": "add-particles", "id": "embers", "x": 360, "y": 940, "width": 1200, "height": 80,
             "system": {"seed": 7, "count": 48, "lifetime": [1.4, 2.8], "speed": [30, 120],
                         "angle": [-100, -80], "gravity": [0, -60], "drag": 0.4,
                         "turbulence": {"strength": 24, "scale": 0.006, "rate": 0.8},
                         "size": [3, 1.1], "colors": ["#d9a441", "#e8c982"]}},
            {"op": "set-camera", "camera": {"zoom": [1.0, 1.08], "pan": [0, -30], "seconds": 3.5,
                                             "depths": [["card", 1.3]]}}
        ]});
        let edited = apply_json(&comp, &batch).expect("the documented ops parse and apply");
        let problems = edited.validate();
        assert!(problems.is_empty(), "{problems:?}");
        assert!(
            edited.node("chart.bar.1").is_some()
                && edited.node("metres.value.32").is_some()
                && edited.node("embers.p47").is_some(),
            "the generated families are all there"
        );

        // The compare MOTION.md shows: two numbers a category, paired and
        // lettered, its legend named from the series.
        let paired = serde_json::json!({"ops": [
            {"op": "add-chart", "id": "paper", "x": 240, "y": 180, "width": 1440, "height": 760,
             "chart": {"kind": "compare", "title": "Paper against screen",
                        "series": ["paper", "screen"],
                        "data": [{"label": "Q1", "value": 7, "b": 5}, {"label": "Q2", "value": 2, "b": 7},
                                  {"label": "Q3", "value": 6, "b": 8}, {"label": "Q4", "value": 8, "b": 9}]}}
        ]});
        let edited = apply_json(&comp, &paired).expect("the documented compare parses");
        assert!(edited.validate().is_empty(), "{:?}", edited.validate());
        assert!(
            edited.node("paper.a.0").is_some()
                && edited.node("paper.b.3").is_some()
                && edited.node("paper.legend.name.1").is_some(),
            "both sides and the legend are there"
        );

        // The progress MOTION.md shows: a fill toward each row's target,
        // its share lettered through the format.
        let goals = serde_json::json!({"ops": [
            {"op": "add-chart", "id": "drive", "x": 320, "y": 200, "width": 1280, "height": 720,
             "chart": {"kind": "progress", "title": "The pledge drive", "format": {"suffix": "%"},
                        "data": [{"label": "winter beds", "value": 4200, "b": 5000},
                                  {"label": "reading room", "value": 2600, "b": 4000},
                                  {"label": "goalposts", "value": 6100, "b": 5000}]}}
        ]});
        let edited = apply_json(&comp, &goals).expect("the documented progress parses");
        assert!(edited.validate().is_empty(), "{:?}", edited.validate());
        assert!(
            edited.node("drive.track.0").is_some()
                && edited.node("drive.fill.0").is_some()
                && edited.node("drive.share.2").is_some(),
            "the tracks, the fills and the shares are there"
        );

        // The candlestick MOTION.md shows: a period a point, the wick over
        // its range, the body grown from its open.
        let weather = serde_json::json!({"ops": [
            {"op": "add-chart", "id": "roof", "x": 220, "y": 180, "width": 1480, "height": 760,
             "chart": {"kind": "candlestick", "title": "A week on the roof",
                        "format": {"suffix": " °C", "decimals": 0},
                        "data": [{"label": "Mon", "open": 11, "high": 19, "low": 9, "value": 17},
                                  {"label": "Tue", "open": 14, "high": 21, "low": 12, "value": 15},
                                  {"label": "Wed", "open": 15, "high": 16, "low": 10, "value": 11},
                                  {"label": "Thu", "open": 9, "high": 14, "low": 7, "value": 13}]}}
        ]});
        let edited = apply_json(&comp, &weather).expect("the documented candlestick parses");
        assert!(edited.validate().is_empty(), "{:?}", edited.validate());
        assert!(
            edited.node("roof.wick.0").is_some()
                && edited.node("roof.body.1").is_some()
                && edited.node("roof.legend.name.0").is_some(),
            "the wicks, the bodies and the legend are there"
        );

        // The table MOTION.md shows: a row a point, the label left, its
        // number right, ruled like a ledger.
        let standings = serde_json::json!({"ops": [
            {"op": "add-chart", "id": "league", "x": 320, "y": 200, "width": 1280, "height": 720,
             "chart": {"kind": "table", "title": "The pub quiz league",
                        "format": {"suffix": " pts", "decimals": 0},
                        "data": [{"label": "The Buzzer Beaters", "value": 87},
                                  {"label": "Quizzy Riggers", "value": 81},
                                  {"label": "The Decimal Points", "value": 78}]}}
        ]});
        let edited = apply_json(&comp, &standings).expect("the documented table parses");
        assert!(edited.validate().is_empty(), "{:?}", edited.validate());
        assert!(
            edited.node("league.label.0").is_some()
                && edited.node("league.value.2").is_some()
                && edited.node("league.rule.1").is_some(),
            "the rows, the numbers and the rules are there"
        );
    }

    #[test]
    fn remap_turns_time_into_a_property_range() {
        let comp = lower_third();
        let span = comp
            .node("card")
            .map(|node| (node.end(comp.duration) - node.start).max(0.1))
            .expect("the card");
        let edited = apply(
            &comp,
            &[Op::Remap {
                node: "card".into(),
                source: "time".into(),
                target: "scale".into(),
                input: (0.0, span),
                output: (0.8, 1.2),
                clamp: true,
                samples: 3,
                curve: None,
            }],
        )
        .expect("the remap applies");
        let track = edited
            .node("card")
            .and_then(|node| node.tracks.get("scale"))
            .expect("the scale track");
        assert_eq!(track.keys.len(), 3);
        assert!((track.keys[0].value.as_number().unwrap() - 0.8).abs() < 1e-9);
        assert!((track.keys[2].value.as_number().unwrap() - 1.2).abs() < 1e-9);
        assert!((track.sample(0.5).unwrap().as_number().unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn remap_reads_one_numeric_track_into_another() {
        let comp = lower_third();
        let edited = apply(
            &comp,
            &[
                Op::SetTrack {
                    node: "card".into(),
                    property: "opacity".into(),
                    keys: vec![(0.0, 0.0, None), (1.0, 1.0, None)],
                },
                Op::Remap {
                    node: "card".into(),
                    source: "opacity".into(),
                    target: "scale".into(),
                    input: (0.0, 1.0),
                    output: (0.5, 1.5),
                    clamp: true,
                    samples: 3,
                    curve: None,
                },
            ],
        )
        .expect("the remap applies");
        let scale = edited
            .node("card")
            .and_then(|node| node.tracks.get("scale"))
            .expect("the scale track");
        assert!((scale.sample(0.0).unwrap().as_number().unwrap() - 0.5).abs() < 1e-9);
        assert!((scale.sample(0.5).unwrap().as_number().unwrap() - 1.0).abs() < 1e-9);
        assert!((scale.sample(1.0).unwrap().as_number().unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn remap_parses_from_json_with_its_defaults() {
        let comp = lower_third();
        let document = serde_json::json!({"ops": [{
            "op": "remap", "node": "card", "source": "time", "target": "scale",
            "input": [0, 1], "output": [0.8, 1.2]
        }]});
        let edited = apply_json(&comp, &document).expect("the documented remap parses");
        let track = edited
            .node("card")
            .and_then(|node| node.tracks.get("scale"))
            .expect("the scale track");
        assert_eq!(track.keys.len(), 16, "the default sample count");
        assert!((track.keys[0].value.as_number().unwrap() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn remap_refuses_a_missing_source_and_a_position_target() {
        let comp = lower_third();
        let missing = apply(
            &comp,
            &[Op::Remap {
                node: "card".into(),
                source: "nope".into(),
                target: "scale".into(),
                input: (0.0, 1.0),
                output: (0.0, 1.0),
                clamp: true,
                samples: 3,
                curve: None,
            }],
        );
        assert!(missing.is_err(), "a source track the node does not carry");
        let position = apply(
            &comp,
            &[Op::Remap {
                node: "card".into(),
                source: "time".into(),
                target: "position".into(),
                input: (0.0, 1.0),
                output: (0.0, 1.0),
                clamp: true,
                samples: 3,
                curve: None,
            }],
        );
        assert!(position.is_err(), "position needs set-track-points");
    }

    #[test]
    fn a_3d_scene_op_adds_and_replaces() {
        let comp = presets::build("funcut.scene3d.showcase").expect("a preset");
        let added = apply(
            &comp,
            &[Op::AddScene3D {
                id: "scene2".into(),
                preset: Some("product".into()),
                scene: None,
                at: 0.0,
                duration: 2.0,
            }],
        )
        .expect("the op applies");
        assert!(added.validate().is_empty(), "{:?}", added.validate());
        let replaced = apply(
            &comp,
            &[Op::SetScene3D {
                node: "scene".into(),
                scene: crate::scene3d::Scene::product(1920, 1080),
            }],
        )
        .expect("the op applies");
        assert!(replaced.validate().is_empty(), "{:?}", replaced.validate());
    }

    #[test]
    fn the_3d_ops_and_node_spell_their_names_without_a_split_digit() {
        let add = serde_json::to_value(Op::AddScene3D {
            id: "scene".into(),
            preset: Some("showcase".into()),
            scene: None,
            at: 0.0,
            duration: 2.0,
        })
        .expect("the op serialises");
        assert_eq!(add["op"], "add-scene3d");
        let set = serde_json::to_value(Op::SetScene3D {
            node: "scene".into(),
            scene: crate::scene3d::Scene::showcase(1920, 1080),
        })
        .expect("the op serialises");
        assert_eq!(set["op"], "set-scene3d");
        let node = serde_json::to_value(NodeKind::Scene3D {
            scene: crate::scene3d::Scene::showcase(1920, 1080),
        })
        .expect("the node serialises");
        assert_eq!(node["kind"], "scene3d");
    }

    /// A generated family refuses an id already drawn, and a start outside
    /// the comp is refused before the first node is built: the refusals
    /// name the ask, not a symptom.
    #[test]
    fn generated_families_refuse_taken_ids_and_impossible_starts() {
        let comp = presets::build("funcut.lower-third.glass").expect("a preset");
        let taken = apply(
            &comp,
            &[Op::AddChart {
                id: "card".into(),
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 300.0,
                at: 0.0,
                chart: Chart::default(),
            }],
        );
        assert!(taken.is_err(), "an id the comp already carries");
        let outside = apply(
            &comp,
            &[Op::AddCounter {
                id: "late".into(),
                x: 100.0,
                y: 100.0,
                at: 9.0,
                counter: Counter::default(),
            }],
        );
        assert!(outside.is_err(), "a start past the comp's own end");
    }
}
