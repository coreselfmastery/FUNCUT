// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! A motion comp: the document a designer, a template or an agent writes.
//!
//! A comp is a small scene graph - nodes with a span, a transform, tracks and
//! one of a handful of kinds - plus the assets it points at. It is stored as
//! JSON inside the project (`comps/<hash>.json`), addressed by content hash,
//! and baked into an RGBA frame sequence by whichever engine is configured.
//! Nothing here knows how to draw: this module is the format, the validation
//! and the ordering, and that is deliberate - the same document has to render
//! identically through the native path and through a sidecar engine, so the
//! format must not smell of either.
//!
//! The vocabulary is chosen to be a superset of what the two engines can do:
//! grouped transforms, HTML/CSS comps (markup + stylesheet), per-word timed
//! text, vector paths with a dash offset, shapes, images and nested
//! compositions. An engine that cannot draw one of those reports it by node id
//! instead of quietly drawing something else.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::track::{Tracks, Value};

/// The format this document is written in. Bumped only for a breaking change;
/// a reader refuses a version it does not know rather than guessing.
pub const FORMAT: u32 = 1;

/// One comp.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Comp {
    /// The format version this document was written in.
    #[serde(default = "current_format")]
    pub format: u32,
    /// A stable id: what a template, a slot or a clip refers to.
    pub id: String,
    /// What the editor shows in a list.
    #[serde(default)]
    pub name: String,
    /// Frame width the comp is authored at.
    pub width: u32,
    /// Frame height the comp is authored at.
    pub height: u32,
    /// Frame rate the comp is authored at. Times are seconds, so this only
    /// sets the default bake rate.
    #[serde(default = "default_fps")]
    pub fps: f64,
    /// How long the comp runs, in seconds.
    pub duration: f64,
    /// The clear colour. `alpha: 0` is a transparent comp, which is what an
    /// overlay wants.
    #[serde(default = "transparent")]
    pub background: Rgba,
    /// Every file the comp points at, by id. Nodes never carry paths.
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
    /// Named timing functions a node can refer to by name, so a designer's
    /// "brand ease" is defined once.
    #[serde(default)]
    pub curves: BTreeMap<String, Curve>,
    /// The nodes, in paint order: earlier is under later.
    #[serde(default)]
    pub nodes: Vec<Node>,
    /// Marks this comp as a template: slots are nodes whose text or asset a
    /// project fills in (the same idea as `template.json`'s placeholders).
    #[serde(default)]
    pub slots: Vec<Slot>,
}

/// A fillable slot on a template comp.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Slot {
    /// The node the value lands on.
    pub node: String,
    /// What to fill: a text node's words, or an image node's asset.
    pub field: SlotField,
    /// What the editor offers when the project has nothing to offer.
    #[serde(default)]
    pub sample: String,
    /// What the slot is for, shown beside the field.
    #[serde(default)]
    pub label: String,
}

/// Which part of a node a slot fills.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotField {
    /// The words a text node draws.
    Text,
    /// The file an image node shows.
    Image,
}

/// A file the comp uses.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Asset {
    /// Path relative to the comp's own directory. Never absolute.
    pub src: String,
    /// What it is. Inferred from the extension when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AssetKind>,
}

/// The kinds of file a comp can point at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetKind {
    /// A still: png, jpeg, webp.
    Image,
    /// Footage.
    Video,
    /// Sound, mixed by the renderer that bakes the comp.
    Audio,
    /// A font file, pinned so the comp draws the same on every machine.
    Font,
    /// A Lottie or dotLottie animation.
    Lottie,
    /// A stylesheet or markup file an HTML node links.
    Html,
}

/// A colour, straight sRGB with alpha.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Rgba {
    /// Red, 0 to 1.
    pub r: f64,
    /// Green, 0 to 1.
    pub g: f64,
    /// Blue, 0 to 1.
    pub b: f64,
    /// Alpha, 0 to 1.
    pub a: f64,
}

