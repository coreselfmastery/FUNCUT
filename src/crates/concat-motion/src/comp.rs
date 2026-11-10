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
    /// Sub-compositions this comp carries, by name: whole comp
    /// documents a [`NodeKind::Composition`] node places, any number of
    /// times, each placement with its own timing and transform - define a
    /// sting once, use it nine times.
    ///
    /// Each is a **scope of its own**: its node ids, assets, curves and
    /// slots belong to it, a parent and a child may both have a node
    /// called `card` and an asset called `logo`, and a diagnostic inside
    /// one is pointed at `/comps/{name}/...` - the same document-in-a-
    /// document rule the timeline engine holds (its nesting cap is eight
    /// levels, and a comp that contains itself through any chain is
    /// refused, here first and by the engine's E207 after).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub comps: BTreeMap<String, Comp>,
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
    /// A rounded rectangle, an ellipse, or a vector path.
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
    /// Grow (`> 0`) or shrink (`< 0`) the shape by this many pixels before
    /// the feather softens it. `0` leaves the shape as authored.
    #[serde(default)]
    pub expansion: f64,
}

/// The shape of a mask.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaskShape {
    /// A rectangle, rounded by `radius`.
    #[default]
    Rect,
    /// An ellipse inscribed in the box.
    Ellipse,
    /// A vector path in the node's own pixels, filled as a stencil.
    Path {
        /// The outline.
        commands: Vec<PathCommand>,
    },
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
    /// A glow halo around the node's own alpha, screened over what is
    /// beneath it.
    Glow {
        /// The halo's blur radius in pixels.
        radius: f64,
        /// How strong the halo is, `0` to `1` and beyond, over the node's
        /// own colour.
        intensity: f64,
        /// The halo's colour.
        color: Rgba,
    },
    /// Chromatic aberration: the red and blue channels pulled apart from
    /// the green, radially out from the node's centre.
    Aberration {
        /// How far the outermost channels drift, in pixels at the frame's
        /// widest corner.
        amount: f64,
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
        /// Draw only `start..end` of the path, as fractions of its own
        /// length, for line reveals and draw-on animation; the `trim` track
        /// (a point: start, end) moves them. `None` draws it whole.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trim: Option<(f64, f64)>,
        /// A second outline this one becomes: the `morph` track (a number
        /// `0..=1`) blends the two, command for command where they agree and
        /// point for point where they do not. `None` never morphs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        morph: Option<Vec<PathCommand>>,
    },
    /// An adjustment: a span of time whose `effects` are applied to
    /// everything painted beneath it in this comp, the way a colourist
    /// grades a stack. Draws nothing of its own.
    Adjustment,
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
        /// Which ramp: a straight line or a disc.
        #[serde(default)]
        ramp: GradientKind,
        /// Degrees; `90` runs left to right. The angle the line runs at, or
        /// the disc's brightest axis for a radial ramp.
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

/// Which ramp a gradient is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GradientKind {
    /// A straight ramp across the box, at `angle` degrees.
    #[default]
    Linear,
    /// A disc: bright at the box's centre, out to its corner.
    Radial,
}

/// Another node's alpha (or luminance) used as this one's stencil.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Matte {
    /// The node whose pixels stencil this one. It must exist, and it draws
    /// nothing of its own while it is a matte.
    pub source: String,
    /// Which channel of the source stencils, and whether it is kept or
    /// cut away.
    #[serde(default)]
    pub mode: MatteMode,
}

/// How a matte source is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatteMode {
    /// Keep this node where the source is opaque.
    #[default]
    Alpha,
    /// Keep it where the source is transparent.
    AlphaInverted,
    /// Keep it where the source is bright.
    Luma,
    /// Keep it where the source is dark.
    LumaInverted,
}

/// Multi-sample motion blur.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct MotionBlur {
    /// How many positions across the shutter are rendered and averaged.
    /// `2` is the cheapest smear that is not a fade; `32` is cinema.
    #[serde(default = "eight")]
    pub samples: u8,
    /// How wide the shutter is, as a fraction of one frame's interval:
    /// `0.5` is the 180-degree film shutter, `1.0` a whole frame.
    #[serde(default = "half")]
    pub shutter: f64,
}

fn eight() -> u8 {
    8
}