impl Rgba {
    /// A colour from its parts.
    pub const fn new(r: f64, g: f64, b: f64, a: f64) -> Rgba {
        Rgba { r, g, b, a }
    }

    /// A colour from `#rrggbb` or `#rrggbbaa`. Anything else is opaque white,
    /// because a comp must render even when a hand-written document is wrong -
    /// including a document whose six or eight bytes are not characters a
    /// colour could be read from, which this reads as bytes rather than
    /// slicing as text so it cannot panic on one.
    pub fn hex(text: &str) -> Rgba {
        let hex = text.trim_start_matches('#');
        let byte = |at: usize| {
            hex.as_bytes()
                .get(at..at + 2)
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .map(|b| b as f64 / 255.0)
        };
        match hex.len() {
            6 => Rgba::new(
                byte(0).unwrap_or(1.0),
                byte(2).unwrap_or(1.0),
                byte(4).unwrap_or(1.0),
                1.0,
            ),
            8 => Rgba::new(
                byte(0).unwrap_or(1.0),
                byte(2).unwrap_or(1.0),
                byte(4).unwrap_or(1.0),
                byte(6).unwrap_or(1.0),
            ),
            _ => Rgba::new(1.0, 1.0, 1.0, 1.0),
        }
    }

    /// The colour as `#rrggbbaa`, which is what a renderer's document wants.
    pub fn to_hex(self) -> String {
        let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            b(self.r),
            b(self.g),
            b(self.b),
            b(self.a)
        )
    }
}

const fn current_format() -> u32 {
    FORMAT
}

fn default_fps() -> f64 {
    30.0
}

fn transparent() -> Rgba {
    Rgba::new(0.0, 0.0, 0.0, 0.0)
}

/// A length, the way CSS writes one.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Length {
    /// Pixels in the comp's frame.
    Px(f64),
    /// A percentage of the frame or the box, depending on the field.
    Percent(f64),
    /// As wide or tall as the content needs.
    Auto,
}

impl Length {
    /// A length in pixels.
    pub fn px(value: f64) -> Length {
        Length::Px(value)
    }

    /// The length in pixels against `reference`, when it can be resolved.
    pub fn resolve(self, reference: f64) -> Option<f64> {
        match self {
            Length::Px(v) => Some(v),
            Length::Percent(p) => Some(reference * p / 100.0),
            Length::Auto => None,
        }
    }
}

/// Where a node sits, relative to its parent.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Transform {
    /// The anchor point's place in the parent's frame. Defaults to the
    /// parent's centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Point>,
    /// The point of the node's own box that `position` places, as fractions of
    /// the box. Defaults to its centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Point>,
    /// Scale about the anchor.
    #[serde(default = "one")]
    pub scale: f64,
    /// Degrees clockwise about the anchor.
    #[serde(default)]
    pub rotation: f64,
}

fn one() -> f64 {
    1.0
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: None,
            anchor: None,
            scale: 1.0,
            rotation: 0.0,
        }
    }
}

/// A point: pixels for a position, fractions of the box for an anchor.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Point {
    /// Across.
    pub x: f64,
    /// Down.
    pub y: f64,
}

impl Point {
    /// A point from its parts.
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }
}

/// How a node mixes with what is under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Blend {
    /// Straight over.
    #[default]
    Normal,
    /// Multiply the two.
    Multiply,
    /// Add, then clamp.
    Screen,
    /// Contrast-preserving multiply.
    Overlay,
    /// Keep the darker of the two.
    Darken,
    /// Keep the lighter of the two.
    Lighten,
    /// Absolute difference.
    Difference,
    /// A soft version of overlay.
    SoftLight,
    /// Add the two, unclamped - glows and light leaks.
    Add,
}

/// A mask on a node, in the node's own box.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Mask {
    /// A rounded rectangle or an ellipse.
    #[serde(default)]
    pub shape: MaskShape,
    /// Corner radius, for the rectangle.
    #[serde(default)]
    pub radius: f64,
    /// Soft edge width in pixels; `0` is hard.
    #[serde(default)]
    pub feather: f64,
    /// Show what the shape covers rather than what it cuts away.
    #[serde(default)]
    pub invert: bool,
}

/// The shape of a mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaskShape {
    /// A rectangle, rounded by `radius`.
    #[default]
    Rect,
    /// An ellipse inscribed in the box.
    Ellipse,
}

/// An effect on a placed node, sized in output pixels.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Effect {
    /// A Gaussian blur, as CSS `blur()`. Animatable through the `blur` track.
    Blur {
        /// Standard deviation in pixels.
        radius: f64,
    },
    /// A drop shadow behind the node's own alpha.
    Shadow {
        /// How far across.
        x: f64,
        /// How far down.
        y: f64,
        /// Blur radius.
        blur: f64,
        /// The shadow's colour.
        color: Rgba,
    },
    /// A named shader from the project's effect catalogue, keyed by
    /// `param:<key>` tracks.
    Shader {
        /// The package id, e.g. `concat.bloom-pulse`.
        id: String,
        /// Parameters, by key.
        #[serde(default)]
        params: BTreeMap<String, f64>,
    },
}

/// What a node draws.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NodeKind {
    /// Nothing of its own: a transform and a place in the graph for children.
    Group {
        /// Its children.
        #[serde(default)]
        children: Vec<String>,
    },
    /// A rectangle.
    Rect {
        /// Width in pixels or a percentage of the parent.
        width: Length,
        /// Height.
        height: Length,
        /// What it is filled with.
        #[serde(default)]
        fill: Option<Paint>,
        /// Corner radius, clipped to half the shorter side.
        #[serde(default)]
        radius: f64,
    },
    /// An ellipse inscribed in its box.
    Ellipse {
        /// Width.
        width: Length,
        /// Height.
        height: Length,
        /// What it is filled with.
        #[serde(default)]
        fill: Option<Paint>,
    },
    /// A vector path: the shape a lower third's underline or a logo mark is.
    Path {
        /// The outline, in the node's own pixels.
        commands: Vec<PathCommand>,
        /// What the inside is filled with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<Paint>,
        /// The outline's own stroke.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke: Option<Stroke>,
        /// Draw the stroke on over time: `dash` is `(dash, gap)` in pixels and
        /// the `dash` track moves the offset. `None` draws it whole.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dash: Option<(f64, f64)>,
    },
    /// Text, drawn by the comp's renderer. `words` makes it word-timed.
    ///
    /// Boxed because a text node is an order of magnitude bigger than a
    /// rectangle, and every node of every comp pays for the largest variant.
    Text(Box<TextNode>),
    /// Markup: the HTML/CSS comp that gives a design-grade card, chip row or
    /// kinetic title. The native path draws the subset it knows; the HTML
    /// engine draws all of it.
    Html {
        /// The element tree.
        markup: String,
        /// The stylesheet, applied after any `<style>` in the markup.
        #[serde(default)]
        css: String,
        /// Box width: pixels, a percentage, or as wide as the content needs.
        #[serde(default = "auto_length")]
        width: Length,
        /// Box height.
        #[serde(default = "auto_length")]
        height: Length,
    },
    /// A still.
    Image {
        /// Asset id.
        asset: String,
        /// How the picture is fitted into its box.
        #[serde(default)]
        fit: Fit,
    },
    /// A Lottie or dotLottie animation, played against the node's span.
    Lottie {
        /// Asset id.
        asset: String,
        /// Loop it when the span outlasts the animation.
        #[serde(default)]
        r#loop: bool,
    },
    /// Another comp, placed as a node: define a sting once, use it nine times.
    Composition {
        /// The comp's id, which must be reachable by the caller.
        comp: String,
    },
}

fn auto_length() -> Length {
    Length::Auto
}