fn half() -> f64 {
    0.5
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

impl Default for TextNode {
    /// The same text a document without the fields parses: every default
    /// here is the field's own `#[serde(default)]`, so a node built in
    /// code and a node read from JSON agree without being told to.
    fn default() -> Self {
        TextNode {
            text: String::new(),
            words: Vec::new(),
            highlight: None,
            font: String::new(),
            size: forty_eight(),
            weight: four_hundred(),
            italic: false,
            letter_spacing: 0.0,
            line_height: one_two(),
            align: TextAlign::Left,
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
    /// Another node used as a stencil for this one's alpha, after its own
    /// mask: the text-through-a-shape and holdout-matte moves of the
    /// compositor's kit. The source draws nothing of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matte: Option<Matte>,
    /// Effects, in order, applied after the transform.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// Multi-sample motion blur for this node and everything under it:
    /// `samples` positions across the shutter are rendered and averaged, so
    /// a fast move smears instead of strobing. `None` is a pin-sharp render.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_blur: Option<MotionBlur>,
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

    /// The capabilities this comp uses that a Geneva timeline cannot draw,
    /// named for the note that says why a bake chose the native engine.
    ///
    /// Empty for a comp Geneva can draw as written - which is every comp of
    /// rectangles, ellipses, text, markup, images and sub-comps, animated
    /// through the tracks Geneva speaks. A comp that carries any of the
    /// features below is drawn by the native engine instead, *with Geneva
    /// installed or without it*: routing it to Geneva would mean the
    /// warn-and-drop path, where a logo mark quietly vanishes because the
    /// engine has no path source - the silent degradation the architecture
    /// refuses. Sub-compositions are walked too, because a placement is
    /// rendered by the same engine its parent is.
    ///
    /// Not listed: things neither engine draws (shader effects, Lottie
    /// nodes), which stay on the engine that would otherwise have drawn the
    /// comp and carry their warning there - routing them would trade a
    /// warning for a different warning.
    pub fn geneva_inexpressible(&self) -> Vec<&'static str> {
        let mut found: Vec<&'static str> = Vec::new();
        self.inexpressible_in(&mut found);
        found
    }

    /// The walk behind [`Comp::geneva_inexpressible`], shared by every
    /// scope the document carries.
    fn inexpressible_in(&self, found: &mut Vec<&'static str>) {
        let mut note = |name: &'static str| {
            if !found.contains(&name) {
                found.push(name);
            }
        };
        for node in &self.nodes {
            match &node.kind {
                NodeKind::Path { .. } => note("a vector path"),
                NodeKind::Adjustment => note("an adjustment layer"),
                NodeKind::Rect { fill, .. } | NodeKind::Ellipse { fill, .. } => {
                    if matches!(fill, Some(Paint::Gradient { .. })) {
                        note("a shape gradient");
                    }
                }
                _ => {}
            }
            if let Some(mask) = &node.mask {
                if matches!(mask.shape, MaskShape::Path { .. }) {
                    note("a path mask");
                }
                if mask.expansion != 0.0 {
                    note("mask expansion");
                }
            }
            if node.matte.is_some() {
                note("a track matte");
            }
            for effect in &node.effects {
                match effect {
                    Effect::Shadow { .. } => note("a drop shadow effect"),
                    Effect::Glow { .. } => note("a glow effect"),
                    Effect::Aberration { .. } => note("a chromatic aberration effect"),
                    _ => {}
                }
            }
            for name in node.tracks.keys() {
                if matches!(name.as_str(), "dash" | "trim" | "morph") {
                    note(match name.as_str() {
                        "dash" => "a dash track",
                        "trim" => "a trim track",
                        _ => "a morph track",
                    });
                }
            }
        }
        for sub in self.comps.values() {
            sub.inexpressible_in(found);
        }
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
        // The trail opens with this document's own id: a sub-comp whose id
        // is already on it is a placement that comes back around, which is
        // the cycle the engine refuses as E207 and this refuses first,
        // with a pointer into the document the engine cannot give.
        let mut trail = vec![self.id.clone()];
        self.validate_in("", &mut trail)
    }

    /// The same validation, rooted at `prefix` and walking `trail`: the
    /// document's own findings carry the prefix, its sub-comps' findings
    /// carry `/comps/{name}` on top of it, and the trail is what a cycle
    /// and the nesting cap are read from. Every sub-comp is validated,
    /// placed or not - each is emitted to the engine, so each has to be a
    /// document the engine accepts.
    fn validate_in(&self, prefix: &str, trail: &mut Vec<String>) -> Vec<Diagnostic> {
        let mut found = self.validate_document();
        for problem in &mut found {
            problem.path = format!("{prefix}{}", problem.path);
        }
        for (name, sub) in &self.comps {
            let path = format!("{prefix}/comps/{name}");
            if trail.contains(&sub.id) {
                found.push(
                    Diagnostic::new(
                        path.clone(),
                        format!(
                            "the comp {:?} contains itself: {} places {:?}, which is already \
                             on the chain {:?}",
                            sub.id, path, sub.id, trail
                        ),
                    )
                    .with_help(
                        "a scene cannot be built from itself at any depth; the engine \
                         refuses it as E207 too",
                    ),
                );
                continue;
            }
            if trail.len() >= 8 {
                found.push(
                    Diagnostic::new(
                        path.clone(),
                        format!(
                            "the comp {:?} sits deeper than the engine's eight levels of \
                             nesting",
                            sub.id
                        ),
                    )
                    .with_help("lift the deepest level out, or inline it as nodes"),
                );
                continue;
            }
            trail.push(sub.id.clone());
            found.extend(sub.validate_in(&path, trail));
            trail.pop();
        }
        found
    }

    /// This document's own findings, in its own scope's paths.
    fn validate_document(&self) -> Vec<Diagnostic> {
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
            if let NodeKind::Path {
                commands,
                morph,
                trim,
                ..
            } = &node.kind
            {
                if commands.is_empty() {
                    found.push(Diagnostic::new(
                        format!("/nodes/{i}/commands"),
                        "a path with no commands draws nothing",
                    ));
                }
                if let Some(target) = morph
                    && target.is_empty()
                {
                    found.push(Diagnostic::new(
                        format!("/nodes/{i}/morph"),
                        "a morph target with no commands morphs into nothing",
                    ));
                }
                if let Some((start, end)) = trim {
                    let (start, end) = (*start, *end);
                    let misplacement =
                        if !(0.0..=1.0).contains(&start) || !(0.0..=1.0).contains(&end) {
                            Some("a trim is placed as a fraction of the path's length, 0 to 1")
                        } else if start > end {
                            Some("a trim runs backwards: trim is (start, end)")
                        } else {
                            None
                        };
                    if let Some(problem) = misplacement {
                        found.push(
                            Diagnostic::new(format!("/nodes/{i}/trim"), problem).with_help(
                                "trim is (start, end) as fractions of the path's length",
                            ),
                        );
                    }
                }
            }
            // A matte names another node, and the pair is a stencil: the
            // source is read, not drawn, while it is a matte. The two
            // checks the pairing needs are the ones a parent needs - it
            // exists, and it is not the node itself.
            if let Some(matte) = &node.matte {
                match self.node(&matte.source) {
                    None => found.push(Diagnostic::new(
                        format!("/nodes/{i}/matte"),
                        format!("no node called {:?}", matte.source),
                    )),
                    Some(source) if source.id == node.id => found.push(Diagnostic::new(
                        format!("/nodes/{i}/matte"),
                        "a node cannot matte itself",
                    )),
                    Some(_) => {}
                }
            }
            // Motion blur is bounded the way the renderer samples it: a
            // shutter wider than one frame would smear into the next one,
            // and fewer than two samples is a pin-sharp render that pays
            // for the field anyway.
            if let Some(blur) = &node.motion_blur
                && (!(2..=32).contains(&blur.samples) || !(0.0..=1.0).contains(&blur.shutter))
            {
                found.push(
                    Diagnostic::new(
                        format!("/nodes/{i}/motion-blur"),
                        format!(
                            "motion blur samples {} and shutter {} are outside 2..=32 and 0..=1",
                            blur.samples, blur.shutter
                        ),
                    )
                    .with_help("samples 8 and shutter 0.5 is the 180-degree film look"),
                );
            }
            // A placement names a comp this document carries: the scoping
            // rule. A name that is not in `comps` is a broken template -
            // and pointing at another document's comp by id is exactly the
            // cross-scope leak the rule exists to refuse.
            if let NodeKind::Composition { comp } = &node.kind
                && !self.comps.contains_key(comp)
            {
                found.push(
                    Diagnostic::new(
                        format!("/nodes/{i}/comp"),
                        format!("no sub-composition called {comp:?}"),
                    )
                    .with_help(
                        "a composition node places a comp this document carries under `comps`",
                    ),
                );
            }
            // The stylesheet a markup node carries is read for the one
            // thing the engine stays quiet about: a transform as a base
            // style is skipped, not drawn, and everything built on it is
            // then wrong while looking valid. `css::static_transforms`
            // names the rule and the property, and the comp is refused
            // here - before the bake - with the pointer to fix it.
            if let NodeKind::Html { markup, css, .. } = &node.kind {
                for hit in crate::css::static_transforms(css)
                    .into_iter()
                    .chain(style_blocks(markup).flat_map(crate::css::static_transforms))
                {
                    found.push(
                        Diagnostic::new(
                            format!("/nodes/{i}/css"),
                            format!(
                                "the rule for {:?} sets {:?} outside a @keyframes block, \
                                 and the markup engine skips a static transform",
                                hit.selector, hit.property
                            ),
                        )
                        .with_help(
                            "move the transform into a @keyframes rule the node's animation \
                             plays, or place the layer with the node's own transform, which \
                             the timeline carries first-class",
                        ),
                    );
                }
                // The weight trap, found the same way: the engine cannot
                // animate a weight (font-variation-settings and
                // font-weight in a @keyframes rule are E442) and does not
                // draw font-variation-settings at all (W450), so a comp
                // that either asks for one is refused here - before the
                // bake - with the one weight that works in the message.
                for hit in crate::css::refused_weights(css)
                    .into_iter()
                    .chain(style_blocks(markup).flat_map(crate::css::refused_weights))
                {
                    let where_it_sits = if hit.animated {
                        format!(
                            "the rule {} sets {:?} inside a @keyframes block, \
                             and the markup engine refuses to animate a weight",
                            hit.selector, hit.property
                        )
                    } else {
                        format!(
                            "the rule for {:?} sets font-variation-settings as a base \
                             style, and the markup engine does not draw it: a variable \
                             font's axis is never read",
                            hit.selector
                        )
                    };
                    found.push(
                        Diagnostic::new(format!("/nodes/{i}/css"), where_it_sits).with_help(
                            "write the weight as a static font-weight on the element - \
                             the engine snaps it to the family's own instances and draws \
                             it; a ramp across staggered units is what typography's \
                             flexing_weight builds",
                        ),
                    );
                }
                // The determinism contract, held on the document: a fetch
                // the render would make, a stylesheet it would import, or a
                // value the clock or an unseeded random would be asked for -
                // any of them makes the render's bytes the machine's mood,
                // not the document's. Refused before the bake, with the
                // token that broke the promise.
                for hit in crate::css::network_refs(css)
                    .into_iter()
                    .chain(style_blocks(markup).flat_map(crate::css::network_refs))
                    .chain(crate::css::network_refs(&markup_outside_styles(markup)))
                {
                    found.push(
                        Diagnostic::new(
                            format!("/nodes/{i}/markup"),
                            format!(
                                "the markup carries {} ({:?}), and the determinism \
                                 contract refuses it: the same comp must be the same \
                                 bytes on every machine, at any time, online or not",
                                hit.kind, hit.text
                            ),
                        )
                        .with_help(
                            "stage the file as an asset the comp names by id, inline it \
                             as a data: URL, or bake the numbers into the document - \
                             set-noise-track carries a seed for exactly this",
                        ),
                    );
                }
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

/// The `<style>` blocks of a markup string, as stylesheet text: the comp's
/// own `css` field is applied *after* these, but the engine reads both, so
/// a static transform hiding in either is the same skipped layer.
fn style_blocks(markup: &str) -> impl Iterator<Item = &str> {
    let mut rest = markup;
    std::iter::from_fn(move || {
        let open = rest.find("<style")?;
        let after_tag = rest[open..].find('>')? + open + 1;
        let body = &rest[after_tag..];
        let close = body.find("</style>")?;
        let css = &body[..close];
        rest = &body[close + "</style>".len()..];
        Some(css)
    })
}

/// The markup with its `<style>` blocks blanked to whitespace, so the
/// attribute and script scan reads the document *around* the styles -
/// the styles themselves are scanned as CSS, once, on their own.
fn markup_outside_styles(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    loop {
        let Some(open) = rest.find("<style") else {
            out.push_str(rest);
            return out;
        };
        let after_tag = rest[open..]
            .find('>')
            .map(|at| open + at + 1)
            .unwrap_or(open);
        out.push_str(&rest[..after_tag]);
        let body = &rest[after_tag..];
        let Some(close) = body.find("</style>") else {
            out.push_str(body);
            return out;
        };
        out.push_str(&" ".repeat(close));
        out.push_str("</style>");
        rest = &body[close + "</style>".len()..];
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
            comps: Default::default(),
            nodes: vec![Node {
                id: "card".into(),
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

    /// The one trap the engine never reports: a transform as a base style
    /// is skipped, not drawn. The comp is refused here, naming the rule
    /// and the property, with the pointer a fix needs.
    #[test]
    fn a_static_transform_in_a_stylesheet_is_refused_and_named() {
        let mut comp = rect_comp();
        comp.nodes[0].kind = NodeKind::Html {
            markup: "<div class=\"layer\"></div>".into(),
            css: ".layer { transform: translateX(120px); }".into(),
            width: Length::Auto,
            height: Length::Auto,
        };
        let found = comp.validate();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].path, "/nodes/0/css");
        assert!(found[0].message.contains(".layer"), "{:?}", found[0]);
        assert!(found[0].message.contains("transform"), "{:?}", found[0]);
        assert!(found[0].message.contains("@keyframes"), "{:?}", found[0]);
        assert!(found[0].help.is_some(), "the fix is said, not guessed");
    }

    /// The same trap inside a `<style>` block of the markup itself, which
    /// the engine reads exactly as it reads the stylesheet field.
    #[test]
    fn a_static_transform_inside_a_style_block_is_refused_too() {
        let mut comp = rect_comp();
        comp.nodes[0].kind = NodeKind::Html {
            markup: "<style>.layer { translate: -100% 0; }</style><div></div>".into(),
            css: String::new(),
            width: Length::Auto,
            height: Length::Auto,
        };
        let found = comp.validate();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("translate"), "{:?}", found[0]);
    }

    /// The library's answer to the trap: the transform inside the
    /// `@keyframes` rule the animation plays is the right way, and a comp
    /// written that way validates clean.
    #[test]
    fn the_same_transform_inside_keyframes_validates_clean() {
        let mut comp = rect_comp();
        comp.nodes[0].kind = NodeKind::Html {
            markup: "<div class=\"layer\"></div>".into(),
            css: ".layer { animation: slide 0.5s ease-out; }\n\
                  @keyframes slide { from { translate: -100% 0 } to { translate: 0 0 } }"
                .into(),
            width: Length::Auto,
            height: Length::Auto,
        };
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());
    }

    /// The weight trap: a `font-variation-settings` inside a `@keyframes`
    /// rule is refused by the engine only at the bake (E442), after the
    /// comp validated everywhere else. The comp is refused here, naming
    /// the rule and the property, with the static weight that works as
    /// the fix.
    #[test]
    fn an_animated_weight_in_a_stylesheet_is_refused_and_named() {
        let mut comp = rect_comp();
        comp.nodes[0].kind = NodeKind::Html {
            markup: "<div class=\"headline\"></div>".into(),
            css: ".headline { animation: flex 0.9s both; }\n\
                  @keyframes flex { from { font-variation-settings: 'wght' 100 } \
                  to { font-variation-settings: 'wght' 900 } }"
                .into(),
            width: Length::Auto,
            height: Length::Auto,
        };
        let found = comp.validate();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].path, "/nodes/0/css");
        assert!(
            found[0].message.contains("@keyframes flex"),
            "{:?}",
            found[0]
        );
        assert!(
            found[0].message.contains("font-variation-settings"),
            "{:?}",
            found[0]
        );
        assert!(found[0].help.is_some(), "the fix is said, not guessed");
    }

    /// The quieter half of the same trap: a `font-variation-settings` as
    /// a base style is not drawn at all (W450), so the variable axis is
    /// never read and the text renders at the family's default instance
    /// while the comp looks exactly as authored.
    #[test]
    fn a_base_style_variation_setting_is_refused_too() {
        let mut comp = rect_comp();
        comp.nodes[0].kind = NodeKind::Html {
            markup: "<div class=\"word\"></div>".into(),
            css: ".word { font-variation-settings: 'wght' 650; }".into(),
            width: Length::Auto,
            height: Length::Auto,
        };
        let found = comp.validate();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].message.contains("font-variation-settings"),
            "{:?}",
            found[0]
        );
    }

    /// The answer to the trap: a static `font-weight`, which the engine
    /// snaps to the family's own instances and draws, validates clean -
    /// and it is the very thing `typography::flexing_weight` writes per
    /// staggered unit.
    #[test]
    fn a_static_font_weight_validates_clean() {
        let mut comp = rect_comp();
        comp.nodes[0].kind = NodeKind::Html {
            markup: "<div class=\"headline\"><span style=\"font-weight: 100\">Make</span>\
                     <span style=\"font-weight: 900\">move</span></div>"
                .into(),
            css: String::new(),
            width: Length::Auto,
            height: Length::Auto,
        };
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());
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

    // ── Sub-compositions: the scoped document-in-a-document ────────────

    /// A placement names a comp this document carries: the scoping rule.
    /// A name that is not in `comps` is refused with the pointer, and a
    /// name that is validates clean - placed once or nine times.
    #[test]
    fn a_placement_names_a_comp_the_document_carries() {
        let mut comp = rect_comp();
        comp.nodes.push(Node {
            id: "place".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform::default(),
            opacity: 1.0,
            blend: crate::comp::Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind: NodeKind::Composition {
                comp: "sting".into(),
            },
        });
        let missing = comp.validate();
        assert!(
            missing.iter().any(|d| d.path == "/nodes/1/comp"
                && d.message.contains("no sub-composition called \"sting\"")),
            "{missing:?}"
        );
        comp.comps.insert("sting".into(), sting_comp());
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());
    }

    /// A sub-comp is a scope of its own: parent and child may both have a
    /// node called `card` and an asset called `logo`, and a diagnostic
    /// inside the child is pointed at `/comps/{name}/...` - the scope is
    /// in the path, not implied.
    #[test]
    fn sub_comps_are_scopes_of_their_own() {
        let mut comp = rect_comp();
        comp.assets.insert(
            "logo".into(),
            Asset {
                src: "logo.png".into(),
                kind: None,
            },
        );
        let mut child = sting_comp();
        // The child's own `card`, its own `logo`: the same names, another
        // scope, no collision and no complaint.
        child.id = "sting".into();
        child.assets.insert(
            "logo".into(),
            Asset {
                src: "sting-logo.png".into(),
                kind: None,
            },
        );
        child.nodes.insert(
            0,
            Node {
                id: "card".into(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform::default(),
                opacity: 1.0,
                blend: crate::comp::Blend::Normal,
                mask: None,
                matte: None,
                motion_blur: None,
                effects: Vec::new(),
                tracks: Default::default(),
                kind: NodeKind::Rect {
                    width: Length::Px(100.0),
                    height: Length::Px(50.0),
                    fill: None,
                    radius: 0.0,
                },
            },
        );
        comp.comps.insert("sting".into(), child);
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());

        // A diagnostic inside the child carries the child's own path.
        let mut broken = comp.clone();
        broken.comps.get_mut("sting").expect("the sting").duration = 0.0;
        let problems = broken.validate();
        assert!(
            problems.iter().any(|d| d.path == "/comps/sting/duration"),
            "{problems:?}"
        );
    }

    /// A comp cannot contain itself through any chain of placements: the
    /// trail refuses it here, with a pointer into the document, before
    /// the engine refuses it as E207.
    #[test]
    fn a_comp_cannot_contain_itself_through_a_chain() {
        let mut outer = rect_comp();
        outer.id = "outer".into();
        let mut middle = sting_comp();
        middle.id = "outer".into(); // placing middle eventually places outer again
        outer.comps.insert("loop".into(), middle);
        let problems = outer.validate();
        assert!(
            problems
                .iter()
                .any(|d| d.path == "/comps/loop" && d.message.contains("contains itself")),
            "{problems:?}"
        );
    }

    /// The engine nests eight levels deep; a document that goes deeper is
    /// refused here with its depth named, not discovered mid-bake.
    #[test]
    fn nesting_past_the_engines_eight_levels_is_refused() {
        let mut comp = rect_comp();
        comp.id = "level0".into();
        let mut current = &mut comp;
        for level in 1..=9 {
            let mut next = sting_comp();
            next.id = format!("level{level}");
            current = current.comps.entry(format!("level{level}")).or_insert(next);
        }
        let problems = comp.validate();
        assert!(
            problems
                .iter()
                .any(|d| d.message.contains("deeper than the engine's eight levels")),
            "{problems:?}"
        );
    }

    /// The comps map serialises and comes back: a document saved by one
    /// hand is read by the next, and an empty map stays unwritten - the
    /// committed comp documents do not grow a field they never used.
    #[test]
    fn the_comps_map_round_trips_and_stays_unwritten_when_empty() {
        let mut comp = rect_comp();
        comp.comps.insert("sting".into(), sting_comp());
        let text = serde_json::to_string(&comp).expect("serialises");
        assert!(text.contains("\"comps\":{\"sting\""));
        let back: Comp = serde_json::from_str(&text).expect("parses");
        assert!(back.comps.contains_key("sting"));
        assert_eq!(back, comp);

        let plain = rect_comp();
        let text = serde_json::to_string(&plain).expect("serialises");
        assert!(
            !text.contains("comps"),
            "an empty map is not written: {text}"
        );
        let back: Comp = serde_json::from_str(&text).expect("parses");
        assert!(back.comps.is_empty());
    }

    /// A sub-comp's files are part of the document's content key in the
    /// same way its own are: changing the logo inside the sting is a
    /// different bake.
    #[test]
    fn the_comps_hash_moves_when_a_sub_comp_moves() {
        let mut comp = rect_comp();
        let before = comp.hash();
        comp.comps.insert("sting".into(), sting_comp());
        assert_ne!(comp.hash(), before);
        let changed = {
            let mut sting = sting_comp();
            sting.duration = 9.0;
            sting
        };
        comp.comps.insert("sting".into(), changed);
        assert_ne!(comp.hash(), before);
    }

    /// A whole comp a placement can use: one gold bar and one word, the
    /// sting the tests place nine times if they want to.
    fn sting_comp() -> Comp {
        Comp {
            format: FORMAT,
            id: "sting".into(),
            name: "Sting".into(),
            width: 640,
            height: 360,
            fps: 30.0,
            duration: 1.2,
            background: transparent(),
            assets: BTreeMap::new(),
            curves: BTreeMap::new(),
            comps: Default::default(),
            nodes: vec![Node {
                id: "bar".into(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform::default(),
                opacity: 1.0,
                blend: crate::comp::Blend::Normal,
                mask: None,
                matte: None,
                motion_blur: None,
                effects: Vec::new(),
                tracks: Default::default(),
                kind: NodeKind::Rect {
                    width: Length::Px(320.0),
                    height: Length::Px(120.0),
                    fill: Some(Paint::Solid {
                        color: Rgba::hex("#d9a441ff"),
                    }),
                    radius: 12.0,
                },
            }],
            slots: Vec::new(),
        }
    }
}

#[cfg(test)]
mod phase1_tests {
    use super::*;

    fn shell(id: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
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
            tracks: Default::default(),
            kind,
        }
    }

    fn comp_with(nodes: Vec<Node>) -> Comp {
        Comp {
            format: FORMAT,
            id: "p1".into(),
            name: "Phase one".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 2.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            comps: Default::default(),
            nodes,
            slots: Vec::new(),
        }
    }

    fn path_node(id: &str) -> Node {
        shell(
            id,
            NodeKind::Path {
                commands: vec![
                    PathCommand::MoveTo {
                        point: Point::new(0.0, 0.0),
                    },
                    PathCommand::LineTo {
                        point: Point::new(100.0, 0.0),
                    },
                    PathCommand::Close,
                ],
                fill: None,
                stroke: None,
                dash: None,
                trim: None,
                morph: None,
            },
        )
    }

    #[test]
    fn a_document_from_before_the_fields_loads_unchanged() {
        // The JSON of a comp written before trim, morph, matte, motion blur,
        // adjustment and radial gradients existed must parse into the new
        // schema with the defaults that mean "not used".
        let old = serde_json::json!({
            "format": 1,
            "id": "legacy",
            "width": 640,
            "height": 360,
            "duration": 1.0,
            "nodes": [
                { "id": "line", "kind": "path",
                  "commands": [ { "op": "move-to", "point": { "x": 0, "y": 0 } } ] },
                { "id": "card", "kind": "rect",
                  "width": { "px": 100 }, "height": { "px": 50 },
                  "fill": { "kind": "gradient", "stops": [[0, { "r": 1, "g": 1, "b": 1, "a": 1 }]] } }
            ]
        });
        let comp: Comp = serde_json::from_value(old).expect("a legacy comp parses");
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());
        assert!(comp.nodes[0].matte.is_none());
        assert!(comp.nodes[0].motion_blur.is_none());
        match &comp.nodes[1].kind {
            NodeKind::Rect { fill, .. } => assert!(matches!(
                fill,
                Some(Paint::Gradient {
                    ramp: GradientKind::Linear,
                    ..
                })
            )),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_trim_out_of_bounds_is_refused() {
        let mut node = path_node("line");
        if let NodeKind::Path { trim, .. } = &mut node.kind {
            *trim = Some((-0.2, 0.8));
        }
        let problems = comp_with(vec![node]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/trim")),
            "{problems:?}"
        );

        let mut node = path_node("line");
        if let NodeKind::Path { trim, .. } = &mut node.kind {
            *trim = Some((0.8, 0.2));
        }
        let problems = comp_with(vec![node]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/trim")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_matte_pointing_nowhere_is_refused() {
        let mut node = path_node("stencilled");
        node.matte = Some(Matte {
            source: "ghost".into(),
            mode: MatteMode::Alpha,
        });
        let problems = comp_with(vec![node]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/matte")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_node_cannot_matte_itself() {
        let mut node = path_node("mirror");
        node.matte = Some(Matte {
            source: "mirror".into(),
            mode: MatteMode::Luma,
        });
        let problems = comp_with(vec![node]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/matte")),
            "{problems:?}"
        );
    }

    #[test]
    fn motion_blur_bounds_are_enforced() {
        let mut node = path_node("fast");
        node.motion_blur = Some(MotionBlur {
            samples: 1,
            shutter: 0.5,
        });
        let problems = comp_with(vec![node]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/motion-blur")),
            "{problems:?}"
        );
        let mut wide = path_node("fast");
        wide.motion_blur = Some(MotionBlur {
            samples: 8,
            shutter: 1.5,
        });
        let problems = comp_with(vec![wide]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/motion-blur")),
            "{problems:?}"
        );
        // The film look itself passes.
        let mut film = path_node("fast");
        film.motion_blur = Some(MotionBlur {
            samples: 8,
            shutter: 0.5,
        });
        assert!(comp_with(vec![film]).validate().is_empty());
    }

    #[test]
    fn an_empty_path_or_morph_target_is_refused() {
        let mut node = path_node("dot");
        if let NodeKind::Path { commands, .. } = &mut node.kind {
            commands.clear();
        }
        let problems = comp_with(vec![node]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/commands")),
            "{problems:?}"
        );

        let mut node = path_node("shifter");
        if let NodeKind::Path { morph, .. } = &mut node.kind {
            *morph = Some(Vec::new());
        }
        let problems = comp_with(vec![node]).validate();
        assert!(
            problems.iter().any(|d| d.path.contains("/morph")),
            "{problems:?}"
        );
    }

    #[test]
    fn geneva_inexpressible_names_what_the_native_engine_is_for() {
        // A plain comp of shapes and text: nothing to route.
        let plain = comp_with(vec![
            shell(
                "card",
                NodeKind::Rect {
                    width: Length::Px(100.0),
                    height: Length::Px(50.0),
                    fill: Some(Paint::Solid {
                        color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                    }),
                    radius: 4.0,
                },
            ),
            shell("words", NodeKind::Text(Box::<TextNode>::default())),
        ]);
        assert!(plain.geneva_inexpressible().is_empty());

        // A path routes; so do mattes, adjustments, path masks, expansion,
        // the dropped effects, the dropped tracks and shape gradients.
        let mut stencilled = path_node("mark");
        stencilled.matte = Some(Matte {
            source: "words".into(),
            mode: MatteMode::Alpha,
        });
        let mut graded = shell("grade", NodeKind::Adjustment);
        graded.effects = vec![Effect::Glow {
            radius: 12.0,
            intensity: 0.6,
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        }];
        let mut masked = shell(
            "plate",
            NodeKind::Ellipse {
                width: Length::Px(100.0),
                height: Length::Px(100.0),
                fill: Some(Paint::Gradient {
                    stops: vec![(0.0, Rgba::new(1.0, 1.0, 1.0, 1.0))],
                    ramp: GradientKind::Radial,
                    angle: 90.0,
                    from: 0.0,
                    to: 1.0,
                }),
            },
        );
        masked.mask = Some(Mask {
            shape: MaskShape::Path {
                commands: vec![
                    PathCommand::MoveTo {
                        point: Point::new(0.0, 0.0),
                    },
                    PathCommand::LineTo {
                        point: Point::new(10.0, 0.0),
                    },
                    PathCommand::Close,
                ],
            },
            radius: 0.0,
            feather: 0.0,
            invert: false,
            expansion: 4.0,
        });
        let comp = comp_with(vec![stencilled, graded, masked]);
        let found = comp.geneva_inexpressible();
        for expected in [
            "a vector path",
            "a track matte",
            "an adjustment layer",
            "a glow effect",
            "a path mask",
            "mask expansion",
            "a shape gradient",
        ] {
            assert!(
                found.contains(&expected),
                "{expected} missing from {found:?}"
            );
        }

        // Sub-compositions are walked: a path inside a placed comp routes
        // the parent too.
        let mut parent = comp_with(Vec::new());
        parent.comps.insert("inner".into(), comp);
        assert!(parent.geneva_inexpressible().contains(&"a vector path"));
    }
}