/// How a picture fills its box.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Scale to fit, keeping the aspect.
    #[default]
    Contain,
    /// Scale to cover, cropping the overflow.
    Cover,
    /// Stretch to the box.
    Fill,
    /// Draw at the picture's own size.
    None,
}

/// What fills a shape.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Paint {
    /// One colour.
    Solid {
        /// The colour.
        color: Rgba,
    },
    /// A gradient, mixed in linear light.
    Gradient {
        /// The stops, 0 to 1 along the ramp.
        stops: Vec<(f64, Rgba)>,
        /// Degrees; `90` runs left to right.
        #[serde(default = "ninety")]
        angle: f64,
        /// The start and end of the ramp's tile, as fractions of the box: a
        /// sweep is a tile twice the text's width moved by a track.
        #[serde(default)]
        from: f64,
        /// The end of the ramp's tile.
        #[serde(default = "one")]
        to: f64,
    },
}

fn ninety() -> f64 {
    90.0
}

/// A stroke on an outline.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Stroke {
    /// The colour.
    pub color: Rgba,
    /// Width in pixels, centred on the outline.
    pub width: f64,
}

/// One command of a path.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum PathCommand {
    /// Start a new subpath here.
    MoveTo {
        /// The point.
        point: Point,
    },
    /// A straight line to here.
    LineTo {
        /// The point.
        point: Point,
    },
    /// A cubic bezier to `point`, with two control points.
    CubicTo {
        /// First control point.
        c1: Point,
        /// Second control point.
        c2: Point,
        /// The end point.
        point: Point,
    },
    /// Close the subpath back to its start.
    Close,
}

/// A text node: a style and some words, with optional per-word timing.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TextNode {
    /// The words, when they are not timed. Ignored when `words` is set.
    #[serde(default)]
    pub text: String,
    /// Timed words, in order: `(word, start, end)` in seconds, relative to the
    /// node. `end: None` holds the word until the next one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
    /// The style of the word showing right now, merged over the base style.
    /// This is what makes karaoke captions a data field rather than an effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<TextStyle>,
    /// Font family name, or an asset id whose family it is.
    #[serde(default)]
    pub font: String,
    /// Size in pixels.
    #[serde(default = "forty_eight")]
    pub size: f64,
    /// Weight, 100 to 900.
    #[serde(default = "four_hundred")]
    pub weight: u32,
    /// False for upright.
    #[serde(default)]
    pub italic: bool,
    /// Letter spacing in pixels.
    #[serde(default)]
    pub letter_spacing: f64,
    /// Line height as a multiple of the size.
    #[serde(default = "one_two")]
    pub line_height: f64,
    /// How the lines sit in the box.
    #[serde(default)]
    pub align: TextAlign,
    /// Wrap width in pixels; the comp's width when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<f64>,
    /// Padding between the words and the plate behind them.
    #[serde(default)]
    pub padding: f64,
    /// A plate behind the words; `alpha: 0` for none.
    #[serde(default = "transparent")]
    pub background: Rgba,
    /// The plate's corner radius.
    #[serde(default)]
    pub radius: f64,
    /// The base colour.
    #[serde(default = "white")]
    pub color: Rgba,
    /// A gradient over the glyphs instead of a flat colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    /// An outline on the glyphs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Stroke>,
    /// A shadow behind the glyphs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<Shadow>,
}

/// Text style overrides a highlight or a slot applies.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct TextStyle {
    /// Overrides the base colour when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgba>,
    /// Overrides the base fill when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    /// Overrides the base weight when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u32>,
    /// Scales the word when set: the "pop" on a karaoke word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

/// One timed word.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Word {
    /// The word, without surrounding space.
    pub text: String,
    /// When it appears, in seconds relative to the node.
    pub start: f64,
    /// When it stops being the current word. `None` means until the next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
}

/// A shadow behind glyphs.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Shadow {
    /// How far across.
    pub x: f64,
    /// How far down.
    pub y: f64,
    /// Blur radius.
    pub blur: f64,
    /// The colour.
    pub color: Rgba,
}

/// How text lines sit in their box.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextAlign {
    /// Flush left.
    Left,
    /// Centred.
    #[default]
    Center,
    /// Flush right.
    Right,
}

fn forty_eight() -> f64 {
    48.0
}

fn four_hundred() -> u32 {
    400
}

fn one_two() -> f64 {
    1.2
}

fn white() -> Rgba {
    Rgba::new(1.0, 1.0, 1.0, 1.0)
}

/// One node of the graph.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Node {
    /// A stable name: what a slot, a track or a diagnostic refers to.
    pub id: String,
    /// The node this one hangs off, when it inherits a transform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// When the node exists, in seconds. It is drawn between `start` and
    /// `start + duration`, and its tracks are relative to that span.
    #[serde(default)]
    pub start: f64,
    /// How long it exists; `0` runs to the end of the comp.
    #[serde(default)]
    pub duration: f64,
    /// Where it sits.
    #[serde(default)]
    pub transform: Transform,
    /// Its own opacity, 0 to 1, before the `opacity` track.
    #[serde(default = "one")]
    pub opacity: f64,
    /// How it mixes with what is under it.
    #[serde(default)]
    pub blend: Blend,
    /// A mask in its own box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Mask>,
    /// Effects, in order, applied after the transform.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// The animatable properties, by name.
    #[serde(default, skip_serializing_if = "Tracks::is_empty")]
    pub tracks: Tracks,
    /// What it draws.
    #[serde(flatten)]
    pub kind: NodeKind,
}

impl Node {
    /// Whether `at` (seconds into the comp) falls inside the node's span.
    pub fn covers(&self, at: f64, comp_duration: f64) -> bool {
        let end = self.end(comp_duration);
        at >= self.start && at < end
    }

    /// The end of the node's span, given how long the comp runs.
    pub fn end(&self, comp_duration: f64) -> f64 {
        if self.duration > 0.0 {
            self.start + self.duration
        } else {
            comp_duration
        }
    }

    /// Where `at` falls in the node's own span, `0..=1`, which is what every
    /// track is keyed against.
    pub fn fraction(&self, at: f64, comp_duration: f64) -> f64 {
        let end = self.end(comp_duration);
        let span = (end - self.start).max(1e-9);
        ((at - self.start) / span).clamp(0.0, 1.0)
    }

    /// A property at `at`, from its track; `fallback` when it has none.
    pub fn property(&self, name: &str, at: f64, comp_duration: f64, fallback: Value) -> Value {
        match self.tracks.get(name) {
            Some(track) => track
                .sample(self.fraction(at, comp_duration))
                .unwrap_or(fallback),
            None => fallback,
        }
    }

    /// The word showing at `at`, for a word-timed text node.
    pub fn current_word(text: &TextNode, at: f64, node_start: f64) -> Option<usize> {
        if text.words.is_empty() {
            return None;
        }
        let local = at - node_start;
        let mut current = None;
        for (i, word) in text.words.iter().enumerate() {
            if local >= word.start {
                let ends = word
                    .end
                    .or_else(|| text.words.get(i + 1).map(|next| next.start))
                    .unwrap_or(f64::INFINITY);
                current = if local < ends { Some(i) } else { current };
            }
        }
        current
    }
}

/// A diagnostic about a comp: what is wrong, where, and what to do.
#[derive(Clone, PartialEq, Debug)]
pub struct Diagnostic {
    /// A JSON pointer into the document, e.g. `/nodes/3/id`.
    pub path: String,
    /// What is wrong.
    pub message: String,
    /// What to do about it, when there is a suggestion.
    pub help: Option<String>,
}

impl Diagnostic {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            path: path.into(),
            message: message.into(),
            help: None,
        }
    }

    fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help = Some(help.into());
        self
    }
}

impl Comp {
    /// The node with this id.
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// The nodes in paint order, with each one's parent already drawn.
    pub fn paint_order(&self) -> Vec<&Node> {
        // The document is authored in paint order; a parent must appear before
        // its children for a transform to inherit, so a stable pass that pulls
        // children after their parents is all this needs.
        let mut ordered: Vec<&Node> = Vec::with_capacity(self.nodes.len());
        let mut placed: BTreeMap<&str, bool> = BTreeMap::new();
        let mut remaining: Vec<&Node> = self.nodes.iter().collect();
        while !remaining.is_empty() {
            let before = remaining.len();
            remaining.retain(|node| {
                let ready = match &node.parent {
                    Some(parent) => placed.contains_key(parent.as_str()),
                    None => true,
                };
                if ready {
                    ordered.push(node);
                    placed.insert(node.id.as_str(), true);
                    false
                } else {
                    true
                }
            });
            if remaining.len() == before {
                // A cycle, or a parent that does not exist: keep the rest in
                // document order rather than dropping it, and say so in
                // `validate`.
                ordered.append(&mut remaining);
                break;
            }
        }
        ordered
    }

    /// Whether anything in this comp only a markup engine can draw.
    ///
    /// True when a node is markup. Everything else - shapes, paths, text,
    /// images, nested comps - the project's own renderer draws.
    pub fn needs_markup(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Html { .. }))
    }

    /// The tracks a node should carry for a property to be animatable, or an
    /// empty map.
    pub fn tracks_of(&self, id: &str) -> Option<&Tracks> {
        self.node(id).map(|node| &node.tracks)
    }

    /// Check the document: ids, parents, spans, assets and slots.
    ///
    /// A comp that fails here is not rendered: a broken document rendered
    /// half-right is worse than one refused with a pointer into it.
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut found = Vec::new();
        if self.format != FORMAT {
            found.push(
                Diagnostic::new(
                    "/format",
                    format!("format {} is not {}", self.format, FORMAT),
                )
                .with_help("re-save the comp, or open it with the version that wrote it"),
            );
        }
        if self.width == 0 || self.height == 0 {
            found.push(Diagnostic::new(
                "/width",
                "a comp needs a frame bigger than zero",
            ));
        }
        if self.duration <= 0.0 {
            found.push(Diagnostic::new("/duration", "a comp needs a length"));
        }
        if self.fps <= 0.0 {
            found.push(Diagnostic::new("/fps", "a comp needs a frame rate"));
        }
        if self.nodes.is_empty() {
            found.push(Diagnostic::new(
                "/nodes",
                "a comp with no nodes draws nothing",
            ));
        }

        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.id.trim().is_empty() {
                found.push(Diagnostic::new(
                    format!("/nodes/{i}/id"),
                    "every node needs an id",
                ));
            }
            if let Some(first) = seen.insert(node.id.as_str(), i) {
                found.push(
                    Diagnostic::new(format!("/nodes/{i}/id"), format!("id {:?} is already used by node {first}", node.id))
                        .with_help("ids are what tracks, slots and diagnostics point at, so they must be unique"),
                );
            }
            if node.start < 0.0 {
                found.push(Diagnostic::new(
                    format!("/nodes/{i}/start"),
                    "a node cannot start before the comp",
                ));
            }
            if node.duration < 0.0 {
                found.push(Diagnostic::new(
                    format!("/nodes/{i}/duration"),
                    "a node cannot run backwards",
                ));
            }
            if let Some(parent) = &node.parent {
                match self.node(parent) {
                    None => found.push(Diagnostic::new(
                        format!("/nodes/{i}/parent"),
                        format!("no node called {parent:?}"),
                    )),
                    Some(parent_node) if parent_node.id == node.id => found.push(Diagnostic::new(
                        format!("/nodes/{i}/parent"),
                        "a node cannot be its own parent",
                    )),
                    Some(_) => {}
                }
            }
            if let NodeKind::Group { children } = &node.kind {
                for child in children {
                    if self.node(child).is_none() {
                        found.push(Diagnostic::new(
                            format!("/nodes/{i}/children"),
                            format!("no node called {child:?}"),
                        ));
                    }
                }
            }
            if let NodeKind::Image { asset, .. } | NodeKind::Lottie { asset, .. } = &node.kind
                && !self.assets.contains_key(asset)
            {
                found.push(Diagnostic::new(
                    format!("/nodes/{i}/asset"),
                    format!("no asset called {asset:?}"),
                ));
            }
            for (name, track) in &node.tracks {
                if track.is_empty() {
                    found.push(Diagnostic::new(
                        format!("/nodes/{i}/tracks/{name}"),
                        "a track with no keys animates nothing",
                    ));
                }
                if track.keys.iter().any(|key| !(0.0..=1.0).contains(&key.at)) {
                    found.push(
                        Diagnostic::new(
                            format!("/nodes/{i}/tracks/{name}"),
                            "keys are placed as fractions of the node's span, 0 to 1",
                        )
                        .with_help("a key at 1.5s of a 1s node is 1.5, not 1500"),
                    );
                }
            }
        }

        for asset in self.assets.iter() {
            if asset.1.src.starts_with('/') || asset.1.src.contains("..") {
                found.push(
                    Diagnostic::new(
                        format!("/assets/{}", asset.0),
                        "an asset path is relative to the comp, and stays inside it",
                    )
                    .with_help("no leading /, no ..: the comp has to travel with its files"),
                );
            }
        }

        for (i, slot) in self.slots.iter().enumerate() {
            if self.node(&slot.node).is_none() {
                found.push(Diagnostic::new(
                    format!("/slots/{i}/node"),
                    format!("no node called {:?}", slot.node),
                ));
            }
        }
        found
    }

    /// A content hash: the same comp, the same number, on every machine.
    ///
    /// FNV-1a over the document's canonical JSON. It is the cache key for a
    /// bake, so an unchanged comp re-renders nothing and a changed one misses
    /// immediately. Not a security hash - nothing here is adversarial.
    pub fn hash(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        eat(b"concat-motion/1");
        match serde_json::to_string(self) {
            Ok(json) => eat(json.as_bytes()),
            Err(_) => eat(self.id.as_bytes()),
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_comp() -> Comp {
        Comp {
            format: FORMAT,
            id: "test.card".into(),
            name: "Card".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 5.0,
            background: transparent(),
            assets: BTreeMap::new(),
            curves: BTreeMap::new(),
            nodes: vec![Node {
                id: "card".into(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform::default(),
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                effects: Vec::new(),
                tracks: Tracks::new(),
                kind: NodeKind::Rect {
                    width: Length::px(200.0),
                    height: Length::px(80.0),
                    fill: Some(Paint::Solid {
                        color: Rgba::hex("#22d3c5"),
                    }),
                    radius: 8.0,
                },
            }],
            slots: Vec::new(),
        }
    }

    #[test]
    fn a_comp_round_trips_through_json() {
        let comp = rect_comp();
        let json = serde_json::to_string(&comp).expect("serialises");
        let back: Comp = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, comp);
    }

    #[test]
    fn a_valid_comp_finds_nothing_wrong() {
        assert!(
            rect_comp().validate().is_empty(),
            "{:?}",
            rect_comp().validate()
        );
    }

    #[test]
    fn a_duplicate_id_is_reported_with_a_pointer() {
        let mut comp = rect_comp();
        comp.nodes.push(comp.nodes[0].clone());
        let found = comp.validate();
        assert!(found.iter().any(|d| d.path == "/nodes/1/id"), "{found:?}");
    }

    #[test]
    fn a_missing_asset_is_reported() {
        let mut comp = rect_comp();
        comp.nodes[0].kind = NodeKind::Image {
            asset: "nope".into(),
            fit: Fit::Cover,
        };
        assert!(
            comp.validate()
                .iter()
                .any(|d| d.message.contains("no asset"))
        );
    }

    #[test]
    fn an_absolute_asset_path_is_refused() {
        let mut comp = rect_comp();
        comp.assets.insert(
            "a".into(),
            Asset {
                src: "/etc/passwd".into(),
                kind: None,
            },
        );
        assert!(comp.validate().iter().any(|d| d.path == "/assets/a"));
    }

    #[test]
    fn keys_outside_the_span_are_refused() {
        let mut comp = rect_comp();
        let mut track = crate::track::Track::default();
        track
            .keys
            .push(crate::track::Key::new(1.5, Value::number(1.0)));
        comp.nodes[0].tracks.insert("opacity".into(), track);
        assert!(
            comp.validate()
                .iter()
                .any(|d| d.message.contains("fractions"))
        );
    }

    #[test]
    fn the_hash_follows_the_content_and_not_the_order_of_edits() {
        let comp = rect_comp();
        let mut edited = comp.clone();
        edited.name = "Other".into();
        assert_eq!(comp.hash(), rect_comp().hash());
        assert_ne!(comp.hash(), edited.hash());
    }

    #[test]
    fn a_child_is_painted_after_its_parent_whatever_the_document_says() {
        let mut comp = rect_comp();
        let child = Node {
            id: "child".into(),
            parent: Some("card".into()),
            ..comp.nodes[0].clone()
        };
        comp.nodes.insert(0, child);
        let order: Vec<&str> = comp.paint_order().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(order, vec!["card", "child"]);
    }

    #[test]
    fn the_word_showing_at_a_time_is_the_one_inside_its_cue() {
        let text = TextNode {
            words: vec![
                Word {
                    text: "make".into(),
                    start: 0.0,
                    end: Some(0.5),
                },
                Word {
                    text: "it".into(),
                    start: 0.5,
                    end: None,
                },
                Word {
                    text: "move".into(),
                    start: 1.2,
                    end: None,
                },
            ],
            ..default_text()
        };
        assert_eq!(Node::current_word(&text, 0.2, 0.0), Some(0));
        assert_eq!(Node::current_word(&text, 0.7, 0.0), Some(1));
        assert_eq!(Node::current_word(&text, 1.3, 0.0), Some(2));
        assert_eq!(Node::current_word(&text, 0.0, 2.0), None);
    }

    fn default_text() -> TextNode {
        TextNode {
            text: String::new(),
            words: Vec::new(),
            highlight: None,
            font: "Inter".into(),
            size: 48.0,
            weight: 400,
            italic: false,
            letter_spacing: 0.0,
            line_height: 1.2,
            align: TextAlign::Center,
            max_width: None,
            padding: 0.0,
            background: transparent(),
            radius: 0.0,
            color: white(),
            fill: None,
            outline: None,
            shadow: None,
        }
    }

    #[test]
    fn a_node_span_is_respected() {
        let mut comp = rect_comp();
        comp.nodes[0].start = 1.0;
        comp.nodes[0].duration = 2.0;
        let node = &comp.nodes[0];
        assert!(!node.covers(0.5, 5.0));
        assert!(node.covers(1.5, 5.0));
        assert!(!node.covers(3.5, 5.0));
        assert!((node.fraction(1.5, 5.0) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn an_open_ended_node_runs_to_the_end_of_the_comp() {
        let comp = rect_comp();
        assert_eq!(comp.nodes[0].end(7.0), 7.0);
    }

    #[test]
    fn hex_reads_bytes_not_characters_and_never_panics() {
        // Six bytes that are not six characters a colour could be read from:
        // opaque white, the documented answer for anything else - and no
        // panic on a boundary that is not a character's edge.
        assert_eq!(Rgba::hex("#日本"), Rgba::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(Rgba::hex("#日"), Rgba::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(Rgba::hex("#zzzzzz"), Rgba::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(
            Rgba::hex("#22d3c5"),
            Rgba::new(
                0x22 as f64 / 255.0,
                0xd3 as f64 / 255.0,
                0xc5 as f64 / 255.0,
                1.0
            )
        );
    }
}
