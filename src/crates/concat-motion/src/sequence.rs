// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! A sequence: comps placed one after another on one timeline, as one render.
//!
//! A single comp bakes to its own little frame sequence. A *sequence* is what
//! a reel, an explainer or a title package actually is - half a dozen comps,
//! each with its own entrance, butted together over plates with a dip between
//! them and a bed of music under the lot - and it is the reason a motion layer
//! is worth having rather than a folder of GIFs.
//!
//! The document this builds is an ordinary Geneva timeline: every scene
//! contributes its layers, shifted to its own start time; the scene's own
//! layers - plates under the comp, overlays over it, each drifting on its own
//! clock, which is what makes a stack of stills read as depth - stack around
//! it; one full-frame curtain dips between scenes; audio beds go in the
//! timeline's own audio tracks. Nothing here is a second renderer - it is the
//! same timeline a comp bakes through, with more than one comp in it.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use concat_motion::{authoring, geneva::Geneva, presets, sequence};
//!
//! let mut reel = sequence::Sequence::new(1920, 1080, 30.0);
//! reel.push(sequence::Scene::new(presets::build("funcut.sting.wordmark").expect("a preset")));
//! reel.push(sequence::Scene::new(presets::build("funcut.caption.kinetic").expect("a preset")));
//!
//! let geneva = Geneva::discover(None).expect("the sidecar");
//! let plan = sequence::plan(&geneva, &reel)?;
//! plan.render(&geneva, std::path::Path::new("cache/reel"), std::path::Path::new("reel.mp4"))?;
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json, json};

use crate::authoring::{self, Op};
use crate::comp::{Asset, Blend, Comp, Fit, Rgba};
use crate::curve::Curve;
use crate::engine::{BakeRequest, MotionError};
use crate::geneva::Geneva;

/// One comp placed in a sequence.
#[derive(Clone, Debug)]
pub struct Scene {
    /// The comp this scene shows.
    pub comp: Comp,
    /// Operations applied to the comp before it is placed, all or nothing.
    pub ops: Vec<Op>,
    /// Where it starts; `None` places it after the scene before it.
    pub at: Option<f64>,
    /// How long it holds; `None` is the comp's own length.
    pub hold: Option<f64>,
    /// The stills stacked into the scene, bottom first: plates under the
    /// comp, and - with [`SceneLayer::above`] - grain, vignettes, light
    /// leaks and foreground silhouettes over it. A scene with one layer
    /// that only drifts is the classic plate; more than one is a
    /// multi-layer scene, and the drift each layer gets is what reads as
    /// depth when they differ.
    pub layers: Vec<SceneLayer>,
    /// The scene's pulse, when the audio drives it: the beat grid, read
    /// in this scene's own clock, baked into the comp's nodes as the
    /// plan is laid. The grid is measured elsewhere (`suggest_cuts`
    /// reads it from the soundtrack); this is the bake.
    pub pulse: Option<crate::pulse::Pulse>,
    /// The scene's colour grade, baked into the comp as its first paint:
    /// a full-bleed box under the comp's own content whose
    /// `backdrop-filter` and soft-light cast approximate the brand LUT
    /// over whatever the scene has under the comp - the plate, the
    /// lower layers. The temperatures are the emotion read's own seven,
    /// so the act is the grade: a cold hook, a warm payoff.
    pub grade: Option<crate::grade::Grade>,
    /// The personality the scene wears over its layout: the weight the
    /// type carries, how tightly it tracks, the hand its eased segments
    /// move with, the tempo its animations run at - and, when no grade
    /// is named, the colour it is graded. One layout, three moods.
    pub style: Option<crate::style::Style>,
}

/// Organic drift, deterministic: a seeded noise field the layer wanders
/// over, baked into its position keys when the plan is laid.
///
/// A pan is a sentence a layer says once; a wobble is a voice - motion that
/// never repeats, never resolves, and never reads as keyframed, because
/// it was not: it is [`crate::noise::Noise`] read at `rate` keys a second
/// and folded onto the drift the layer already carries. The determinism
/// contract holds exactly: the **seed** is the whole of the randomness,
/// so the same layer wobbles identically on every machine, in every
/// render, forever - a cache key it belongs in, not a gamble.
///
/// The field is read one cycle per ten keys, so one knob shapes the feel:
/// a higher `rate` samples finer *and* wanders faster, and a lower one is
/// a lazier drift. The `amount` is in fractions of the frame, like `pan` -
/// `0.008` is a plate breathing; `0.02` is a hand-held read.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Wobble {
    /// The noise field's seed: the whole of the randomness, and the one
    /// field a determinism contract can allow. Two layers on one seed
    /// read two channels of one field and never wobble in step.
    pub seed: u64,
    /// How far the wander reaches, in fractions of the frame each side of
    /// the drift. A cover-fitted layer grows to cover its own wobble, the
    /// way it grows for a pan.
    #[serde(default = "default_wobble_amount")]
    pub amount: f64,
    /// How many keys a second the field is sampled at - also how fast the
    /// field is read, one cycle per ten keys. Held to 1..=30 in the
    /// emission, whatever the document says: a rate of zero would freeze
    /// the layer and a rate of a thousand would be a track, not a drift.
    #[serde(default = "default_wobble_rate")]
    pub rate: f64,
}

fn default_wobble_amount() -> f64 {
    0.008
}

fn default_wobble_rate() -> f64 {
    8.0
}

/// A still layered into a scene - under its comp as a plate, or over it
/// as an overlay - with its own drift, blend, opacity and timing.
///
/// The whole point of a layer is that it moves on its own clock: a
/// background plate drifting four percent while a foreground card slides
/// two is parallax, and parallax is what makes a stack of stills read as
/// a scene rather than a collage. [`SceneLayer::new`] is a plain
/// full-frame plate; every other field starts at "nothing", so a layer
/// says only what it means.
#[derive(Clone, Debug)]
pub struct SceneLayer {
    /// An asset id on the sequence.
    pub asset: String,
    /// How much the still grows over its span, as a scale delta: `0.06` is
    /// six percent, which reads as life without reading as a zoom.
    pub zoom: f64,
    /// How far it drifts across the frame over its span, as fractions of
    /// the frame: `(0.02, 0.0)` ends two percent of a frame right of where
    /// it began. A cover-fitted layer is scaled up by its own drift so the
    /// edge never shows.
    pub pan: (f64, f64),
    /// A static offset from the centre, in fractions of the frame:
    /// `(0.1, 0.0)` sits a tenth of a frame right. A cover-fitted layer
    /// grows to cover this too.
    pub offset: (f64, f64),
    /// The base scale multiplier: `1.0` fills the frame, `1.2` overscans
    /// twenty percent, `0.5` draws at half size with the frame's edges
    /// showing, which is sometimes exactly the point.
    pub scale: f64,
    /// How the picture is fitted into the frame. `Cover` is the plate;
    /// anything else is a deliberate shape.
    pub fit: Fit,
    /// How it mixes with what is under it: `screen` and `add` turn a soft
    /// radial picture into a light leak, `multiply` shades one into a
    /// vignette.
    pub blend: Blend,
    /// Its steady opacity, 0 to 1, before any fades.
    pub opacity: f64,
    /// Fade up from transparent, in seconds.
    pub fade_in: f64,
    /// Fade down to transparent, in seconds.
    pub fade_out: f64,
    /// When it starts, from the scene's own start: a foreground that
    /// arrives at `1.2` lands over a scene already running.
    pub start: f64,
    /// How long it holds; `None` runs to the scene's end.
    pub hold: Option<f64>,
    /// Whether it paints over the comp rather than under it - grain,
    /// vignettes, light leaks, foreground silhouettes. It still paints
    /// under the curtain and under any scene that overlaps this one.
    pub above: bool,
    /// The easing its fades travel, when they travel one: the library's
    /// own curves - `Curve::named("bounce")`, `steps(8)`, any bezier, a
    /// spring - attached to the segment each fade starts, so the header
    /// can arrive on the settle curve while the body bounces in under
    /// it. `None` (the default) is the straight fade every layer shipped
    /// with, and a layer that does not fade does not carry one at all.
    pub fade_curve: Option<Curve>,
    /// The named scene this layer places, when its source is a scene
    /// rather than an asset: the scene is emitted once as a reusable
    /// composition and this layer shows it. `None` is the ordinary
    /// picture layer, and `asset` is the placement's name in errors.
    pub scene: Option<String>,
    /// How fast the layer drifts, as a multiple of its own `zoom` and
    /// `pan`: `None` (the default) is as written, so every layer built
    /// before the field existed renders exactly as it did. `Some(2.0)` is
    /// twice the drift, `Some(0.5)` half - nearer things move faster,
    /// which is the whole of parallax. A layer with no drift of its own
    /// is given the classic plate's (four percent of growth, three of
    /// slide) before the rate is applied, so `parallax` alone moves a
    /// layer.
    pub parallax: Option<f64>,
    /// Organic drift, when the layer carries it: a seeded noise field
    /// folded onto the drift, baked into the position keys at plan time.
    /// `None` (the default) is the layer as every storyboard wrote it
    /// before the field existed.
    pub wobble: Option<Wobble>,
}

impl SceneLayer {
    /// A full-frame plate: cover-fitted, opaque, normal-blended, running
    /// the whole scene. Everything else is opt-in.
    pub fn new(asset: impl Into<String>) -> SceneLayer {
        SceneLayer {
            asset: asset.into(),
            zoom: 0.0,
            pan: (0.0, 0.0),
            offset: (0.0, 0.0),
            scale: 1.0,
            fit: Fit::Cover,
            blend: Blend::Normal,
            opacity: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            start: 0.0,
            hold: None,
            above: false,
            scene: None,
            parallax: None,
            fade_curve: None,
            wobble: None,
        }
    }

    /// Grow over the span, as a scale delta.
    pub fn zooming(mut self, zoom: f64) -> SceneLayer {
        self.zoom = zoom;
        self
    }

    /// Drift across the frame over the span, as fractions of the frame.
    pub fn panning(mut self, x: f64, y: f64) -> SceneLayer {
        self.pan = (x, y);
        self
    }

    /// Sit at a static offset from the centre, as fractions of the frame.
    pub fn at_offset(mut self, x: f64, y: f64) -> SceneLayer {
        self.offset = (x, y);
        self
    }

    /// Set the base scale multiplier.
    pub fn scaled(mut self, scale: f64) -> SceneLayer {
        self.scale = scale;
        self
    }

    /// Fit the picture another way than cover.
    pub fn fitted(mut self, fit: Fit) -> SceneLayer {
        self.fit = fit;
        self
    }

    /// Mix with what is under it another way than normal.
    pub fn blending(mut self, blend: Blend) -> SceneLayer {
        self.blend = blend;
        self
    }

    /// Mix with what is under it another way than normal - the name the
    /// scene's own vocabulary uses (`screen`, `multiply`, ...). The same
    /// as [`SceneLayer::blending`].
    pub fn blend(self, mode: Blend) -> SceneLayer {
        self.blending(mode)
    }

    /// Drift at this rate rather than as written: a multiple of the
    /// layer's own `zoom` and `pan`, or of the classic plate's drift when
    /// the layer has none. Three layers at rates 0.5, 1.0 and 2.0 are
    /// three depths of one scene - the near one hurrying, the far one
    /// barely breathing - which is what parallax is.
    ///
    /// The rate is applied where the timeline is emitted, so the layer's
    /// own numbers stay what they said: `zoom` is still the drift at rate
    /// 1.0, and the emitted keyframes are the ones the engine draws.
    pub fn parallax(mut self, rate: f64) -> SceneLayer {
        self.parallax = Some(rate);
        self
    }

    /// Wander on a seeded noise field, folded onto the drift: the organic
    /// motion a keyframe cannot write by hand. The seed is the whole of
    /// the randomness - see [`Wobble`] for what that buys.
    pub fn wobbling(mut self, wobble: Wobble) -> SceneLayer {
        self.wobble = Some(wobble);
        self
    }

    /// Set the steady opacity, 0 to 1.
    pub fn opacity(mut self, opacity: f64) -> SceneLayer {
        self.opacity = opacity;
        self
    }

    /// Fade up over `in_seconds` and down over `out_seconds`, between
    /// which it holds its own opacity.
    pub fn fading(mut self, in_seconds: f64, out_seconds: f64) -> SceneLayer {
        self.fade_in = in_seconds;
        self.fade_out = out_seconds;
        self
    }

    /// Start this long after the scene does.
    pub fn starting_at(mut self, seconds: f64) -> SceneLayer {
        self.start = seconds;
        self
    }

    /// Start this long after the scene does - the choreography's name
    /// for it: a stack of layers taking the stage one after another is a
    /// scene, where a stack that arrives at once is a collage. The same
    /// as [`SceneLayer::starting_at`]; the emitted timeline carries it as
    /// the clip's `start`.
    pub fn delay(self, seconds: f64) -> SceneLayer {
        self.starting_at(seconds)
    }

    /// Hold for this long rather than the scene's own span.
    pub fn holding(mut self, seconds: f64) -> SceneLayer {
        self.hold = Some(seconds);
        self
    }

    /// Paint over the comp rather than under it.
    pub fn above(mut self) -> SceneLayer {
        self.above = true;
        self
    }

    /// Ease the fades: the curve each fade's segment travels, from the
    /// library by name or built whole - `steps(8)` for a mechanical
    /// reveal, `linear(0, 1.4, 0.9, 1)` for a bounce, a spring for a
    /// settle. The header and the body no longer share a curve.
    pub fn eased(mut self, curve: Curve) -> SceneLayer {
        self.fade_curve = Some(curve);
        self
    }
}

/// A bed of sound under the whole sequence.
#[derive(Clone, Debug)]
pub struct AudioBed {
    /// An asset id on the sequence.
    pub asset: String,
    /// Gain in decibels.
    pub gain_db: f64,
    /// Fade up from silence.
    pub fade_in: f64,
    /// Fade to silence.
    pub fade_out: f64,
}

/// The dip between scenes: a full-frame wash of one colour, opaque at the
/// cuts and clear in between.
#[derive(Clone, Copy, Debug)]
pub struct Curtain {
    /// The colour dipped through.
    pub color: Rgba,
    /// Fade up from it at the top of the sequence.
    pub open: f64,
    /// Fade down into it at the end.
    pub close: f64,
    /// Half-width of each dip, in seconds.
    pub dip: f64,
    /// How opaque it gets at the cut: 1 is a full cut to colour.
    pub depth: f64,
}

impl Default for Curtain {
    fn default() -> Self {
        Curtain {
            color: Rgba::new(0.02, 0.024, 0.04, 1.0),
            open: 0.5,
            close: 0.7,
            dip: 0.22,
            depth: 1.0,
        }
    }
}

/// A run of comps, plates and sound, rendered as one file.
#[derive(Clone, Debug)]
pub struct Sequence {
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Frame rate.
    pub fps: f64,
    /// The master clear colour, opaque: a sequence is a picture, not an
    /// overlay.
    pub background: Rgba,
    /// Where every asset path resolves from - the folder the reel was
    /// written in. Assets are copied out of it into the render directory,
    /// because an engine is never given a path that leaves its own.
    pub source_dir: PathBuf,
    /// The scenes, in order.
    pub scenes: Vec<Scene>,
    /// Shared assets the scenes and the sound refer to, by id.
    pub assets: BTreeMap<String, Asset>,
    /// The sound under it.
    pub audio: Vec<AudioBed>,
    /// The dip between scenes; `None` cuts hard.
    pub curtain: Option<Curtain>,
    /// Encoder quality, 0 to 51, lower is better.
    pub crf: u8,
    /// The storyboard's own scene cards, kept so a layer that names a
    /// scene can find it: the plan places named scenes as nested
    /// compositions. Empty for a sequence built by hand.
    pub scene_cards: Vec<crate::storyboard::SceneCard>,
    /// The choreographed cuts, keyed by the **outgoing** scene's index:
    /// `transitions[0]` is the cut between scene 0 and scene 1. A cut
    /// writes motion into both sides - the outgoing scene's last
    /// moments and the incoming scene's first - and replaces the
    /// curtain's dip at that boundary, because a whip pan and a colour
    /// wash at the same frame is two transitions fighting.
    pub transitions: std::collections::BTreeMap<usize, crate::transition::Cut>,
    /// The choreographed way the reel *opens*: the cut into the first
    /// scene, whose enter side is the only side there is. A zoom onto
    /// the opening frame, a rack focus onto the hook - the entrance a
    /// reel earns before it has anywhere to come from.
    pub opening: Option<crate::transition::Cut>,
}

impl Sequence {
    /// An empty sequence of the given frame.
    pub fn new(width: u32, height: u32, fps: f64) -> Sequence {
        Sequence {
            width,
            height,
            fps,
            background: Rgba::new(0.02, 0.024, 0.04, 1.0),
            source_dir: PathBuf::new(),
            scenes: Vec::new(),
            assets: BTreeMap::new(),
            audio: Vec::new(),
            curtain: Some(Curtain::default()),
            scene_cards: Vec::new(),
            transitions: BTreeMap::new(),
            opening: None,
            crf: 18,
        }
    }

    /// Add a scene at the end.
    pub fn push(&mut self, scene: Scene) -> &mut Sequence {
        self.scenes.push(scene);
        self
    }

    /// Add an asset the scenes or the sound refer to.
    pub fn asset(&mut self, id: impl Into<String>, asset: Asset) -> &mut Sequence {
        self.assets.insert(id.into(), asset);
        self
    }

    /// Add a bed of sound.
    pub fn bed(&mut self, bed: AudioBed) -> &mut Sequence {
        self.audio.push(bed);
        self
    }

    /// Choreograph the cut after scene `after`: both sides of the
    /// boundary get the [`Cut`]'s motion, and the curtain's dip at that
    /// boundary stands down for it. The last scene's own exit can be
    /// choreographed the same way - a staggered exit at the reel's end
    /// is the credits rolling out.
    pub fn with_cut(&mut self, after: usize, cut: crate::transition::Cut) -> &mut Sequence {
        self.transitions.insert(after, cut);
        self
    }

    /// Choreograph the way the reel opens: the cut onto the first
    /// scene's first frames.
    pub fn opens_with(&mut self, cut: crate::transition::Cut) -> &mut Sequence {
        self.opening = Some(cut);
        self
    }

    /// How long the sequence runs: the end of the last scene.
    pub fn duration(&self) -> f64 {
        self.layout().last().map(|(_, _, end)| *end).unwrap_or(0.0)
    }

    /// Each scene's start and length, with `at` and `hold` resolved.
    ///
    /// A scene that names no start follows the one before it, so a reel is
    /// written in order without arithmetic; a scene that names no hold is as
    /// long as its comp. A scene that enters by an **overlapping** cut
    /// starts before the one before it ends - by the cut's own seconds, the
    /// window both scenes share - because a choreographed cut is a
    /// composite, not a splice: without the overlap, the whip pan renders
    /// as two edits.
    fn layout(&self) -> Vec<(usize, f64, f64)> {
        let mut out = Vec::with_capacity(self.scenes.len());
        let mut clock = 0.0;
        let mut previous_hold = 0.0;
        for (index, scene) in self.scenes.iter().enumerate() {
            let start = scene
                .at
                .unwrap_or_else(|| clock - enter_overlap(self, index, previous_hold));
            let length = scene.hold.unwrap_or(scene.comp.duration).max(0.1);
            out.push((index, start, start + length));
            clock = start + length;
            previous_hold = length;
        }
        out
    }
}

/// How far scene `index` starts before the scene before it ends: the
/// temporal window the cut it enters by plays over, with the scene
/// before it still leaving on screen. The window is the cut's own
/// seconds, clamped so the scene before keeps at least a tenth of a
/// second of itself - a cut longer than the scene it leaves plays as
/// the sequential exit it is, and the plan says so rather than pulling
/// the incoming scene under the whole of the outgoing one.
///
/// An authored `at` overrides this exactly as it always did: the author
/// placed the scene, and the placement is theirs.
fn enter_overlap(sequence: &Sequence, index: usize, previous_hold: f64) -> f64 {
    if index == 0 {
        // The opening cut has nothing before it to share a window with.
        return 0.0;
    }
    let Some(cut) = sequence.transitions.get(&(index - 1)) else {
        return 0.0;
    };
    if !cut.overlaps() || cut.seconds() <= 1e-9 {
        return 0.0;
    }
    cut.seconds().min((previous_hold - 0.1).max(0.0))
}

impl Scene {
    /// A scene showing `comp`.
    pub fn new(comp: Comp) -> Scene {
        Scene {
            comp,
            ops: Vec::new(),
            at: None,
            hold: None,
            layers: Vec::new(),
            pulse: None,
            grade: None,
            style: None,
        }
    }

    /// The operations to apply before placing it.
    pub fn with_ops(mut self, ops: Vec<Op>) -> Scene {
        self.ops = ops;
        self
    }

    /// Start it at a time, rather than after the scene before it.
    pub fn starting_at(mut self, at: f64) -> Scene {
        self.at = Some(at);
        self
    }

    /// Hold it for a length other than the comp's own.
    pub fn holding(mut self, seconds: f64) -> Scene {
        self.hold = Some(seconds);
        self
    }

    /// Put a still under it, drifting `zoom` as a scale delta over the
    /// scene. The first call is the classic plate; a second stacks another
    /// layer under the comp, above the first.
    pub fn over(mut self, asset: impl Into<String>, zoom: f64) -> Scene {
        self.layers.push(SceneLayer::new(asset).zooming(zoom));
        self
    }

    /// Stack one more layer into the scene - a plate under the comp, or,
    /// with [`SceneLayer::above`], an overlay over it. Layers stack in the
    /// order they are added, bottom first on each side of the comp.
    pub fn layer(mut self, layer: SceneLayer) -> Scene {
        self.layers.push(layer);
        self
    }

    /// Give the scene a pulse: the beat grid, read in the scene's own
    /// clock, baked into the comp's nodes when the plan is laid. A
    /// chart that grows on the downbeat, a caption that pulses on the
    /// transient - the video gets a heartbeat.
    pub fn pulsing(mut self, pulse: crate::pulse::Pulse) -> Scene {
        self.pulse = Some(pulse);
        self
    }

    /// Give the scene a colour grade, baked into the comp as its first
    /// paint when the plan is laid: the `backdrop-filter` and soft-light
    /// cast over whatever sits under the comp, the comp's own words
    /// untouched. [`crate::grade::Grade::at`] is the shorthand for a
    /// temperature at its own strength - the cold hook, the warm payoff.
    pub fn graded(mut self, grade: crate::grade::Grade) -> Scene {
        self.grade = Some(grade);
        self
    }

    /// Give the scene a personality to wear over its layout - the
    /// typography, the easing hand, the tempo, and the colour when no
    /// grade is named. The layout keeps its geometry and its words.
    pub fn styled(mut self, style: crate::style::Style) -> Scene {
        self.style = Some(style);
        self
    }
}

impl AudioBed {
    /// A bed at unity gain.
    pub fn new(asset: impl Into<String>) -> AudioBed {
        AudioBed {
            asset: asset.into(),
            gain_db: 0.0,
            fade_in: 0.5,
            fade_out: 1.0,
        }
    }

    /// Set the gain, in decibels.
    pub fn at(mut self, gain_db: f64) -> AudioBed {
        self.gain_db = gain_db;
        self
    }
}

/// A sequence: every scene, in order, with the assets they share.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The Geneva timeline document.
    pub document: Json,
    /// Where each asset's bytes come from, by the path the document uses.
    pub files: BTreeMap<String, PathBuf>,
    /// Everything the mapping could not express, node by node.
    pub warnings: Vec<String>,
    /// How long it runs, in seconds.
    pub duration: f64,
    /// How many scenes it has.
    pub scenes: usize,
    /// Where the asset paths resolve from when they are relative.
    pub source_root: PathBuf,
}

impl Plan {
    /// The document, as text.
    pub fn json(&self) -> String {
        serde_json::to_string_pretty(&self.document).unwrap_or_default()
    }

    /// Write the timeline and copy in every asset it refers to, so the folder
    /// is a render that needs nothing outside itself.
    pub fn write(&self, dir: &Path) -> Result<PathBuf, MotionError> {
        for (dest, source) in &self.files {
            let to = dir.join(dest);
            if to.is_file() {
                continue;
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|error| MotionError::Io {
                    doing: "creating".into(),
                    path: parent.to_path_buf(),
                    source: error,
                })?;
            }
            let from = if source.is_absolute() {
                source.clone()
            } else {
                self.source_root.join(source)
            };
            std::fs::copy(&from, &to).map_err(|error| MotionError::Io {
                doing: "copying an asset into the render".into(),
                path: from,
                source: error,
            })?;
        }
        std::fs::create_dir_all(dir).map_err(|error| MotionError::Io {
            doing: "creating".into(),
            path: dir.to_path_buf(),
            source: error,
        })?;
        let path = dir.join("timeline.json");
        std::fs::write(&path, self.json()).map_err(|error| MotionError::Io {
            doing: "writing".into(),
            path: path.clone(),
            source: error,
        })?;
        Ok(path)
    }

    /// Render the whole sequence to `out`, from a render directory.
    ///
    /// The engine runs with the render directory as its own, so a relative
    /// `out` would land inside it: the file is named in full before it is
    /// asked for, and the answer's `written` is the path the caller meant.
    pub fn render(&self, geneva: &Geneva, dir: &Path, out: &Path) -> Result<Json, MotionError> {
        self.write(dir)?;
        let out = absolute(out);
        let args: Vec<String> = vec![
            "--format".into(),
            "json".into(),
            "render".into(),
            "timeline.json".into(),
            "-o".into(),
            out.display().to_string(),
        ];
        geneva.run_in(dir, &args)
    }

    /// Render the whole sequence through **`geneva farm`**: the render cut
    /// into `parts`, each part drawn by its own worker process, the parts
    /// joined without a re-encode. The optional high-performance path for
    /// long renders - even on a single machine it improves CPU
    /// utilization, and on several it is minutes for the price of one.
    ///
    /// `parts` is clamped to at least one; a render that cannot be split
    /// (the engine's own `E504`) comes back as that refusal, not as a
    /// wrong file. Workers on other machines are the engine's own to
    /// arrange - see PERFORMANCE.md's "the farm" for the setups the flag
    /// pays for.
    pub fn render_parts(
        &self,
        geneva: &Geneva,
        dir: &Path,
        out: &Path,
        parts: u32,
    ) -> Result<Json, MotionError> {
        self.write(dir)?;
        let out = absolute(out);
        let parts = parts.max(1);
        let args: Vec<String> = vec![
            "--format".into(),
            "json".into(),
            "farm".into(),
            "timeline.json".into(),
            "-o".into(),
            out.display().to_string(),
            "--parts".into(),
            parts.to_string(),
        ];
        geneva.run_in(dir, &args)
    }

    /// Draw one still of the sequence, for checking a cut before rendering
    /// the whole thing.
    pub fn frame(
        &self,
        geneva: &Geneva,
        dir: &Path,
        at: f64,
        out: &Path,
    ) -> Result<Json, MotionError> {
        self.write(dir)?;
        let out = absolute(out);
        let args: Vec<String> = vec![
            "--format".into(),
            "json".into(),
            "frame".into(),
            "timeline.json".into(),
            "--at".into(),
            format!("{at}s"),
            "-o".into(),
            out.display().to_string(),
        ];
        geneva.run_in(dir, &args)
    }
}

/// A path as the caller meant it: an engine that runs in a render directory
/// would otherwise resolve a relative output inside that directory.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// The position path of a wobbling layer, as keyframe marks: the drift and
/// the noise field folded into one path, sampled `rate` times a second -
/// with the cut's windows at the edges kept **whole**, so a whip pan still
/// eases exactly the way a plain layer's does. A choreographed sweep is
/// two keys and a curve; chopping it into noise samples would flatten the
/// ease into a polyline, so the windows keep their own two keys each,
/// the wander folded into them too - the path never steps at a window's
/// edge, the sweep simply rides on top of it.
///
/// The field is read at one cycle per ten keys (`rate / 10` cycles a
/// second), so one knob shapes the whole feel; the keys between the
/// samples are linear - at eight to thirty keys a second and a fraction
/// of a percent of reach, linear *is* smooth.
fn wobble_marks<'a>(
    wobble: Wobble,
    span: f64,
    offset: (f64, f64),
    pan: (f64, f64),
    enter: Option<&'a crate::transition::PlateCut>,
    enter_seconds: f64,
    exit: Option<&'a crate::transition::PlateCut>,
    exit_seconds: f64,
) -> Vec<(f64, (f64, f64), Option<&'a Curve>)> {
    let noise = crate::noise::Noise::from_seed(wobble.seed);
    let rate = wobble.rate.clamp(1.0, 30.0);
    let amount = wobble.amount;
    // The drift's linear journey, folded with the field's wander at the
    // same moment: one path, both motions in it, continuous everywhere.
    let path_at = |t: f64| {
        let u = if span > 1e-9 { t / span } else { 0.0 };
        let (x, y) = noise.drift(t * rate / 10.0, 0);
        (
            offset.0 + pan.0 * u + x * amount,
            offset.1 + pan.1 * u + y * amount,
        )
    };
    // The windows that carry position motion: a cut whose `from` is zero
    // (a rack focus) moves nothing here, and its window is no window.
    let enter = enter.filter(|cut| cut.from != (0.0, 0.0));
    let exit = exit.filter(|cut| cut.from != (0.0, 0.0));
    let mut marks: Vec<(f64, (f64, f64), Option<&Curve>)> = Vec::new();
    if let Some(cut) = enter {
        let from = path_at(0.0);
        marks.push((
            0.0,
            (from.0 + cut.from.0, from.1 + cut.from.1),
            Some(&cut.curve),
        ));
        marks.push((enter_seconds, path_at(enter_seconds), None));
    }
    // The samples: strictly inside the windows' remainders. A sample that
    // lands on a window's edge is the same key the window wrote - the
    // dedup below takes the pair of them as one, whichever came first.
    let start = if enter.is_some() { enter_seconds } else { 0.0 };
    let end = if exit.is_some() {
        span - exit_seconds
    } else {
        span
    };
    let steps = (((end - start).max(0.0)) * rate).ceil().min(600.0) as usize;
    for step in 0..=steps {
        let t = if steps == 0 {
            start
        } else {
            start
                + (end - start) * f64::from(u32::try_from(step).unwrap_or(0))
                    / f64::from(u32::try_from(steps).unwrap_or(1))
        };
        marks.push((t, path_at(t), None));
    }
    if let Some(cut) = exit {
        let to = path_at(span);
        marks.push((
            span - exit_seconds,
            path_at(span - exit_seconds),
            Some(&cut.curve),
        ));
        marks.push((span, (to.0 + cut.from.0, to.1 + cut.from.1), None));
    }
    // One time order, no duplicates within a millisecond - a track whose
    // keys do not strictly increase is refused whole (E303), and a wobble
    // that collided with a window edge would trip it.
    marks.sort_by(|a, b| a.0.total_cmp(&b.0));
    marks.dedup_by(|a, b| (a.0 - b.0).abs() <= 0.001);
    marks
}

/// A layer's timeline id: the first plate is `s0.plate`, the second
/// `s0.plate2`, and overlays number the same way on their own side of the
/// comp - so a timeline read by a person says what each layer is.
fn layer_id(scene: usize, side: &str, nth: usize) -> String {
    if nth == 1 {
        format!("s{scene}.{side}")
    } else {
        format!("s{scene}.{side}{nth}")
    }
}

/// The clip a scene layer becomes: a still fitted to the frame, moving on
/// its own clock.
///
/// A cover-fitted layer that pans or sits off-centre is scaled up by its
/// own travel - the overhang each side of a cover picture at scale `s` is
/// `(s - 1) / 2` of the frame, so twice the furthest its centre moves from
/// the middle is the least growth that keeps the edge hidden. A deliberate
/// shrink (`scale` under one) is left alone: a layer that means to show the
/// frame's edges keeps them.
///
/// A choreographed cut rides at the span's edges: the enter side sweeps
/// the layer in from its offset and settles it where its own drift takes
/// over, the exit side carries it away at the end - the same vocabulary
/// (`pan`, `offset`, `scale`) the layer itself speaks, so the drift and
/// the cut read as one motion.
fn layer_clip(
    layer: &SceneLayer,
    width: f64,
    height: f64,
    start: f64,
    span: f64,
    enter: Option<&crate::transition::PlateCut>,
    exit: Option<&crate::transition::PlateCut>,
) -> Map<String, Json> {
    let mut clip = Map::new();
    clip.insert(
        "source".into(),
        json!({ "kind": "image", "asset": layer.asset }),
    );
    clip.insert(
        "fit".into(),
        serde_json::to_value(layer.fit).unwrap_or(json!("cover")),
    );
    clip.insert("start".into(), json!(seconds(start)));
    clip.insert("duration".into(), json!(seconds(span)));

    // The layer's drift, at its parallax rate: a rate multiplies the
    // drift the layer was given, and a layer with no drift of its own
    // takes the classic plate's (four percent of growth, three of slide)
    // before the rate is applied - so `parallax(rate)` alone moves a
    // layer, and three rates on three layers are three depths. A rate
    // that is not a positive finite number is ignored rather than
    // believed: the layer drifts as written.
    let rate = layer
        .parallax
        .filter(|rate| rate.is_finite() && *rate > 0.0);
    let (zoom, pan) = match rate {
        Some(rate) => {
            let (zoom, pan) = if layer.zoom.abs() < 1e-9 && layer.pan == (0.0, 0.0) {
                (0.04, (-0.03, 0.0))
            } else {
                (layer.zoom, layer.pan)
            };
            (zoom * rate, (pan.0 * rate, pan.1 * rate))
        }
        None => (layer.zoom, layer.pan),
    };
    let offset = layer.offset;

    // The cut's windows, held to a third of the span each so the two
    // sides of one boundary never meet inside a single layer's span. A
    // window that lands at or under a millisecond is not a cut - its two
    // keys would round to one time, and a track whose keys do not
    // strictly increase is refused whole (E303) - so it is dropped
    // rather than performed, the way the comp side refuses a
    // zero-length cut.
    let enter_seconds = enter
        .map(|cut| cut.seconds.min(span / 3.0))
        .filter(|seconds| *seconds > 0.001)
        .unwrap_or(0.0);
    let exit_seconds = exit
        .map(|cut| cut.seconds.min(span / 3.0))
        .filter(|seconds| *seconds > 0.001)
        .unwrap_or(0.0);

    // Where the layer sits, in pixels from the frame's corner: the centre,
    // plus its offset as a fraction of the frame, plus its drift.
    let placed = |x: f64, y: f64| {
        json!([
            round(width / 2.0 + x * width),
            round(height / 2.0 + y * height)
        ])
    };

    // The least scale that keeps a cover picture covering while it drifts
    // - and while it wobbles, which is a drift it did not write down: the
    // wobble's amount is spent per axis exactly as the pan's reach is.
    let reach = |from: f64, to: f64| from.abs().max(to.abs());
    let wobble_amount = layer
        .wobble
        .filter(|wobble| {
            wobble.amount.is_finite()
                && wobble.amount > 0.0
                && wobble.rate.is_finite()
                && wobble.rate > 0.0
        })
        .map(|wobble| wobble.amount)
        .unwrap_or(0.0);
    let mut base = layer.scale.max(0.0);
    if matches!(layer.fit, Fit::Cover) && base >= 1.0 {
        let travel = 2.0
            * (reach(offset.0, offset.0 + pan.0) + wobble_amount)
                .max(reach(offset.1, offset.1 + pan.1) + wobble_amount);
        base = base.max(1.0 + travel);
    }

    // Position: the cut's windows at the edges, the layer's own drift
    // between them - and, when the layer wobbles, the noise field the
    // drift rides on, sampled to keys (see [`wobble_marks`]; the cut
    // still owns its own window, edge to edge, unchopped).
    // One list of marks, in time order by construction:
    // 0 < enter_seconds <= span - exit_seconds < span.
    let rest = offset;
    let drift_end = (offset.0 + pan.0, offset.1 + pan.1);
    let mut position_marks: Vec<(f64, (f64, f64), Option<&Curve>)> = Vec::new();
    if let Some(wobble) = layer.wobble.filter(|wobble| {
        wobble.amount.is_finite()
            && wobble.amount > 0.0
            && wobble.rate.is_finite()
            && wobble.rate > 0.0
    }) {
        position_marks = wobble_marks(
            wobble,
            span,
            offset,
            pan,
            enter.filter(|_| enter_seconds > 0.0),
            enter_seconds,
            exit.filter(|_| exit_seconds > 0.0),
            exit_seconds,
        );
    } else {
        if let Some(cut) = enter
            && enter_seconds > 0.0
            && cut.from != (0.0, 0.0)
        {
            position_marks.push((
                0.0,
                (offset.0 + cut.from.0, offset.1 + cut.from.1),
                Some(&cut.curve),
            ));
            position_marks.push((enter_seconds, rest, None));
        } else if pan != (0.0, 0.0) {
            position_marks.push((0.0, rest, None));
        }
        if let Some(cut) = exit
            && exit_seconds > 0.0
            && cut.from != (0.0, 0.0)
        {
            // The exit's curve rides the key that starts the exit's segment -
            // Geneva's `ease` shapes the segment starting at its keyframe, so a
            // curve on the boundary's last key would shape nothing and the
            // sweep would travel a straight line.
            position_marks.push((span - exit_seconds, drift_end, Some(&cut.curve)));
            position_marks.push((
                span,
                (drift_end.0 + cut.from.0, drift_end.1 + cut.from.1),
                None,
            ));
        } else if pan != (0.0, 0.0) {
            position_marks.push((span, drift_end, None));
        }
    }

    let mut transform = Map::new();
    if position_marks.len() >= 2 {
        transform.insert(
            "position".into(),
            json!({ "keyframes": position_marks
                .iter()
                .map(|(at, (x, y), curve)| match curve {
                    Some(curve) => json!([seconds(*at), placed(*x, *y), crate::geneva::ease(curve)]),
                    None => json!([seconds(*at), placed(*x, *y)]),
                })
                .collect::<Vec<Json>>() }),
        );
    } else if offset != (0.0, 0.0) {
        transform.insert("position".into(), placed(offset.0, offset.1));
    }

    // Scale: the same shape - the cut's edge scales at the boundaries,
    // the layer's own zoom between them, and the drift's own end value
    // is what the exit rides from.
    let rest_scale = base;
    let drift_scale = base * (1.0 + zoom);
    let mut scale_marks: Vec<(f64, f64, Option<&Curve>)> = Vec::new();
    if let Some(cut) = enter
        && enter_seconds > 0.0
        && (cut.scale_from - 1.0).abs() > 1e-9
    {
        scale_marks.push((0.0, rest_scale * cut.scale_from, Some(&cut.curve)));
        scale_marks.push((enter_seconds, rest_scale, None));
    } else if zoom.abs() > 1e-9 {
        scale_marks.push((0.0, rest_scale, None));
    }
    if let Some(cut) = exit
        && exit_seconds > 0.0
        && (cut.scale_from - 1.0).abs() > 1e-9
    {
        // As the position above: the curve starts the segment, or it
        // shapes nothing.
        scale_marks.push((span - exit_seconds, drift_scale, Some(&cut.curve)));
        scale_marks.push((span, drift_scale * cut.scale_from, None));
    } else if zoom.abs() > 1e-9 {
        scale_marks.push((span, drift_scale, None));
    }
    if scale_marks.len() >= 2 {
        transform.insert(
            "scale".into(),
            json!({ "keyframes": scale_marks
                .iter()
                .map(|(at, value, curve)| match curve {
                    Some(curve) => json!([seconds(*at), round(*value), crate::geneva::ease(curve)]),
                    None => json!([seconds(*at), round(*value)]),
                })
                .collect::<Vec<Json>>() }),
        );
    } else if (base - 1.0).abs() > 1e-9 {
        transform.insert("scale".into(), json!(round(base)));
    }
    if !transform.is_empty() {
        clip.insert("transform".into(), Json::Object(transform));
    }

    // A rack focus rides the clip's blur effect: the radius keyframed
    // up to the cut on the way out, down from it on the way in.
    let mut blur_marks: Vec<(f64, f64, Option<&Curve>)> = Vec::new();
    if let Some(cut) = enter
        && enter_seconds > 0.0
        && cut.blur_from > 1e-9
    {
        blur_marks.push((0.0, cut.blur_from, Some(&cut.curve)));
        blur_marks.push((enter_seconds, 0.0, None));
    }
    if let Some(cut) = exit
        && exit_seconds > 0.0
        && cut.blur_from > 1e-9
    {
        // And the same once more: the rack's curve belongs to the key
        // the blur rises from.
        blur_marks.push((span - exit_seconds, 0.0, Some(&cut.curve)));
        blur_marks.push((span, cut.blur_from, None));
    }
    if !blur_marks.is_empty() {
        clip.insert(
            "effects".into(),
            json!([{
                "kind": "blur",
                "radius": { "keyframes": blur_marks
                    .iter()
                    .map(|(at, value, curve)| match curve {
                        Some(curve) => json!([seconds(*at), round(*value), crate::geneva::ease(curve)]),
                        None => json!([seconds(*at), round(*value)]),
                    })
                    .collect::<Vec<Json>>() },
            }]),
        );
    }

    // Opacity: a steady number, or a fade up, a hold and a fade down.
    if let Some(opacity) = fade_keys(layer, span) {
        clip.insert("opacity".into(), opacity);
    } else if (layer.opacity - 1.0).abs() > 1e-9 {
        clip.insert("opacity".into(), json!(round(layer.opacity)));
    }

    if !matches!(layer.blend, Blend::Normal)
        && let Json::String(name) = serde_json::to_value(layer.blend).unwrap_or(Json::Null)
    {
        clip.insert("blend".into(), json!(name));
    }
    clip
}

/// The opacity a layer's fades write over a span of `span` seconds: a
/// fade up from transparent, a hold at the layer's own opacity, a fade
/// down at the end. Each fade is held to half the span so the two never
/// cross, and keys that land on the same time are merged - a track whose
/// times do not strictly increase is refused by the engine, and a layer
/// with only one of the two fades used to write two keys at `"0s"` (or
/// two at the span's end) that stopped the whole render. A layer with
/// neither fade gets `None`, and its steady opacity instead.
///
/// A fade curve, when the layer carries one, rides on the key that
/// *starts* each fade's segment - the third element of the keyframe,
/// exactly the way a comp track's keys carry theirs - so the segment
/// itself travels the curve and the hold between the fades does not.
fn fade_keys(layer: &SceneLayer, span: f64) -> Option<Json> {
    if layer.fade_in <= 1e-9 && layer.fade_out <= 1e-9 {
        return None;
    }
    let fade_in = layer.fade_in.min(span / 2.0).max(0.0);
    let fade_out = layer.fade_out.min(span / 2.0).max(0.0);
    // Each mark: `(at, value, starts_a_fade)`.
    let mut marks: Vec<(f64, f64, bool)> = Vec::new();
    if fade_in > 1e-9 {
        marks.push((0.0, 0.0, true));
        marks.push((fade_in, round(layer.opacity), false));
    } else {
        // No fade up: the layer is already there when its span begins.
        marks.push((0.0, round(layer.opacity), false));
    }
    let hold_at = span - fade_out;
    let last_at = marks.last().map(|mark| mark.0).unwrap_or(0.0);
    if fade_out > 1e-9 {
        if hold_at > last_at + 1e-9 {
            marks.push((hold_at, round(layer.opacity), true));
        } else if (hold_at - last_at).abs() <= 1e-9
            && let Some(last) = marks.last_mut()
        {
            // The fade-out starts exactly where the last mark stands -
            // the fade-in's end, the two fades meeting in the middle of
            // the span, which the half-span clamp makes the one shape
            // this can be - so that mark is the key that starts the
            // fade-out's segment, and the layer's curve rides it. Left
            // unflagged, the mark stayed the fade-in's landing key and
            // the way out travelled a straight line.
            last.2 = true;
        }
    }
    marks.push((
        span,
        if fade_out > 1e-9 {
            0.0
        } else {
            round(layer.opacity)
        },
        false,
    ));
    // A mark that lands on the one before it is the same key, not a
    // second: the times must strictly increase or the track is refused.
    let mut keys: Vec<Json> = Vec::new();
    for (at, value, starts_a_fade) in marks {
        if let Some(last) = keys.last()
            && last[0]
                .as_str()
                .and_then(parse_seconds)
                .is_some_and(|t| at - t <= 0.001)
        {
            keys.pop();
        }
        let key = match (&layer.fade_curve, starts_a_fade) {
            (Some(curve), true) => {
                json!([seconds(at.max(0.0)), value, crate::geneva::ease(curve)])
            }
            _ => json!([seconds(at.max(0.0)), value]),
        };
        keys.push(key);
    }
    Some(json!({ "keyframes": keys }))
}

/// A number the timeline can carry cleanly: three decimals, the same
/// precision the comp path writes its keyframe values at.
fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The drift velocity of a scene's plate, in fractions of the frame per
/// second - what a velocity-matched cut reads to hide itself. The first
/// layer that pans says it; a scene of stills speaks with the classic
/// plate's own three-percent slide.
fn plate_velocity(sequence: &Sequence, index: usize) -> (f64, f64) {
    let Some(scene) = sequence.scenes.get(index) else {
        return (0.0, 0.0);
    };
    let span = scene.hold.unwrap_or(scene.comp.duration).max(0.1);
    for layer in &scene.layers {
        let rate = layer
            .parallax
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .unwrap_or(1.0);
        let pan = if layer.zoom.abs() < 1e-9 && layer.pan == (0.0, 0.0) {
            (-0.03, 0.0)
        } else {
            layer.pan
        };
        if pan != (0.0, 0.0) {
            return (pan.0 * rate / span, pan.1 * rate / span);
        }
    }
    (-0.03 / span, 0.0)
}

/// Lay a sequence out as one Geneva timeline.
///
/// Every scene's comp is validated and its operations applied first: a shot
/// that does not make sense stops the plan and names the scene, rather than
/// reaching a render. The layers keep the comp's own ids, prefixed with the
/// scene's index, so two scenes using the same template never collide.
pub fn plan(geneva: &Geneva, sequence: &Sequence) -> Result<Plan, MotionError> {
    plan_with_chain(geneva, sequence, &[])
}

/// The plan, under the compositions already being planned above this
/// one: `chain` names them, and a scene that places one of them is a
/// cycle, not a composition.
fn plan_with_chain(
    geneva: &Geneva,
    sequence: &Sequence,
    chain: &[String],
) -> Result<Plan, MotionError> {
    let mut warnings = Vec::new();
    let mut layers: Vec<Json> = Vec::new();
    let mut assets: Map<String, Json> = Map::new();
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();

    // Shared assets first: plates and sound name them, and they are staged
    // once however many scenes use them.
    for (id, asset) in &sequence.assets {
        let staged = staged_name(id, &asset.src);
        assets.insert(
            id.clone(),
            json!({ "src": staged, "kind": asset_kind(asset) }),
        );
        files.insert(staged, PathBuf::from(&asset.src));
    }

    let mut scene_ends: Vec<f64> = Vec::new();
    let mut holds: Vec<f64> = Vec::new();
    for (index, scene) in sequence.scenes.iter().enumerate() {
        let mut comp = if scene.ops.is_empty() {
            scene.comp.clone()
        } else {
            authoring::apply(&scene.comp, &scene.ops).map_err(|error| MotionError::Engine {
                code: None,
                message: format!("scene {index} ({}): {error}", scene.comp.id),
                path: None,
                help: Some("the batch is applied to the comp before it is placed".into()),
            })?
        };
        // A scene that holds for less than its comp runs the comp shorter, not
        // cut off: every node ends by the new end, so a track keyed as a
        // fraction of a span is still inside its span rather than waving at a
        // time the clip never reaches.
        if let Some(hold) = scene.hold
            && hold < comp.duration
        {
            comp.duration = hold;
        }
        for node in &mut comp.nodes {
            if node.duration > 0.0 {
                // A hold that ends before a node begins leaves the node a
                // zero span: never negative, which validate would read as
                // running backwards and fail the whole reel for. A node of
                // zero duration runs to the comp's end, which a hold that
                // ends before it begins simply means it draws nothing
                // inside the held window.
                node.duration = (node.duration.min(comp.duration - node.start)).max(0.0);
            }
        }
        // The choreographed cuts this scene sits between: the one it
        // enters by (the reel's own opening, or the cut after the scene
        // before it) and the one it leaves by (the cut after this scene
        // - the reel's closing, when nothing follows). The cut writes
        // motion into the comp's own nodes here, before anything is
        // validated or emitted, riding on top of the authored tracks.
        // A velocity match reads the *outgoing* scene's drift, so the
        // enter side is handed the scene before this one's.
        // Before them, the style: the personality is the default hand,
        // and the choreography - the cut, then the pulse - is allowed to
        // override it, not the other way round.
        if let Some(style) = scene.style {
            crate::style::restyle(&mut comp, style);
        }
        // The motion principles, audited while the comp is still the
        // author's: the choreography and the pulse write their own curves
        // after this (correct by construction, and policed by their own
        // tests), so what the lint reads here is what was authored - an
        // entrance that creeps in, an exit that hangs in the doorway, a
        // bounce on a scene that never asked to be playful. The `punchy`
        // style is that ask, and it is honoured; everything else is a
        // finding that rides the plan's warnings, where a critic or an
        // agent can act on it - advice, not refusal, because a comp that
        // breaks a principle on purpose is a comp that should render.
        let playful = scene.style == Some(crate::style::Style::Punchy);
        for note in crate::discipline::audit(&comp, playful) {
            warnings.push(format!("scene {index}: {note}"));
        }
        let enter_cut = if index == 0 {
            sequence.opening.as_ref()
        } else {
            sequence.transitions.get(&(index - 1))
        };
        let exit_cut = sequence.transitions.get(&index);
        let enter_drift = if index == 0 {
            (0.0, 0.0)
        } else {
            plate_velocity(sequence, index - 1)
        };
        if let Some(cut) = enter_cut {
            crate::transition::prepare_comp(
                &mut comp,
                crate::transition::Side::Enter,
                cut,
                enter_drift,
            );
            // A cut whose window is the scene's whole length has no room
            // to settle in: the entrance is clamped to the span, which
            // is shorter than the author wrote. Said, not passed over.
            if cut.seconds() + 1e-9 >= comp.duration {
                warnings.push(format!(
                    "scene {index} enters by a {} that runs its whole {:.3} s: \
                     the entrance is clamped shorter than written",
                    cut.name(),
                    comp.duration
                ));
            }
        }
        if let Some(cut) = exit_cut {
            crate::transition::prepare_comp(
                &mut comp,
                crate::transition::Side::Exit,
                cut,
                plate_velocity(sequence, index),
            );
            if index + 1 == sequence.scenes.len() {
                warnings.push(format!(
                    "scene {index} leaves by a {} with nothing after it: the reel's closing choreography",
                    cut.name()
                ));
            }
            // The exit side does not clamp: a window as long as the
            // scene leaves the exit no frame to play over, and the
            // choreography is skipped whole - the transition appears
            // missing while the plan stays green. Named here.
            if cut.seconds() + 1e-9 >= comp.duration {
                warnings.push(format!(
                    "scene {index} leaves by a {} that runs its whole {:.3} s: \
                     the exit has no frame to play over and is skipped",
                    cut.name(),
                    comp.duration
                ));
            }
        }
        // The pulse, baked in after the operations and the cuts: the
        // beat grid's keys merge with the nodes' own tracks, so a fade,
        // a transition and a heartbeat all ride the same property
        // without one rewriting the other.
        if let Some(pulse) = &scene.pulse {
            let landed = crate::pulse::apply(&mut comp, pulse);
            // The grid is read in the scene's own clock, from its own
            // start - a grid measured against the whole voiceover and
            // fed here unshifted lands every beat outside the scene and
            // pulses nothing, which is the one silent way a beat-driven
            // scene can render without its pulse. Said, not swallowed.
            if landed == 0
                && pulse.amount > 0.0
                && pulse
                    .beats
                    .iter()
                    .any(|beat| beat.is_finite() && *beat >= 0.0)
            {
                warnings.push(format!(
                    "scene {index}: the pulse's beats all fall outside its {:.3} s - \
                     the grid is read in the scene's own clock, from its own start; \
                     a grid measured against the whole reel must be shifted by the \
                     scene's own start before it is written",
                    comp.duration
                ));
            }
        }
        // The grade, baked in after everything else has had its say: a
        // full-bleed box at the very bottom of the paint order, under
        // every node the comp itself paints, over the plate and the
        // lower layers - so the last thing added to the document is the
        // first thing drawn in it.
        // The grade the scene wears: its own when it names one, and its
        // style's when it does not - the personality's colour treatment
        // is a grade like any other, and an explicit grade still wins.
        // The grade needs something under the comp to land on: the
        // plate, a lower layer, a scene this one overlaps. Without it
        // the cast reaches only the curtain, and a grade nobody can see
        // is a grade the author should hear about.
        let worn = scene
            .grade
            .clone()
            .or_else(|| scene.style.map(|style| style.grade()));
        if let Some(grade) = &worn {
            let has_ground = scene.layers.iter().any(|layer| !layer.above);
            crate::grade::apply(&mut comp, grade).map_err(|why| {
                // A grade this wrong was the storyboard's to refuse with
                // its scene named; a builder can still make one, and the
                // plan is where it stops.
                MotionError::Invalid {
                    diagnostics: vec![crate::comp::Diagnostic {
                        path: format!("/scenes/{index}/grade"),
                        message: why,
                        help: None,
                    }],
                }
            })?;
            if !grade.is_identity() && !has_ground {
                warnings.push(format!(
                    "scene {index}: the grade has nothing under the comp to grade - no plate, \
                     no lower layer - so the cast lands on the curtain alone"
                ));
            }
        }
        let problems = comp.validate();
        if !problems.is_empty() {
            return Err(MotionError::Invalid {
                diagnostics: problems,
            });
        }
        // The placement: an authored `at` is the author's; otherwise the
        // scene follows the one before it, less the window its entering cut
        // shares with that scene - the temporal overlap without which a
        // whip pan renders as an exit, a splice, and an entrance.
        let hold = scene.hold.unwrap_or(comp.duration).max(0.1);
        let previous_hold = holds.last().copied().unwrap_or(0.0);
        let overlap = enter_overlap(sequence, index, previous_hold);
        let start = scene
            .at
            .unwrap_or_else(|| scene_ends.last().copied().unwrap_or(0.0) - overlap);
        if scene.at.is_none()
            && let Some(cut) = enter_cut
            && cut.overlaps()
            && cut.seconds() - overlap > 1e-9
        {
            warnings.push(format!(
                "scene {index} enters by a {} that overlaps only {overlap:.3} s of the \
                 {:.3} s the scene before it holds: the rest of the cut plays as a plain \
                 cut, and the boundary shows",
                cut.name(),
                previous_hold
            ));
        }
        scene_ends.push(start + hold);
        holds.push(hold);

        // The scene's own layers: plates under the comp, overlays over it,
        // each with its own drift, blend and span. A layer naming an asset
        // the sequence does not carry stops the plan and names the scene -
        // a storyboard with a missing picture is a mistake to hear about,
        // not one to render around.
        let mut under: Vec<Json> = Vec::new();
        let mut over: Vec<Json> = Vec::new();
        let (mut plates, mut overlays) = (0usize, 0usize);
        for layer in &scene.layers {
            // A layer whose source is a scene places that scene as a
            // nested composition: the scene is emitted once, below, as
            // a reusable `compositions` entry, and this clip shows it
            // with this layer's own timing, drift, blend and fades.
            if let Some(named) = layer.scene.clone() {
                let target = sequence
                    .scene_cards
                    .iter()
                    .position(|card| card.id.as_deref() == Some(named.as_str()));
                let Some(target) = target else {
                    return Err(unsupported(&format!(
                        "scene {index} ({}): a layer names the scene {named:?}, which this storyboard does not define",
                        scene.comp.id
                    )));
                };
                let span_start = layer.start.max(0.0);
                if span_start >= hold {
                    warnings.push(format!(
                        "scene {index}: the layer for the scene {named:?} starts after its scene ends and was dropped"
                    ));
                    continue;
                }
                let span = layer
                    .hold
                    .unwrap_or(hold - span_start)
                    .min(hold - span_start)
                    .max(0.0);
                if span <= 0.0 {
                    warnings.push(format!(
                        "scene {index}: the layer for the scene {named:?} holds no time inside its scene and was dropped"
                    ));
                    continue;
                }
                let mut clip = Map::new();
                clip.insert(
                    "source".into(),
                    json!({
                        "kind": "composition",
                        "composition": format!("s{target}.{named}")
                    }),
                );
                clip.insert("start".into(), json!(seconds(start + span_start)));
                clip.insert("duration".into(), json!(seconds(span)));
                // The layer's own drift, at its parallax rate, and its
                // own fades: a scene placed as a layer moves the way the
                // layer says it does - the same shape the still-layer
                // clip gets, so a plate and a placed scene read alike.
                let rate = layer
                    .parallax
                    .filter(|rate| rate.is_finite() && *rate > 0.0);
                let (zoom, pan) = match rate {
                    Some(rate) => {
                        let (zoom, pan) = if layer.zoom.abs() < 1e-9 && layer.pan == (0.0, 0.0) {
                            (0.04, (-0.03, 0.0))
                        } else {
                            (layer.zoom, layer.pan)
                        };
                        (zoom * rate, (pan.0 * rate, pan.1 * rate))
                    }
                    None => (layer.zoom, layer.pan),
                };
                let placed = |x: f64, y: f64| {
                    json!([
                        round(f64::from(sequence.width) / 2.0 + x * f64::from(sequence.width)),
                        round(f64::from(sequence.height) / 2.0 + y * f64::from(sequence.height))
                    ])
                };
                let mut transform = Map::new();
                if pan != (0.0, 0.0) {
                    transform.insert(
                        "position".into(),
                        json!({ "keyframes": [
                            [seconds(0.0), placed(layer.offset.0, layer.offset.1)],
                            [seconds(span), placed(layer.offset.0 + pan.0, layer.offset.1 + pan.1)],
                        ] }),
                    );
                } else if layer.offset != (0.0, 0.0) {
                    transform.insert("position".into(), placed(layer.offset.0, layer.offset.1));
                }
                let base = layer.scale.max(0.0);
                if zoom.abs() > 1e-9 {
                    transform.insert(
                        "scale".into(),
                        json!({ "keyframes": [
                            [seconds(0.0), round(base)],
                            [seconds(span), round(base * (1.0 + zoom))],
                        ] }),
                    );
                } else if (base - 1.0).abs() > 1e-9 {
                    transform.insert("scale".into(), json!(round(base)));
                }
                if !transform.is_empty() {
                    clip.insert("transform".into(), Json::Object(transform));
                }
                if let Some(opacity) = fade_keys(layer, span) {
                    clip.insert("opacity".into(), opacity);
                } else if (layer.opacity - 1.0).abs() > 1e-9 {
                    clip.insert("opacity".into(), json!(round(layer.opacity)));
                }
                if !matches!(layer.blend, Blend::Normal)
                    && let Json::String(name) =
                        serde_json::to_value(layer.blend).unwrap_or(Json::Null)
                {
                    clip.insert("blend".into(), json!(name));
                }
                let id = if layer.above {
                    overlays += 1;
                    layer_id(index, "overlay", overlays)
                } else {
                    plates += 1;
                    layer_id(index, "plate", plates)
                };
                let stack = if layer.above { &mut over } else { &mut under };
                stack.push(json!({ "id": id, "clips": [Json::Object(clip)] }));
                continue;
            }
            if !sequence.assets.contains_key(&layer.asset) {
                return Err(unsupported(&format!(
                    "scene {index} ({}): a {} names the asset {:?}, which the sequence does not carry",
                    scene.comp.id,
                    if layer.above {
                        "overlay layer"
                    } else {
                        "plate layer"
                    },
                    layer.asset
                )));
            }
            let span_start = layer.start.max(0.0);
            if span_start >= hold {
                warnings.push(format!(
                    "scene {index}: the layer {:?} starts after its scene ends and was dropped",
                    layer.asset
                ));
                continue;
            }
            let span = layer
                .hold
                .unwrap_or(hold - span_start)
                .min(hold - span_start)
                .max(0.0);
            if span <= 0.0 {
                warnings.push(format!(
                    "scene {index}: the layer {:?} holds no time inside its scene and was dropped",
                    layer.asset
                ));
                continue;
            }
            // The cut's plate motion, for the sides this layer's span
            // reaches: an enter cut only reaches a layer that starts
            // with its scene, an exit cut one that is still there at
            // the end - a layer that leaves early never sees the cut.
            let reaches_start = span_start <= 1e-9;
            let reaches_end = span >= hold - span_start - 1e-9;
            let enter_plate = enter_cut.filter(|_| reaches_start).map(|cut| {
                crate::transition::plate_cut(
                    cut,
                    crate::transition::Side::Enter,
                    enter_drift,
                    (f64::from(sequence.width), f64::from(sequence.height)),
                )
            });
            let exit_plate = exit_cut.filter(|_| reaches_end).map(|cut| {
                crate::transition::plate_cut(
                    cut,
                    crate::transition::Side::Exit,
                    plate_velocity(sequence, index),
                    (f64::from(sequence.width), f64::from(sequence.height)),
                )
            });
            let clip = layer_clip(
                layer,
                f64::from(sequence.width),
                f64::from(sequence.height),
                start + span_start,
                span,
                enter_plate.as_ref(),
                exit_plate.as_ref(),
            );
            let id = if layer.above {
                overlays += 1;
                layer_id(index, "overlay", overlays)
            } else {
                plates += 1;
                layer_id(index, "plate", plates)
            };
            let stack = if layer.above { &mut over } else { &mut under };
            stack.push(json!({
                "id": id,
                "clips": [Json::Object(clip)],
            }));
        }
        // The plates go under everything else the scene paints.
        layers.extend(under);

        // The comp's own layers, shifted to the scene's start.
        let (document, scene_warnings) = geneva.timeline(
            &comp,
            &BakeRequest {
                width: Some(sequence.width),
                height: Some(sequence.height),
                fps: Some(sequence.fps),
                ..BakeRequest::default()
            },
        );
        warnings.extend(
            scene_warnings
                .into_iter()
                .map(|warning| format!("scene {index}: {warning}")),
        );

        for (id, asset) in document
            .get("assets")
            .and_then(Json::as_object)
            .cloned()
            .unwrap_or_default()
        {
            let staged = staged_name(
                &format!("s{index}.{id}"),
                asset["src"].as_str().unwrap_or_default(),
            );
            assets.insert(
                format!("s{index}.{id}"),
                json!({ "src": staged, "kind": asset["kind"].clone() }),
            );
            if let Some(asset) = comp.assets.get(&id) {
                files.insert(staged, PathBuf::from(&asset.src));
            }
        }

        for layer in document
            .get("layers")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let id = layer["id"].as_str().unwrap_or("layer").to_string();
            let mut clips = Vec::new();
            for clip in layer["clips"].as_array().cloned().unwrap_or_default() {
                let mut clip = match clip {
                    Json::Object(map) => map,
                    other => return Err(unsupported(&format!("scene {index}: a clip is {other}"))),
                };
                let offset = clip
                    .get("start")
                    .and_then(Json::as_str)
                    .and_then(parse_seconds)
                    .unwrap_or(0.0);
                // A node with `duration: 0` runs to the end of its comp; in a
                // sequence it must stop at the scene's end instead, or it would
                // paint over every scene after it.
                let own = clip
                    .get("duration")
                    .and_then(Json::as_str)
                    .and_then(parse_seconds)
                    .unwrap_or(comp.duration - offset);
                let length = own.min(hold - offset).max(0.0);
                if length <= 0.0 {
                    warnings.push(format!(
                        "scene {index}: the node {id:?} starts after its scene ends and was dropped"
                    ));
                    continue;
                }
                clip.insert("start".into(), json!(seconds(start + offset)));
                clip.insert("duration".into(), json!(seconds(length)));
                // Asset references inside the source move to the scene's own
                // copy of the asset.
                if let Some(source) = clip.get_mut("source").and_then(Json::as_object_mut)
                    && let Some(Json::String(name)) = source.get_mut("asset")
                {
                    *name = format!("s{index}.{name}");
                }
                clips.push(Json::Object(clip));
            }
            if !clips.is_empty() {
                layers.push(json!({ "id": format!("s{index}.{id}"), "clips": clips }));
            }
        }

        // The overlays paint over the comp - still under the curtain, and
        // under any scene that overlaps this one.
        layers.extend(over);
    }

    let duration = scene_ends.iter().copied().fold(0.0_f64, f64::max);

    // The curtain: over everything, opaque at the ends, dipping at each
    // cut and clear in between. A reel of one scene has ends and no
    // cuts, so it opens and closes and never dips - the reel cache's
    // one-scene segments depend on exactly that, and refusing the
    // curtain for them dropped every fade from the cached reels.
    if let Some(curtain) = sequence.curtain {
        // Both fades are held to a quarter of the reel, as the dips are held
        // to an eighth: a fade that outlasts a short reel would otherwise
        // land its far key at or before zero, where the dedup below takes it
        // for the reel's opening key - and a reel that opens clear has lost
        // the fade up it was promised.
        let open = curtain.open.min(duration / 4.0);
        let close = curtain.close.min(duration / 4.0);
        let mut marks: Vec<(f64, f64)> = vec![(0.0, 1.0), (open, 0.0)];
        for (index, end) in scene_ends[..scene_ends.len().saturating_sub(1)]
            .iter()
            .enumerate()
        {
            // A choreographed cut replaces the dip at its boundary: a
            // whip pan and a colour wash at the same frame is two
            // transitions fighting, and the choreography wins.
            if sequence.transitions.contains_key(&index) {
                continue;
            }
            let dip = curtain.dip.min(duration / 8.0);
            marks.push(((end - dip).max(0.0), 0.0));
            marks.push((*end, curtain.depth));
            marks.push((end + dip, 0.0));
        }
        marks.push((duration - close, 0.0));
        marks.push((duration, 1.0));
        marks.sort_by(|a, b| a.0.total_cmp(&b.0));
        // A dip that lands on top of the previous one is a longer dip, not a
        // second: the times must strictly increase or the track is refused.
        let mut keys: Vec<Json> = Vec::new();
        for (at, value) in marks {
            if let Some(last) = keys.last()
                && last[0]
                    .as_str()
                    .and_then(parse_seconds)
                    .is_some_and(|t| at - t <= 0.001)
            {
                keys.pop();
            }
            keys.push(json!([seconds(at.max(0.0)), value]));
        }
        layers.push(json!({
            "id": "curtain",
            "clips": [{
                "source": { "kind": "solid", "color": curtain.color.to_hex() },
                "start": "0s",
                "duration": seconds(duration),
                "opacity": { "keyframes": keys },
            }],
        }));
    }

    let mut audio = Vec::new();
    for bed in &sequence.audio {
        if !sequence.assets.contains_key(&bed.asset) {
            return Err(unsupported(&format!(
                "the audio bed names the asset {:?}, which the sequence does not carry",
                bed.asset
            )));
        }
        audio.push(json!({
            "id": format!("bed.{}", bed.asset),
            "clips": [{
                "asset": bed.asset,
                "start": "0s",
                "duration": seconds(duration),
                "gain_db": bed.gain_db,
                "fade_in": seconds(bed.fade_in.min(duration / 3.0)),
                "fade_out": seconds(bed.fade_out.min(duration / 3.0)),
            }],
        }));
    }

    let mut output = Map::new();
    output.insert("width".into(), json!(sequence.width));
    output.insert("height".into(), json!(sequence.height));
    output.insert("fps".into(), json!(sequence.fps));
    output.insert("duration".into(), json!(seconds(duration)));
    output.insert("background".into(), json!(sequence.background.to_hex()));
    output.insert(
        "encode".into(),
        json!({
            "container": "mp4",
            "video": { "codec": "h264", "crf": sequence.crf },
        }),
    );

    // The named scenes a layer places, each emitted once as a reusable
    // composition: the scene's own stack - plates, comp, overlays -
    // planned on its own, at relative times, and referenced by every
    // layer that shows it. A composition of a composition nests the same
    // way, one level per name, and the engine holds it to eight.
    let mut compositions: Map<String, Json> = Map::new();
    collect_compositions(
        geneva,
        sequence,
        &sequence.scene_cards,
        chain,
        &mut compositions,
        &mut assets,
        &mut files,
    )?;

    let mut document = Map::new();
    document.insert("geneva".into(), json!(crate::geneva::TIMELINE_VERSION));
    document.insert("output".into(), Json::Object(output));
    if !assets.is_empty() {
        document.insert("assets".into(), Json::Object(assets));
    }
    if !compositions.is_empty() {
        document.insert("compositions".into(), Json::Object(compositions));
    }
    document.insert("layers".into(), Json::Array(layers));
    if !audio.is_empty() {
        document.insert("audio".into(), Json::Array(audio));
    }

    Ok(Plan {
        document: Json::Object(document),
        files,
        warnings,
        duration,
        scenes: sequence.scenes.len(),
        source_root: sequence.source_dir.clone(),
    })
}

/// Every named scene a layer of this storyboard places, emitted as a
/// reusable composition: the scene planned on its own (a sub-sequence of
/// itself, at relative times) and its layers taken whole. A composition
/// that itself places a scene brings that composition along, merged
/// under its own globally-unique name, so nesting is one recursive call
/// and the names never collide.
///
/// `chain` is the compositions being planned above this call: a scene
/// that places one of them is a cycle, and the plan says so instead of
/// walking into itself forever. `assets` and `files` receive what the
/// sub-plan staged for the scene's own comp, under the names the parent
/// document uses - the whole-board door stages them itself, and the
/// segment door does not, so the composition's references resolve at
/// both.
fn collect_compositions(
    geneva: &Geneva,
    sequence: &Sequence,
    cards: &[crate::storyboard::SceneCard],
    chain: &[String],
    compositions: &mut Map<String, Json>,
    assets: &mut Map<String, Json>,
    files: &mut BTreeMap<String, PathBuf>,
) -> Result<(), MotionError> {
    // Only the scenes this sequence's layers actually place: a named
    // scene nobody places is not emitted (an unused composition is a
    // warning).
    let referenced: std::collections::HashSet<&str> = sequence
        .scenes
        .iter()
        .flat_map(|scene| scene.layers.iter().filter_map(|l| l.scene.as_deref()))
        .collect();
    let named: Vec<(usize, &crate::storyboard::SceneCard)> = cards
        .iter()
        .enumerate()
        .filter(|(_, card)| {
            card.id
                .as_deref()
                .is_some_and(|name| referenced.contains(name))
        })
        .collect();
    for (index, card) in named {
        let name = card.id.clone().unwrap_or_default();
        let key = format!("s{index}.{name}");
        if chain.contains(&key) {
            return Err(unsupported(&format!(
                "the scene {name:?} places itself, directly or through the scenes it places"
            )));
        }
        if compositions.contains_key(&key) {
            continue;
        }
        // A sub-sequence of this one scene: same frame, same assets, no
        // curtain (a composition is transparent - a `Sequence::new`'s
        // default curtain would paint black fades into it), its own
        // layers at relative times.
        let mut sub = Sequence::new(sequence.width, sequence.height, sequence.fps);
        sub.source_dir = sequence.source_dir.clone();
        sub.assets = sequence.assets.clone();
        sub.scene_cards = sequence.scene_cards.clone();
        sub.curtain = None;
        let mut scene =
            card.scene(index, &sequence.source_dir)
                .map_err(|message| MotionError::Engine {
                    code: None,
                    message,
                    path: None,
                    help: None,
                })?;
        // A composition shows the scene at relative times: its own `at`
        // is where it sits in a whole reel, which is not where it sits
        // inside the composition that places it.
        scene.at = None;
        sub.push(scene);
        let mut deeper_chain = chain.to_vec();
        deeper_chain.push(key.clone());
        let planned = plan_with_chain(geneva, &sub, &deeper_chain)?;
        // Deeper compositions first, so a composition that places a scene
        // has that scene's composition beside it in the document.
        if let Some(deeper) = planned
            .document
            .get("compositions")
            .and_then(Json::as_object)
        {
            for (deeper_key, value) in deeper {
                compositions.insert(deeper_key.clone(), value.clone());
            }
        }
        // The sub-plan staged the scene's comp assets at its own position
        // zero (`s0.<id>`); the parent names the same comp `s{index}.<id>`,
        // so the composition's references are renumbered to the parent's
        // names and the staged entries hoisted beside them. Without this,
        // a placed scene that was not the storyboard's first rendered its
        // pictures from a name the document does not carry (E200) - or
        // worse, silently reused scene zero's file of the same id.
        let mut layers = planned.document["layers"].clone();
        renumber_scene_assets(&mut layers, "s0.", &format!("s{index}."));
        if let Some(staged) = planned.document.get("assets").and_then(Json::as_object) {
            for (asset, entry) in staged {
                let parent_name = match asset.strip_prefix("s0.") {
                    Some(rest) => format!("s{index}.{rest}"),
                    None => asset.clone(),
                };
                assets.entry(parent_name).or_insert_with(|| entry.clone());
            }
        }
        for (staged, source) in &planned.files {
            files
                .entry(staged.clone())
                .or_insert_with(|| source.clone());
        }
        compositions.insert(
            key,
            json!({
                "width": sequence.width,
                "height": sequence.height,
                "background": "transparent",
                "layers": layers,
            }),
        );
    }
    Ok(())
}

/// Rewrite every `source.asset` name under `from` to the same name under
/// `to`, in a layer list the planner emitted: the compositions a
/// sub-plan carries were renumbered at their own level and are left
/// alone - only this level's own layers move.
fn renumber_scene_assets(value: &mut Json, from: &str, to: &str) {
    match value {
        Json::Object(map) => {
            for (field, item) in map.iter_mut() {
                if field == "asset" {
                    if let Json::String(name) = item
                        && let Some(rest) = name.strip_prefix(from)
                    {
                        *name = format!("{to}{rest}");
                    }
                } else {
                    renumber_scene_assets(item, from, to);
                }
            }
        }
        Json::Array(items) => {
            for item in items {
                renumber_scene_assets(item, from, to);
            }
        }
        _ => {}
    }
}

/// A name for an asset inside the render directory: one folder, one file,
/// the id kept so two scenes cannot overwrite each other's pictures.
fn staged_name(id: &str, src: &str) -> String {
    let name = Path::new(src)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "asset".into());
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("assets/{safe}.{name}")
}

/// The Geneva asset kind for an asset, from its kind or its extension.
fn asset_kind(asset: &Asset) -> &'static str {
    crate::geneva::geneva_kind(asset).unwrap_or("image")
}

/// Seconds as the timeline writes them: `2s`, `0.5s`, `7.2s` - three
/// decimals, shared with the reel cache's own timeline writing, so a
/// fade one door writes is a fade the other door refuses to read.
pub(crate) fn seconds(value: f64) -> String {
    let text = format!("{:.3}", value.max(0.0));
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    format!("{}s", if trimmed.is_empty() { "0" } else { trimmed })
}

/// Read `7.2s` or `7.2` back as a number.
fn parse_seconds(text: &str) -> Option<f64> {
    text.trim_end_matches('s').parse().ok()
}

/// A refusal that names what could not be expressed.
fn unsupported(what: &str) -> MotionError {
    MotionError::Unsupported {
        node: "sequence".into(),
        what: what.into(),
        engine: crate::engine::EngineKind::Geneva,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets;

    fn geneva() -> Geneva {
        // The document mapping never runs the binary, so a name is enough.
        Geneva::at("geneva")
    }

    fn reel(scenes: Vec<Scene>) -> Sequence {
        let mut sequence = Sequence::new(1920, 1080, 30.0);
        for scene in scenes {
            sequence.push(scene);
        }
        sequence
    }

    fn layers(plan: &Plan) -> Vec<Json> {
        plan.document["layers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn scenes_are_placed_one_after_another_and_keep_their_own_ids() {
        let plan = plan(
            &geneva(),
            &reel(vec![
                Scene::new(presets::build("funcut.chapter.chip").expect("a preset")),
                Scene::new(presets::build("funcut.sting.wordmark").expect("a preset")),
            ]),
        )
        .expect("plans");

        assert_eq!(plan.scenes, 2);
        let ids: Vec<String> = layers(&plan)
            .iter()
            .map(|layer| layer["id"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(ids.contains(&"s0.chap".to_string()), "{ids:?}");
        assert!(ids.contains(&"s1.mark".to_string()), "{ids:?}");
        assert_eq!(
            ids.iter().filter(|id| *id == "curtain").count(),
            1,
            "{ids:?}"
        );
        // The second scene starts where the first ends, not at zero.
        let first_end = 5.0; // the chip's own length
        let second = layers(&plan)
            .into_iter()
            .find(|layer| layer["id"].as_str().is_some_and(|id| id.starts_with("s1.")))
            .expect("a layer from the second scene");
        let start = second["clips"][0]["start"].as_str().expect("a start");
        assert_eq!(parse_seconds(start).expect("a time"), first_end);
        assert!(
            (plan.duration - (first_end + 3.5)).abs() < 1e-6,
            "{}",
            plan.duration
        );
    }

    #[test]
    fn a_hold_shorter_than_the_comp_cuts_its_nodes_instead_of_leaking_them() {
        let plan = plan(
            &geneva(),
            &reel(vec![
                Scene::new(presets::build("funcut.chapter.chip").expect("a preset")).holding(2.0),
                Scene::new(presets::build("funcut.sting.wordmark").expect("a preset")),
            ]),
        )
        .expect("plans");

        for layer in layers(&plan) {
            let id = layer["id"].as_str().unwrap_or_default();
            if !id.starts_with("s0.") {
                continue;
            }
            for clip in layer["clips"].as_array().cloned().unwrap_or_default() {
                let start = parse_seconds(clip["start"].as_str().unwrap_or("0s")).unwrap_or(0.0);
                let length =
                    parse_seconds(clip["duration"].as_str().unwrap_or("0s")).unwrap_or(0.0);
                assert!(
                    start + length <= 2.0 + 1e-6,
                    "{id} runs to {}",
                    start + length
                );
            }
        }
    }

    #[test]
    fn two_scenes_never_share_an_asset_copy() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.lower-third.glass").expect("a preset")),
            Scene::new(presets::build("funcut.lower-third.glass").expect("a preset")),
        ]);
        sequence.asset(
            "plate",
            Asset {
                src: "assets/plate_dark.jpg".into(),
                kind: None,
            },
        );
        sequence.scenes[0]
            .layers
            .push(SceneLayer::new("plate").zooming(0.06));
        sequence.scenes[1]
            .layers
            .push(SceneLayer::new("plate").zooming(0.06));

        let plan = plan(&geneva(), &sequence).expect("plans");
        let staged = plan.document["assets"].as_object().expect("assets");
        assert!(staged.contains_key("plate"), "{staged:?}");
        let names: Vec<String> = plan.files.keys().cloned().collect();
        let unique: std::collections::BTreeSet<&String> = names.iter().collect();
        assert_eq!(names.len(), unique.len(), "{names:?}");
        // Both scenes' plate layers point at the one staged copy.
        let plates: Vec<Json> = layers(&plan)
            .into_iter()
            .filter(|layer| {
                layer["id"]
                    .as_str()
                    .is_some_and(|id| id.ends_with(".plate"))
            })
            .collect();
        assert_eq!(plates.len(), 2);
        assert_eq!(plates[0]["clips"][0]["source"]["asset"], json!("plate"));
    }

    #[test]
    fn the_curtain_is_opaque_at_both_ends_and_dips_at_the_cut() {
        let plan = plan(
            &geneva(),
            &reel(vec![
                Scene::new(presets::build("funcut.chapter.chip").expect("a preset")),
                Scene::new(presets::build("funcut.sting.wordmark").expect("a preset")),
            ]),
        )
        .expect("plans");
        let curtain = layers(&plan)
            .into_iter()
            .find(|layer| layer["id"] == json!("curtain"))
            .expect("a curtain");
        let keys = curtain["clips"][0]["opacity"]["keyframes"]
            .as_array()
            .cloned()
            .expect("keys");
        let first = &keys[0];
        let last = keys.last().expect("a last key");
        assert_eq!(first[1], json!(1.0));
        assert_eq!(last[1], json!(1.0));
        assert!(
            keys.iter().any(|key| key[1].as_f64() == Some(0.0)),
            "clear in between"
        );
        assert!(
            keys.iter().any(|key| key[1].as_f64() == Some(1.0)),
            "opaque at the cut"
        );
        // Times strictly increase: geneva refuses a track that goes backwards.
        let times: Vec<f64> = keys
            .iter()
            .map(|key| parse_seconds(key[0].as_str().unwrap_or("0")).unwrap_or(0.0))
            .collect();
        assert!(times.windows(2).all(|pair| pair[1] > pair[0]), "{times:?}");
    }

    #[test]
    fn operations_are_applied_before_the_scene_is_placed() {
        let scene = Scene::new(presets::build("funcut.lower-third.glass").expect("a preset"))
            .with_ops(vec![Op::SetSlot {
                slot: "Name".into(),
                value: "Grace Hopper".into(),
            }]);
        let plan = plan(&geneva(), &reel(vec![scene])).expect("plans");
        let text = plan.json();
        assert!(
            text.contains("Grace Hopper"),
            "the filled name reached the timeline"
        );
    }

    #[test]
    fn an_operation_that_cannot_apply_names_its_scene() {
        let error = plan(
            &geneva(),
            &reel(vec![
                Scene::new(presets::build("funcut.chapter.chip").expect("a preset")),
                Scene::new(presets::build("funcut.chapter.chip").expect("a preset")).with_ops(
                    vec![Op::Move {
                        node: "nope".into(),
                        start: 1.0,
                    }],
                ),
            ]),
        )
        .expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("scene 1"), "{message}");
        assert!(message.contains("nope"), "{message}");
    }

    #[test]
    fn a_bed_of_sound_becomes_a_track_and_a_missing_asset_is_refused() {
        let mut sequence = reel(vec![Scene::new(
            presets::build("funcut.chapter.chip").expect("a preset"),
        )]);
        sequence.asset(
            "score",
            Asset {
                src: "assets/score.wav".into(),
                kind: None,
            },
        );
        sequence.bed(AudioBed::new("score").at(-6.0));
        let laid_out = plan(&geneva(), &sequence).expect("plans");
        let audio = laid_out.document["audio"]
            .as_array()
            .cloned()
            .expect("audio");
        assert_eq!(audio[0]["clips"][0]["asset"], json!("score"));
        assert_eq!(audio[0]["clips"][0]["gain_db"], json!(-6.0));
        assert!(
            laid_out
                .files
                .keys()
                .any(|name| name.ends_with("score.wav"))
        );

        sequence.audio = vec![AudioBed::new("nothing")];
        let error = plan(&geneva(), &sequence).expect_err("a bed with no asset is refused");
        assert!(error.to_string().contains("nothing"), "{error}");
    }

    #[test]
    fn a_single_scene_opens_and_closes_but_never_dips() {
        let plan = plan(
            &geneva(),
            &reel(vec![Scene::new(
                presets::build("funcut.chapter.chip").expect("a preset"),
            )]),
        )
        .expect("plans");
        let curtain = layers(&plan)
            .into_iter()
            .find(|layer| layer["id"] == json!("curtain"))
            .expect("a single scene still opens and closes");
        let keys = curtain["clips"][0]["opacity"]["keyframes"]
            .as_array()
            .cloned()
            .expect("keyframes");
        // No cuts, so no dips: the opening fade, the closing fade, and
        // nothing in between - exactly the shape a cached segment needs
        // at every boundary it did not cut at.
        assert_eq!(keys.len(), 4, "{keys:?}");
        assert_eq!(keys[0][1], json!(1.0), "{keys:?}");
        assert_eq!(keys.last().unwrap()[1], json!(1.0), "{keys:?}");
    }

    /// A reel shorter than the curtain's fades still opens on the fade up it
    /// was promised: a close key pushed to zero must not take the opening
    /// key's place.
    #[test]
    fn a_reel_shorter_than_the_curtains_fades_still_opens_opaque() {
        let plan = plan(
            &geneva(),
            &reel(vec![
                Scene::new(presets::build("funcut.chapter.chip").expect("a preset")).holding(0.3),
                Scene::new(presets::build("funcut.sting.wordmark").expect("a preset")).holding(0.3),
            ]),
        )
        .expect("plans");
        assert!(plan.duration < 1.0, "{}", plan.duration);
        let curtain = layers(&plan)
            .into_iter()
            .find(|layer| layer["id"] == json!("curtain"))
            .expect("a curtain");
        let keys = curtain["clips"][0]["opacity"]["keyframes"]
            .as_array()
            .cloned()
            .expect("keys");
        // Opaque at the top, and the fade up runs from there rather than
        // starting already clear.
        assert_eq!(keys[0][1], json!(1.0), "{keys:?}");
        assert_eq!(
            parse_seconds(keys[0][0].as_str().unwrap_or("0s")).unwrap_or(-1.0),
            0.0,
            "{keys:?}"
        );
        let second = parse_seconds(keys[1][0].as_str().unwrap_or("0s")).unwrap_or(-1.0);
        assert!(second > 0.0, "the clear key comes after the top: {keys:?}");
        // And the close fades down into opacity, not up from clear.
        assert_eq!(keys.last().unwrap()[1], json!(1.0), "{keys:?}");
    }

    #[test]
    fn the_plan_is_a_geneva_document_with_one_opaque_master_frame() {
        let plan = plan(
            &geneva(),
            &reel(vec![Scene::new(
                presets::build("funcut.chapter.chip").expect("a preset"),
            )]),
        )
        .expect("plans");
        assert_eq!(plan.document["geneva"], json!("1.1"));
        assert_eq!(plan.document["output"]["width"], json!(1920));
        assert_eq!(
            plan.document["output"]["encode"]["video"]["codec"],
            json!("h264")
        );
        assert_eq!(plan.document["output"]["background"], json!("#05060aff"));
    }

    #[test]
    fn a_hold_shorter_than_a_late_node_leaves_a_valid_scene() {
        let mut comp = presets::build("funcut.chapter.chip").expect("a preset");
        // Every node begins after the hold ends: the clamp leaves each a
        // zero span rather than a negative one, and the reel still plans.
        for node in &mut comp.nodes {
            node.start = comp.duration + 1.0;
            node.duration = 2.0;
        }
        let mut scene = Scene::new(comp);
        scene.hold = Some(0.5);
        let planned = plan(&geneva(), &reel(vec![scene]));
        assert!(planned.is_ok(), "{:?}", planned.err());
    }

    /// The ids of a plan's layers, in paint order - bottom first.
    fn ids(plan: &Plan) -> Vec<String> {
        layers(plan)
            .iter()
            .map(|layer| layer["id"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    fn layered() -> Sequence {
        let mut sequence = Sequence::new(1920, 1080, 30.0);
        for (id, file) in [
            ("back", "assets/plate_dark.jpg"),
            ("mid", "assets/plate_studio.jpg"),
            ("leak", "assets/leak.png"),
            ("grain", "assets/grain.png"),
        ] {
            sequence.asset(
                id,
                Asset {
                    src: file.into(),
                    kind: None,
                },
            );
        }
        let scene = Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
            .holding(4.0)
            .over("back", 0.04)
            .layer(SceneLayer::new("mid").panning(-0.03, 0.0).fading(0.5, 0.8))
            .layer(
                SceneLayer::new("leak")
                    .blending(Blend::Screen)
                    .at_offset(-0.25, -0.2)
                    .scaled(0.8)
                    .fading(0.4, 0.4)
                    .above(),
            )
            .layer(SceneLayer::new("grain").opacity(0.08).above());
        sequence.push(scene);
        sequence
    }

    #[test]
    fn layers_stack_plates_under_the_comp_and_overlays_over_it() {
        let plan = plan(&geneva(), &layered()).expect("plans");
        let order = ids(&plan);
        let comp_at = order
            .iter()
            .position(|id| {
                id.starts_with("s0.") && !id.contains("plate") && !id.contains("overlay")
            })
            .expect("a comp layer");
        let back_at = order
            .iter()
            .position(|id| id == "s0.plate")
            .expect("plate one");
        let mid_at = order
            .iter()
            .position(|id| id == "s0.plate2")
            .expect("plate two");
        let leak_at = order
            .iter()
            .position(|id| id == "s0.overlay")
            .expect("overlay one");
        let grain_at = order
            .iter()
            .position(|id| id == "s0.overlay2")
            .expect("overlay two");
        // Bottom to top: back, mid, the comp, the light leak, the grain,
        // and the curtain over everything - one scene, so it opens and
        // closes and never dips.
        assert!(back_at < mid_at, "{order:?}");
        assert!(mid_at < comp_at, "{order:?}");
        assert!(comp_at < leak_at, "{order:?}");
        assert!(leak_at < grain_at, "{order:?}");
        assert_eq!(order.last().unwrap(), "curtain", "{order:?}");
    }

    #[test]
    fn a_layer_moves_on_its_own_clock() {
        let plan = plan(&geneva(), &layered()).expect("plans");
        let by_id = |id: &str| {
            layers(&plan)
                .into_iter()
                .find(|layer| layer["id"] == json!(id))
                .unwrap_or_else(|| panic!("no layer {id}"))
        };

        // The mid plate drifts left three percent of a frame, fading in and
        // out - over the scene's own four seconds, from its own start.
        let mid = by_id("s0.plate2");
        let clip = &mid["clips"][0];
        assert_eq!(clip["start"], json!("0s"));
        assert_eq!(clip["duration"], json!("4s"));
        let position = &clip["transform"]["position"]["keyframes"];
        // 960 - 0.03 * 1920 = 902.4, from the centre 960.
        assert_eq!(position[0][1], json!([960.0, 540.0]), "{position:?}");
        assert_eq!(position[1][1], json!([902.4, 540.0]), "{position:?}");
        // A cover layer that pans grows by its own travel: 1 + 2 * 0.03.
        assert_eq!(clip["transform"]["scale"], json!(1.06), "{clip:?}");
        let opacity = &clip["opacity"]["keyframes"];
        assert_eq!(opacity[0], json!(["0s", 0.0]), "{opacity:?}");
        assert_eq!(opacity[1], json!(["0.5s", 1.0]), "{opacity:?}");
        assert_eq!(opacity[2], json!(["3.2s", 1.0]), "{opacity:?}");
        assert_eq!(opacity[3], json!(["4s", 0.0]), "{opacity:?}");

        // The light leak blends screen, sits off-centre at four fifths size,
        // and keeps the size it asked for: a shrink is a shape, not a cover.
        let leak = by_id("s0.overlay");
        let clip = &leak["clips"][0];
        assert_eq!(clip["blend"], json!("screen"));
        assert_eq!(
            clip["transform"]["position"],
            json!([480.0, 324.0]),
            "{clip:?}"
        );
        assert_eq!(clip["transform"]["scale"], json!(0.8), "{clip:?}");

        // The grain is a steady opacity, no fades.
        let grain = by_id("s0.overlay2");
        assert_eq!(grain["clips"][0]["opacity"], json!(0.08));
    }

    #[test]
    fn a_layer_with_its_own_span_lands_inside_the_scene() {
        let mut sequence = layered();
        // A foreground card that arrives at 1.2 s and leaves at 3 s, in a
        // scene that holds four.
        sequence.scenes[0]
            .layers
            .push(SceneLayer::new("mid").starting_at(1.2).holding(1.8).above());
        let planned = plan(&geneva(), &sequence).expect("plans");
        let card = layers(&planned)
            .into_iter()
            .find(|layer| layer["id"] == json!("s0.overlay3"))
            .expect("the third overlay");
        let clip = &card["clips"][0];
        assert_eq!(clip["start"], json!("1.2s"));
        assert_eq!(clip["duration"], json!("1.8s"));

        // A layer that outlives its scene is cut to the scene's end, and one
        // that starts after it ends is dropped with a warning, not a
        // backwards track.
        sequence.scenes[0].layers.pop();
        sequence.scenes[0]
            .layers
            .push(SceneLayer::new("mid").holding(9.0));
        sequence.scenes[0]
            .layers
            .push(SceneLayer::new("mid").starting_at(9.0));
        let replanned = plan(&geneva(), &sequence).expect("plans");
        let plate3 = layers(&replanned)
            .into_iter()
            .find(|layer| layer["id"] == json!("s0.plate3"))
            .expect("the third plate");
        assert_eq!(plate3["clips"][0]["duration"], json!("4s"));
        assert!(
            !ids(&replanned).contains(&"s0.plate4".to_string()),
            "the late layer was dropped"
        );
        assert!(
            replanned
                .warnings
                .iter()
                .any(|warning: &String| warning.contains("starts after its scene ends")),
            "{:?}",
            replanned.warnings
        );
    }

    #[test]
    fn a_layer_naming_an_unknown_asset_is_refused() {
        let mut sequence = layered();
        sequence.scenes[0].layers.push(SceneLayer::new("nowhere"));
        let error = plan(&geneva(), &sequence).expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("scene 0"), "{message}");
        assert!(message.contains("nowhere"), "{message}");
    }

    #[test]
    fn the_classic_over_call_is_still_one_drifting_plate() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.chapter.chip").expect("a preset")).over("p", 0.05),
        ]);
        sequence.asset(
            "p",
            Asset {
                src: "assets/plate.jpg".into(),
                kind: None,
            },
        );
        let plan = plan(&geneva(), &sequence).expect("plans");
        let plate = layers(&plan)
            .into_iter()
            .find(|layer| layer["id"] == json!("s0.plate"))
            .expect("the plate");
        let clip = &plate["clips"][0];
        assert_eq!(clip["fit"], json!("cover"));
        assert_eq!(
            clip["transform"]["scale"]["keyframes"][1],
            json!(["5s", 1.05])
        );
    }

    #[test]
    fn fades_that_outlast_their_span_never_cross() {
        let mut sequence = layered();
        // Four seconds of fades in a two-second hold: each is clamped to
        // half the span, and the hold key between them is dropped.
        sequence.scenes[0].layers.push(
            SceneLayer::new("mid")
                .fading(4.0, 4.0)
                .starting_at(0.0)
                .holding(2.0),
        );
        let plan = plan(&geneva(), &sequence).expect("plans");
        let plate = layers(&plan)
            .into_iter()
            .find(|layer| layer["id"] == json!("s0.plate3"))
            .expect("the fading plate");
        let keys = &plate["clips"][0]["opacity"]["keyframes"];
        let times: Vec<&Json> = keys.as_array().unwrap().iter().collect();
        // Strictly increasing times, or the engine refuses the track.
        for pair in times.windows(2) {
            let a = parse_seconds(pair[0][0].as_str().unwrap_or("0s")).unwrap_or(-1.0);
            let b = parse_seconds(pair[1][0].as_str().unwrap_or("0s")).unwrap_or(-1.0);
            assert!(b > a, "{keys:?}");
        }
        assert_eq!(keys.as_array().unwrap().len(), 3, "{keys:?}");
    }

    /// A whip pan smears both plates through the cut, not just the comp:
    /// the outgoing plate racks up into the blur at the boundary and is
    /// gone into it, the incoming resolves out of the same radius - the
    /// motion blur that makes a whip read as one camera move and not a
    /// slide. The plate side is where the plan draws it, so the plan is
    /// where the test reads it, exactly the way the engine would.
    #[test]
    fn a_whip_pan_smears_both_plates_through_the_cut() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.chapter.chip").expect("a preset"))
                .over("p0", 0.05)
                .holding(4.0),
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .over("p1", 0.05)
                .holding(4.0),
        ]);
        sequence.asset(
            "p0",
            Asset {
                src: "assets/plate.jpg".into(),
                kind: None,
            },
        );
        sequence.asset(
            "p1",
            Asset {
                src: "assets/plate.jpg".into(),
                kind: None,
            },
        );
        sequence.with_cut(
            0,
            crate::transition::Cut::WhipPan {
                direction: crate::transition::Direction::Left,
                seconds: 0.24,
                blur: 12.0,
            },
        );
        let plan = plan(&geneva(), &sequence).expect("plans");
        let radius_keys = |id: &str| {
            let plate = layers(&plan)
                .into_iter()
                .find(|layer| layer["id"] == json!(id))
                .unwrap_or_else(|| panic!("the plate {id}"));
            let effects = plate["clips"][0]["effects"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let blur = effects
                .into_iter()
                .find(|effect| effect["kind"] == json!("blur"))
                .unwrap_or_else(|| panic!("the plate {id} carries no blur effect"));
            blur["radius"]["keyframes"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| panic!("the plate {id} blur is not keyframed"))
        };
        let outgoing = radius_keys("s0.plate");
        let incoming = radius_keys("s1.plate");
        // Out racks UP into the smear: sharp at rest, the full radius
        // at the boundary - its last key is the boundary itself.
        assert_eq!(outgoing.first().unwrap()[1], json!(0.0));
        assert_eq!(outgoing.last().unwrap()[1], json!(12.0));
        // In resolves OUT of it: the full radius at its first frame,
        // sharp again by the time it settles.
        assert_eq!(incoming.first().unwrap()[1], json!(12.0));
        assert_eq!(incoming.last().unwrap()[1], json!(0.0));
    }

    // The scene builder's three first-class options, asserted on the
    // emitted timeline: three parallax rates, three blends, three delays,
    // all different, all in the document the engine actually draws.
    #[test]
    fn three_parallax_rates_emit_three_different_drifts() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(SceneLayer::new("back").parallax(0.5))
                .layer(SceneLayer::new("mid").parallax(1.0))
                .layer(SceneLayer::new("front").parallax(2.0)),
        ]);
        sequence.asset(
            "back",
            Asset {
                src: "back.png".into(),
                kind: None,
            },
        );
        sequence.asset(
            "mid",
            Asset {
                src: "mid.png".into(),
                kind: None,
            },
        );
        sequence.asset(
            "front",
            Asset {
                src: "front.png".into(),
                kind: None,
            },
        );
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let drift_of = |id: &str| -> (f64, f64) {
            let layer = planned.document["layers"]
                .as_array()
                .expect("layers")
                .iter()
                .find(|l| l["id"] == json!(id))
                .unwrap_or_else(|| panic!("no layer {id}"));
            let clip = &layer["clips"][0];
            let transform = &clip["transform"];
            let scale_end = transform["scale"]["keyframes"][1][1].as_f64().unwrap();
            let pan_end = transform["position"]["keyframes"][1][1].as_array().unwrap();
            (scale_end, pan_end[0].as_f64().unwrap())
        };
        let (back_scale, back_x) = drift_of("s0.plate");
        let (mid_scale, mid_x) = drift_of("s0.plate2");
        let (front_scale, front_x) = drift_of("s0.plate3");
        // The classic drift, scaled by each rate: the far layer at 0.5
        // drifts least, the near one at 2.0 most, and the middle at 1.0
        // exactly the plate's own.
        assert!(
            back_scale < mid_scale && mid_scale < front_scale,
            "scales {back_scale} < {mid_scale} < {front_scale}"
        );
        assert!(
            back_x > mid_x && mid_x > front_x,
            "the pan goes left, so the drift is most negative at the fastest rate: {back_x} > {mid_x} > {front_x}"
        );
        // The rate is a multiple: double the rate, double the drift. The
        // plate's own scale keyframe is 1 + 2 * |pan| for the cover
        // overhang, so the relation is checked on the pan, which is pure.
        assert!((back_x - 960.0).abs() > 1e-6, "the far layer moves");
        assert!(
            ((front_x - 960.0) / (back_x - 960.0) - 4.0).abs() < 0.01,
            "twice the rate is four times the travel of half of it: {front_x} vs {back_x}"
        );
    }

    #[test]
    fn a_layer_with_its_own_drift_has_the_rate_scale_it() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(
                    SceneLayer::new("back")
                        .zooming(0.10)
                        .panning(-0.06, 0.0)
                        .parallax(0.5),
                ),
        ]);
        sequence.asset(
            "back",
            Asset {
                src: "back.png".into(),
                kind: None,
            },
        );
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let clip = &planned.document["layers"][0]["clips"][0];
        let end_scale = clip["transform"]["scale"]["keyframes"][1][1]
            .as_f64()
            .unwrap();
        let pan_end = clip["transform"]["position"]["keyframes"][1][1][0]
            .as_f64()
            .unwrap();
        // Half of ten percent growth lands on the cover base 1.06, and
        // half of six percent of the width left of centre is 960 - 57.6.
        assert!((end_scale - 1.06 * 1.05).abs() < 0.01, "{end_scale}");
        assert!((pan_end - (960.0 - 0.03 * 1920.0)).abs() < 0.5, "{pan_end}");
    }

    #[test]
    fn a_layer_without_a_rate_drifts_as_written() {
        // The field is an Option, and None is the whole of backwards
        // compatibility: the layer built before parallax existed renders
        // exactly the timeline it always did.
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(SceneLayer::new("back").zooming(0.05)),
        ]);
        sequence.asset(
            "back",
            Asset {
                src: "back.png".into(),
                kind: None,
            },
        );
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let clip = &planned.document["layers"][0]["clips"][0];
        let keys = &clip["transform"]["scale"]["keyframes"];
        assert_eq!(keys[0][1], json!(1.0));
        assert_eq!(keys[1][1], json!(1.05));
        assert!(
            clip["transform"].get("position").is_none(),
            "no pan was written"
        );
    }

    /// The wobble, held to its three contracts: deterministic (the same
    /// seed answers the same path, and the plan is byte-stable), bounded
    /// (the wander stays inside the amount the layer named) and sampled
    /// densely enough to read as motion - and never so densely a cut's
    /// own window is chopped into it.
    #[test]
    fn a_wobbling_layer_reads_a_seeded_field_into_its_keys() {
        let wobble_layer = || {
            SceneLayer::new("back")
                .panning(-0.03, 0.0)
                .wobbling(Wobble {
                    seed: 2026,
                    amount: 0.012,
                    rate: 10.0,
                })
        };
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(wobble_layer()),
        ]);
        sequence.asset(
            "back",
            Asset {
                src: "back.png".into(),
                kind: None,
            },
        );
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let clip = &planned.document["layers"][0]["clips"][0];
        let keys = clip["transform"]["position"]["keyframes"]
            .as_array()
            .expect("the wobble writes a position track");
        // Dense: ten keys a second over four seconds, plus the ends.
        assert!(
            keys.len() >= 38 && keys.len() <= 45,
            "sampled at about the rate: {} keys",
            keys.len()
        );
        // Strictly increasing, and the track starts and ends on the span.
        let times: Vec<f64> = keys
            .iter()
            .map(|key| parse_seconds(key[0].as_str().expect("a time")).expect("a time"))
            .collect();
        for pair in times.windows(2) {
            assert!(pair[0] < pair[1], "strictly increasing: {times:?}");
        }
        assert_eq!(times.first().copied(), Some(0.0));
        // Bounded: the wander plus the pan never leaves the reach the
        // layer named, frame-wise, of the centre.
        for key in keys {
            let x = key[1][0].as_f64().expect("pixels");
            let y = key[1][1].as_f64().expect("pixels");
            assert!(
                (960.0 - (0.03 + 0.012) * 1920.0..=960.0 + 0.012 * 1920.0).contains(&x),
                "x wandered past its reach: {x}"
            );
            assert!(
                (540.0 - 0.012 * 1080.0..=540.0 + 0.012 * 1080.0).contains(&y),
                "y wandered past its reach: {y}"
            );
        }
        // Deterministic: the same seed, the same path, byte for byte.
        let mut again = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(wobble_layer()),
        ]);
        again.asset(
            "back",
            Asset {
                src: "back.png".into(),
                kind: None,
            },
        );
        let replanned = plan(&geneva(), &again).expect("a plan");
        assert_eq!(
            replanned.document["layers"][0]["clips"][0]["transform"]["position"],
            clip["transform"]["position"],
            "the same seed answers the same path"
        );
    }

    /// A different seed is a different world, and the seed is the only
    /// difference: two layers on one field read two channels of it and
    /// never wobble in step, which is the point of a channel number.
    #[test]
    fn a_different_seed_wobbles_a_different_path() {
        let layer_of = |seed: u64| {
            let mut sequence = reel(vec![
                Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                    .holding(4.0)
                    .layer(SceneLayer::new("back").wobbling(Wobble {
                        seed,
                        amount: 0.012,
                        rate: 10.0,
                    })),
            ]);
            sequence.asset(
                "back",
                Asset {
                    src: "back.png".into(),
                    kind: None,
                },
            );
            let planned = plan(&geneva(), &sequence).expect("a plan");
            planned.document["layers"][0]["clips"][0]["transform"]["position"].clone()
        };
        assert_ne!(layer_of(1), layer_of(2), "two seeds, two paths");
    }

    /// A cover layer grows to cover its own wobble, the way it grows for a
    /// pan: an edge that shows behind a wandering plate is the one thing
    /// the fit promised never to do.
    #[test]
    fn a_cover_layer_grows_to_cover_its_own_wobble() {
        let layer_of = |wobble: Option<Wobble>| {
            let mut layer = SceneLayer::new("back");
            if let Some(wobble) = wobble {
                layer = layer.wobbling(wobble);
            }
            let mut sequence = reel(vec![
                Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                    .holding(4.0)
                    .layer(layer),
            ]);
            sequence.asset(
                "back",
                Asset {
                    src: "back.png".into(),
                    kind: None,
                },
            );
            let planned = plan(&geneva(), &sequence).expect("a plan");
            planned.document["layers"][0]["clips"][0].clone()
        };
        let plain = layer_of(None);
        let wobbling = layer_of(Some(Wobble {
            seed: 5,
            amount: 0.02,
            rate: 8.0,
        }));
        let plain_scale = plain["transform"]["scale"].as_f64().unwrap_or(1.0);
        let wobble_scale = wobbling["transform"]["scale"].as_f64().unwrap_or(1.0);
        assert!(
            wobble_scale >= plain_scale + 2.0 * 0.02 - 1e-9,
            "the wobble's reach is grown into the base scale: {plain_scale} -> {wobble_scale}"
        );
    }

    #[test]
    fn three_blends_and_three_delays_land_in_the_timeline() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(
                    SceneLayer::new("vignette")
                        .blend(Blend::Multiply)
                        .delay(0.0),
                )
                .layer(
                    SceneLayer::new("leak")
                        .blend(Blend::Screen)
                        .delay(0.4)
                        .above(),
                )
                .layer(
                    SceneLayer::new("difference")
                        .blend(Blend::Difference)
                        .delay(1.2)
                        .above(),
                ),
        ]);
        for id in ["vignette", "leak", "difference"] {
            sequence.asset(
                id,
                Asset {
                    src: format!("{id}.png"),
                    kind: None,
                },
            );
        }
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let layers = planned.document["layers"].as_array().expect("layers");
        let read = |id: &str| {
            layers
                .iter()
                .find(|l| l["id"] == json!(id))
                .unwrap_or_else(|| panic!("no layer {id}"))["clips"][0]
                .clone()
        };
        // Three different blend values, each on its own layer.
        assert_eq!(read("s0.plate")["blend"], json!("multiply"));
        assert_eq!(read("s0.overlay")["blend"], json!("screen"));
        assert_eq!(read("s0.overlay2")["blend"], json!("difference"));
        // Three different delays: the timeline carries each as the clip's
        // start, offset from the scene's own.
        assert_eq!(read("s0.plate")["start"], json!("0s"));
        assert_eq!(read("s0.overlay")["start"], json!("0.4s"));
        assert_eq!(read("s0.overlay2")["start"], json!("1.2s"));
    }

    #[test]
    fn a_storyboard_layer_can_carry_its_rate() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "assets": { "back": "b.png", "mid": "m.png" },
                 "scenes": [ { "preset": "funcut.sting.wordmark", "hold": 4.0,
                   "layers": [ { "asset": "back", "parallax": 0.5 },
                               { "asset": "mid", "zoom": 0.08, "pan": [-0.04, 0.0], "parallax": 2.0 } ] } ] }"##,
        ).expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let back = &sequence.scenes[0].layers[0];
        let mid = &sequence.scenes[0].layers[1];
        assert_eq!(back.parallax, Some(0.5));
        assert_eq!(mid.parallax, Some(2.0));
        assert_eq!(
            mid.zoom, 0.08,
            "the layer's own numbers stay what they said"
        );
    }

    /// The easing library, exposed: a scene authored with `steps(8)` for a
    /// typewriter reveal, `linear(0, 1.4, 0.9, 1)` for a bounce, and a
    /// different curve per layer - the header and the body no longer
    /// share one. The engine plays every one of them as written; what was
    /// missing was a way to say it.
    #[test]
    fn per_layer_easing_lands_in_the_timeline_as_written() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(
                    SceneLayer::new("header")
                        .fading(0.6, 0.6)
                        .eased(Curve::TYPEWRITER),
                )
                .layer(
                    SceneLayer::new("body")
                        .fading(0.6, 0.6)
                        .eased(Curve::bounce()),
                ),
        ]);
        for id in ["header", "body"] {
            sequence.asset(
                id,
                Asset {
                    src: format!("{id}.png"),
                    kind: None,
                },
            );
        }
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let read = |id: &str| {
            planned.document["layers"]
                .as_array()
                .expect("layers")
                .iter()
                .find(|layer| layer["id"] == json!(id))
                .unwrap_or_else(|| panic!("no layer {id}"))["clips"][0]["opacity"]["keyframes"]
                .as_array()
                .expect("keyframes")
                .clone()
        };
        let header = read("s0.plate");
        let body = read("s0.plate2");
        // The typewriter's clacks: steps(8, jump-end) on the key that
        // starts each fade's segment.
        assert_eq!(
            header[0][2],
            json!({ "steps": [8, "jump-end"] }),
            "{header:?}"
        );
        // The bounce's overshoot: linear(0, 1.4, 0.9, 1) as its stops.
        assert_eq!(
            body[0][2]["linear"][1],
            json!([0.3333333333333333, 1.4]),
            "{body:?}"
        );
        assert_eq!(body[0][2]["linear"].as_array().map(Vec::len), Some(4));
        // And the two layers really do carry different curves.
        assert_ne!(header[0][2], body[0][2]);
        // The holds between the fades carry no curve at all.
        assert_eq!(header[1].as_array().map(Vec::len), Some(2), "{header:?}");
    }

    /// A layer with no fade curve writes the keys it always wrote: the
    /// straight fade, two values and no third element.
    #[test]
    fn a_layer_without_a_curve_writes_the_plain_fade_it_always_did() {
        let layer = SceneLayer::new("plate").fading(0.5, 0.5);
        let keys = fade_keys(&layer, 4.0).expect("keyframes");
        let keys = keys["keyframes"].as_array().expect("keys");
        assert!(
            keys.iter()
                .all(|key| key.as_array().map(Vec::len) == Some(2)),
            "{keys:?}"
        );
    }

    /// A storyboard names the easing by its library name, and the name
    /// lands on the layer the plan carries.
    #[test]
    fn a_storyboard_names_a_layers_easing_from_the_library() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "assets": { "back": "b.png" },
                 "scenes": [ { "preset": "funcut.sting.wordmark", "hold": 4.0,
                   "layers": [ { "asset": "back", "fadeIn": 0.6, "fadeOut": 0.6,
                                 "fadeCurve": "bounce" } ] } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        assert_eq!(
            sequence.scenes[0].layers[0].fade_curve,
            Some(Curve::bounce()),
            "the name is the curve"
        );
        // An unknown name is refused with the library spelled out, not
        // silently ignored as a plain fade.
        let refused = crate::storyboard::Storyboard::parse(
            r##"{ "assets": { "back": "b.png" },
                 "scenes": [ { "preset": "funcut.sting.wordmark",
                   "layers": [ { "asset": "back", "fadeCurve": "wobbly" } ] } ] }"##,
        )
        .expect_err("an unknown curve name is refused");
        assert!(refused.contains("wobbly"), "{refused}");
        assert!(refused.contains("typewriter"), "{refused}");
    }

    /// The done-when of the step: a two-scene timeline with a whip pan
    /// and a zoom-through, choreographed rather than spliced - both
    /// sides of each boundary carry the motion, in the plates and in the
    /// comps, and the curtain stands down at the choreographed cuts.
    #[test]
    fn a_two_scene_timeline_with_a_whip_pan_and_a_zoom_through_is_choreographed() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.caption.kinetic").expect("a preset")).holding(4.0),
            Scene::new(presets::build("funcut.chapter.chip").expect("a preset")).holding(4.0),
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset")).holding(4.0),
        ]);
        sequence.asset(
            "plate",
            Asset {
                src: "plate.png".into(),
                kind: None,
            },
        );
        for scene in &mut sequence.scenes {
            scene.layers.push(SceneLayer::new("plate").zooming(0.04));
        }
        // The cut after scene 0 whips left; the cut after scene 1 zooms
        // through; the reel itself opens onto its first frame.
        sequence.with_cut(
            0,
            crate::transition::Cut::WhipPan {
                direction: crate::transition::Direction::Left,
                seconds: 0.24,
                blur: 12.0,
            },
        );
        sequence.with_cut(1, crate::transition::Cut::ZoomThrough { seconds: 0.35 });
        sequence.opens_with(crate::transition::Cut::ZoomThrough { seconds: 0.35 });
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let layers = planned.document["layers"].as_array().expect("layers");
        let read = |id: &str| {
            layers
                .iter()
                .find(|layer| layer["id"] == json!(id))
                .unwrap_or_else(|| panic!("no layer {id}"))["clips"][0]
                .clone()
        };

        // Scene 0's plate sweeps out to the left over its last 0.24s:
        // the exit key sits at the scene's end (4s), its position left
        // of centre. The accelerating curve rides the key that *starts*
        // the exit's segment - Geneva's `ease` shapes the segment
        // starting at its keyframe, so a curve on the boundary's own
        // last key would shape nothing and the sweep would run straight.
        let outgoing = read("s0.plate");
        let keys = &outgoing["transform"]["position"]["keyframes"];
        let keys = keys.as_array().expect("keys");
        let last = keys.last().expect("the exit key");
        assert_eq!(last[0], json!("4s"), "{keys:?}");
        let x = last[1][0].as_f64().expect("pixels");
        assert!(x < 960.0 - 0.5 * 1920.0, "swept left of centre: {x}");
        let exit_start = keys[keys.len() - 2].clone();
        assert_eq!(exit_start[0], json!("3.76s"), "{keys:?}");
        assert_eq!(
            exit_start[2],
            json!({ "cubic-bezier": [0.42, 0.0, 1.0, 1.0] })
        );

        // Scene 1's plate sweeps in from the right over its first 0.24s:
        // the enter key sits at the clip's own start, its position right
        // of centre, on the settling curve. The clip itself starts
        // **0.24 s before the cut** - at 3.76 s, while scene 0 is still
        // sweeping out - because a choreographed cut is a composite, not
        // a splice: the whole point of the window is that one frame holds
        // both scenes, the outgoing leaving and the incoming arriving,
        // which is what makes the whip read as one camera move. The
        // two clips overlap in time by exactly the cut's own seconds.
        let incoming = read("s1.plate");
        let keys = &incoming["transform"]["position"]["keyframes"];
        let first = keys
            .as_array()
            .expect("keys")
            .first()
            .expect("the enter key");
        assert_eq!(first[0], json!("0s"), "{keys:?}");
        assert_eq!(
            incoming["start"],
            json!("3.76s"),
            "the clip starts inside the cut's shared window, not at the cut"
        );
        let incoming_end: f64 = parse_seconds(incoming["duration"].as_str().expect("a span"))
            .expect("a duration")
            + parse_seconds(incoming["start"].as_str().expect("a start")).expect("a start");
        let mid_window = 3.9_f64;
        let outgoing_covers = outgoing["start"].as_str().map(parse_seconds) == Some(Some(0.0))
            && parse_seconds(outgoing["duration"].as_str().expect("a span")) > Some(mid_window);
        assert!(
            outgoing_covers,
            "the outgoing scene is still on screen in the shared window"
        );
        assert!(
            parse_seconds(incoming["start"].as_str().expect("a start")) <= Some(mid_window)
                && incoming_end > mid_window,
            "the incoming scene is already on screen in the shared window"
        );
        let x = first[1][0].as_f64().expect("pixels");
        assert!(x > 960.0 + 0.5 * 1920.0, "swept in from the right: {x}");
        assert_eq!(
            first[2],
            json!({ "cubic-bezier": [0.0, 0.0, 0.58, 1.0] }),
            "the settling curve"
        );

        // Scene 1's plate grows to 1.35 at its end - over its own 4%
        // drift, which compounds: 1.04 × 1.35 = 1.404 - and scene 2's
        // plate arrives from the same place and settles: one camera
        // move, both sides.
        let zoom_out = read("s1.plate")["transform"]["scale"]["keyframes"].clone();
        let last = zoom_out
            .as_array()
            .expect("keys")
            .last()
            .expect("the exit key");
        assert_eq!(last[1], json!(1.404), "the plate exits through 1.35");
        let zoom_in = read("s2.plate")["transform"]["scale"]["keyframes"].clone();
        let first = zoom_in
            .as_array()
            .expect("keys")
            .first()
            .expect("the enter key");
        assert_eq!(first[1], json!(1.35), "and arrives from 1.35");

        // The comps carry the cuts too: scene 0's caption - a node with
        // a place of its own - slides out with the whip. A node that is
        // never placed keeps its place: the whip moves what it can move.
        let caption = read("s0.caption");
        let keys = caption["transform"]["position"]["keyframes"]
            .as_array()
            .expect("the comp rides the whip");
        let last = keys.last().expect("the exit key");
        assert_eq!(last[0], json!("4s"));
        let x = last[1][0].as_f64().expect("pixels");
        assert!(x < 500.0, "the caption sweeps out with the plate: {x}");

        // The curtain stands down at both choreographed cuts and dips
        // nowhere: a two-dip reel has one dip per cut, this one has none.
        let curtain = read("curtain")["opacity"]["keyframes"].clone();
        let keys = curtain.as_array().expect("the curtain's keys");
        let dips = keys
            .iter()
            .filter(|key| key[1].as_f64().unwrap_or(0.0) > 0.5)
            .count();
        assert_eq!(dips, 2, "only the reel's own ends: {keys:?}");

        // And the whole document is one the engine would take: every
        // keyframe time strictly increases.
        for layer in layers {
            for clip in layer["clips"].as_array().unwrap_or(&Vec::new()) {
                for property in ["position", "scale", "opacity"] {
                    let Some(keys) = clip["transform"][property]["keyframes"].as_array() else {
                        continue;
                    };
                    let times: Vec<f64> = keys
                        .iter()
                        .map(|key| key[0].as_str().and_then(parse_seconds).expect("a time"))
                        .collect();
                    for pair in times.windows(2) {
                        assert!(pair[0] < pair[1], "{property} in {}: {keys:?}", layer["id"]);
                    }
                }
            }
        }
    }

    /// A staggered exit on the reel's last scene is its closing
    /// choreography - the credits rolling out, not a dip to colour.
    #[test]
    fn the_last_scene_can_leave_by_a_staggered_exit() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.stats.triptych").expect("a preset")).holding(5.0),
        ]);
        sequence.with_cut(
            0,
            crate::transition::Cut::StaggerExit {
                seconds: 0.22,
                step: 0.06,
            },
        );
        let planned = plan(&geneva(), &sequence).expect("a plan");
        // The comp's nodes fade out one after the next.
        let comp_layer = planned.document["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .find(|layer| layer["id"].as_str().unwrap_or_default().starts_with("s0."))
            .expect("the comp's own layer")
            .clone();
        assert!(
            comp_layer["clips"][0]["opacity"]["keyframes"].is_array()
                || planned
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("stagger")),
            "the exit is written or said"
        );
        assert!(
            planned
                .warnings
                .iter()
                .any(|warning| warning.contains("closing choreography")),
            "{:?}",
            planned.warnings
        );
    }

    /// A cut whose window runs the whole scene has no frame to play
    /// over: the exit is skipped whole and the entrance clamped, and
    /// either way the choreography that renders is not the one that was
    /// written. The plan says so rather than staying quietly green.
    #[test]
    fn a_cut_longer_than_its_scene_is_said_not_rendered_short() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.stats.triptych").expect("a preset")).holding(0.3),
            Scene::new(presets::build("funcut.stats.triptych").expect("a preset")).holding(0.3),
        ]);
        sequence.with_cut(0, crate::transition::Cut::ZoomThrough { seconds: 0.35 });
        let planned = plan(&geneva(), &sequence).expect("a plan");
        // Scene 0 leaves by the cut, scene 1 enters by it, and both are
        // shorter than the cut's own window.
        assert!(
            planned.warnings.iter().any(|warning| warning
                .contains("scene 0 leaves by a zoom-through that runs its whole 0.300 s")),
            "{:?}",
            planned.warnings
        );
        assert!(
            planned.warnings.iter().any(|warning| warning
                .contains("scene 1 enters by a zoom-through that runs its whole 0.300 s")),
            "{:?}",
            planned.warnings
        );
    }

    /// And the healthy case stays quiet: a cut that fits its scene says
    /// nothing, so the warning carries meaning rather than noise.
    #[test]
    fn a_cut_that_fits_its_scene_says_nothing() {
        let mut sequence = reel(vec![
            Scene::new(presets::build("funcut.stats.triptych").expect("a preset")).holding(4.0),
            Scene::new(presets::build("funcut.stats.triptych").expect("a preset")).holding(4.0),
        ]);
        sequence.with_cut(0, crate::transition::Cut::ZoomThrough { seconds: 0.35 });
        let planned = plan(&geneva(), &sequence).expect("a plan");
        assert!(
            !planned
                .warnings
                .iter()
                .any(|warning| warning.contains("runs its whole")),
            "{:?}",
            planned.warnings
        );
    }

    /// The storyboard names a cut on the scene that enters by it, and
    /// the plan carries both sides; an `exit` on the last scene is the
    /// reel's own closing.
    #[test]
    fn a_storyboard_choreographs_its_cuts_by_name() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "assets": { "back": "b.png" },
                 "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 4.0,
                     "layers": [ { "asset": "back", "zoom": 0.04 } ] },
                   { "preset": "funcut.chapter.chip", "hold": 4.0,
                     "transition": { "kind": "whip-pan", "direction": "left" },
                     "layers": [ { "asset": "back", "zoom": 0.04 } ] },
                   { "preset": "funcut.sting.wordmark", "hold": 4.0,
                     "exit": { "kind": "stagger-exit", "seconds": 0.2, "step": 0.05 },
                     "layers": [ { "asset": "back", "zoom": 0.04 } ] } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        assert_eq!(
            sequence.transitions.len(),
            2,
            "whip after 0, stagger after 2"
        );
        assert!(matches!(
            sequence.transitions.get(&0),
            Some(crate::transition::Cut::WhipPan { .. })
        ));
        assert!(matches!(
            sequence.transitions.get(&2),
            Some(crate::transition::Cut::StaggerExit { .. })
        ));
        assert!(
            sequence.opening.is_none(),
            "the first scene names no way in"
        );
    }

    /// A layer whose fades meet in the middle of its span - the way out
    /// starting exactly where the way in ends - still carries its curve
    /// on the way out: the shared key starts the fade-out's segment, and
    /// the curve rides it rather than dying with the fade-in's landing.
    #[test]
    fn a_layer_whose_fades_meet_keeps_the_curve_on_the_way_out() {
        let layer = SceneLayer::new("plate")
            .fading(0.6, 0.6)
            .eased(Curve::TYPEWRITER);
        let keys = fade_keys(&layer, 1.2).expect("keyframes");
        let keys = keys["keyframes"].as_array().expect("keys");
        // Three keys: the fade-in from 0, the shared middle, the end.
        assert_eq!(keys.len(), 3, "{keys:?}");
        // The middle key starts the fade-out's segment, so it carries
        // the curve - before, it stayed the fade-in's landing key and
        // the way out travelled a straight line.
        assert_eq!(keys[1][2], json!({ "steps": [8, "jump-end"] }), "{keys:?}");
    }

    /// The pulse, from a storyboard: the measured grid lands in the plan
    /// as keyframes the comp's nodes carry, in the scene's own clock -
    /// the chart that grows on the downbeat.
    #[test]
    fn a_storyboard_pulse_bakes_into_the_plan() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.chart.bars", "hold": 5.0,
                     "pulse": { "beats": [0.98, 1.94, 2.91],
                                "driven": "scale", "amount": 0.05,
                                "release": 0.32 } } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        // The comp's own layers carry the pulse: the sheet node's scale
        // track holds a bump at every beat and a settle after each.
        let sheet = planned.document["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .find(|layer| layer["id"] == json!("s0.sheet"))
            .expect("the chart's sheet");
        let keys = sheet["clips"][0]["transform"]["scale"]["keyframes"]
            .as_array()
            .expect("the pulse landed")
            .clone();
        assert!(keys.len() >= 6, "rest + three bumps and settles: {keys:?}");
        // The first beat, as the timeline carries it: 0.98s into the
        // scene, the scale up by the amount.
        assert_eq!(keys[0][0], json!("0s"));
        assert!(keys.iter().any(|key| key[0] == json!("0.98s")), "{keys:?}");
        assert!(
            keys.iter().any(|key| key[1].as_f64() == Some(1.05)),
            "the bump: {keys:?}"
        );
        // And the plan validates: strictly increasing, in range.
        let times: Vec<f64> = keys
            .iter()
            .map(|key| key[0].as_str().and_then(parse_seconds).expect("a time"))
            .collect();
        for pair in times.windows(2) {
            assert!(pair[0] < pair[1], "{keys:?}");
        }
    }

    /// A pulse rides with its scene wherever the scene is placed: the
    /// grid is read in the scene's own clock, so the same scene pulses
    /// at the same moments of itself whether it opens the reel or sits
    /// fourth. This is the drift question - a keyframe baked against the
    /// scene's project start would move with the placement, and the
    /// chart would grow on the wrong frame.
    #[test]
    fn a_pulse_rides_with_its_scene_wherever_it_is_placed() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.chapter.chip", "hold": 4.0 },
                   { "preset": "funcut.chart.bars", "hold": 5.0,
                     "pulse": { "beats": [0.98, 1.94],
                                "driven": "scale", "amount": 0.05,
                                "release": 0.32 } } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        // The chart scene sits second, starting at 4.0 s in the reel.
        let sheet = planned.document["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .find(|layer| layer["id"] == json!("s1.sheet"))
            .expect("the second scene's chart");
        let clip = &sheet["clips"][0];
        let start = clip["start"].as_str().expect("a start");
        assert_eq!(start, "4s", "the scene sits second, after a 4 s opener");
        // And the pulse's keys are the clip's own: 0.98 s into the
        // scene - which is 4.98 s in the reel - not the project time a
        // grid measured against the whole reel would have written.
        let keys = clip["transform"]["scale"]["keyframes"]
            .as_array()
            .expect("the pulse landed");
        assert!(
            keys.iter().any(|key| key[0] == json!("0.98s")),
            "the beat rides the scene's own clock: {keys:?}"
        );
        assert!(
            !keys.iter().any(|key| key[0] == json!("4.98s")),
            "the beat is clip-local, not project-baked: {keys:?}"
        );
    }

    /// A grid whose every beat falls outside its scene - the signature
    /// of a project-time grid fed to a scene that reads its own clock -
    /// is named in the plan's warnings rather than silently pulsing
    /// nothing.
    #[test]
    fn a_pulse_whose_beats_all_miss_its_scene_is_said() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.chapter.chip", "hold": 4.0 },
                   { "preset": "funcut.chart.bars", "hold": 5.0,
                     "pulse": { "beats": [4.98, 5.94],
                                "driven": "scale", "amount": 0.05,
                                "release": 0.32 } } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        assert!(
            planned.warnings.iter().any(|warning| warning
                .contains("scene 1: the pulse's beats all fall outside its 4.600 s")),
            "{:?}",
            planned.warnings
        );
        // And the healthy neighbour stays quiet: the same grid shifted
        // into the scene's own clock pulses and says nothing.
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.chapter.chip", "hold": 4.0 },
                   { "preset": "funcut.chart.bars", "hold": 5.0,
                     "pulse": { "beats": [0.98, 1.94],
                                "driven": "scale", "amount": 0.05,
                                "release": 0.32 } } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        assert!(
            !planned
                .warnings
                .iter()
                .any(|warning| warning.contains("beats all fall outside")),
            "{:?}",
            planned.warnings
        );
    }

    /// The act a scene plays in chooses its pulse's hand, wired in from
    /// the storyboard: the emotion read's curve for the act - calm for
    /// the hook, the overshoot for the reveal - replaces the pulse's
    /// default spring, and a pulse that names its own curve keeps it.
    #[test]
    fn an_act_chooses_the_pulse_curve_when_nobody_else_did() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.chart.bars", "hold": 5.0,
                     "act": "hook",
                     "pulse": { "beats": [0.98], "driven": "scale",
                                "amount": 0.05, "release": 0.32 } },
                   { "preset": "funcut.chart.bars", "hold": 5.0,
                     "act": "reveal",
                     "pulse": { "beats": [0.98], "driven": "scale",
                                "amount": 0.05, "release": 0.32,
                                "curve": "bounce" } },
                   { "preset": "funcut.chart.bars", "hold": 5.0,
                     "pulse": { "beats": [0.98], "driven": "scale",
                                "amount": 0.05, "release": 0.32 } } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let bump_of = |scene: usize| {
            planned.document["layers"]
                .as_array()
                .expect("layers")
                .iter()
                .find(|layer| layer["id"] == json!(format!("s{scene}.sheet")))
                .expect("the chart's sheet")["clips"][0]["transform"]["scale"]
                .as_object()
                .expect("the pulse landed")["keyframes"]
                .as_array()
                .expect("the keys")
                .clone()
        };
        // The hook's bump: the read's own ease-out, not the default
        // spring the pulse was born with.
        let hook = bump_of(0);
        let hook_bump = hook
            .iter()
            .find(|key| key[1].as_f64() == Some(1.05))
            .expect("the hook's bump");
        assert_eq!(
            hook_bump[2],
            json!({ "cubic-bezier": [0.0, 0.0, 0.58, 1.0] }),
            "the hook pulses calm: {hook_bump:?}"
        );
        // The reveal names bounce and keeps it - the author's hand beats
        // the read's, which would have given it the spring-pop.
        let reveal = bump_of(1);
        let reveal_bump = reveal
            .iter()
            .find(|key| key[1].as_f64() == Some(1.05))
            .expect("the reveal's bump");
        assert!(
            reveal_bump[2].get("linear").is_some(),
            "a named curve is kept: {reveal_bump:?}"
        );
        // And a scene that names no act keeps the default spring: the
        // wiring adds a hand, it does not take one away.
        let plain = bump_of(2);
        let plain_bump = plain
            .iter()
            .find(|key| key[1].as_f64() == Some(1.05))
            .expect("the plain bump");
        assert!(
            plain_bump[2].get("spring").is_some(),
            "the default spring stays: {plain_bump:?}"
        );
    }

    /// The grade, from a storyboard: the scene wears its act's
    /// temperature, baked into the plan as the comp's first paint - the
    /// cold hook over the plate, the comp's own words untouched above
    /// it, the brand's colour worn when it is named.
    #[test]
    fn a_storyboard_grade_bakes_into_the_plan_as_the_first_paint() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 5.0,
                     "plate": "back",
                     "grade": { "temperature": "cold" } } ],
               "assets": { "back": "assets/plate_dark.png" } }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        // The grade node is the scene's first comp layer: under the
        // wordmark, over the plate.
        let layers = planned.document["layers"].as_array().expect("layers");
        let grade = layers
            .iter()
            .find(|layer| layer["id"] == json!("s0.funcut-grade"))
            .expect("the grade layer");
        let source = &grade["clips"][0]["source"];
        assert_eq!(source["kind"], json!("html"));
        assert_eq!(
            source["html"].as_str(),
            Some(r#"<div class="funcut-grade"></div>"#)
        );
        let css = source["css"].as_str().expect("the stylesheet");
        assert!(
            css.contains("backdrop-filter:saturate(0.88) contrast(1.1) brightness(0.94);"),
            "{css}"
        );
        assert!(css.contains("background:rgba(24,68,102,0.28);"), "{css}");
        assert!(css.contains("mix-blend-mode:soft-light;"), "{css}");
        // The paint order is the layer order: the plate, then the grade,
        // then the comp's own wordmark.
        let at = |id: &str| {
            layers
                .iter()
                .position(|layer| layer["id"] == json!(id))
                .unwrap_or_else(|| panic!("no {id}: {layers:?}"))
        };
        let plate_at = layers
            .iter()
            .position(|layer| {
                layer["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("s0.plate"))
            })
            .expect("the plate");
        assert!(
            plate_at < at("s0.funcut-grade"),
            "the grade is over the plate"
        );
        assert!(
            at("s0.funcut-grade") < at("s0.mark"),
            "the grade is under the comp's own words"
        );
        // Nothing was warned about: a graded plate is a scene.
        assert!(
            planned
                .warnings
                .iter()
                .all(|warning| !warning.contains("grade")),
            "{:?}",
            planned.warnings
        );
    }

    /// The brand's own colour, worn instead of the temperature's cast:
    /// the palette is respected by wearing it, not by matching it.
    #[test]
    fn a_brand_tint_is_worn_instead_of_the_stock_cast() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 5.0,
                     "plate": "back",
                     "grade": { "temperature": "warm", "tint": "#3165a4" } } ],
               "assets": { "back": "assets/plate_dark.png" } }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let grade = planned.document["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .find(|layer| layer["id"] == json!("s0.funcut-grade"))
            .expect("the grade layer");
        let css = grade["clips"][0]["source"]["css"].as_str().expect("css");
        // The warm tonal moves, the brand's own colour as the cast.
        assert!(
            css.contains("backdrop-filter:saturate(1.12) contrast(1.04) brightness(1.02);"),
            "{css}"
        );
        assert!(css.contains("background:rgba(49,101,164,0.22);"), "{css}");
        assert!(!css.contains("255,178,92"), "the stock cast is gone: {css}");
    }

    /// A grade with nothing under the comp lands on the curtain alone,
    /// and the plan says so rather than rendering a grade nobody can
    /// see.
    #[test]
    fn a_grade_with_nothing_under_the_comp_is_said_not_shown() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 5.0,
                     "grade": { "temperature": "cold" } } ] }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        assert!(
            planned
                .warnings
                .iter()
                .any(|warning| warning.contains("nothing under the comp")),
            "{:?}",
            planned.warnings
        );
    }

    /// A grade that is wrong is refused with the scene it belongs to,
    /// before any engine is asked.
    #[test]
    fn a_storyboard_grade_that_is_wrong_is_refused_with_its_scene_named() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 5.0,
                     "grade": { "temperature": "warm", "amount": 1.5 } } ] }"##,
        )
        .expect("a storyboard");
        let problems = storyboard
            .validate()
            .expect_err("the amount is out of range");
        assert!(
            problems
                .iter()
                .any(|problem| problem.scene == 0 && problem.problem.contains("`amount` is 1.5")),
            "{problems:?}"
        );
    }

    /// The style, from a storyboard: the same layout wearing a
    /// personality - the type and the motion restyled, the personality's
    /// grade worn when the scene names none of its own.
    #[test]
    fn a_storyboard_style_restyles_the_layout_and_wears_its_colour() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 5.0,
                     "plate": "back", "style": "punchy" } ],
               "assets": { "back": "assets/plate_dark.png" } }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        // The wordmark's own stylesheet took the personality: the
        // heaviest weight, the tight tracking, the overshoot.
        let mark = planned.document["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .find(|layer| layer["id"] == json!("s0.mark"))
            .expect("the wordmark");
        let css = mark["clips"][0]["source"]["css"].as_str().expect("css");
        assert!(css.contains("font-weight: 900"), "{css}");
        assert!(css.contains("letter-spacing: -0.04em"), "{css}");
        assert!(css.contains("cubic-bezier(0.3, 1.6, 0.5, 1)"), "{css}");
        // The layout keeps its geometry and its words.
        assert!(css.contains("font-size: 190px"), "{css}");
        let markup = mark["clips"][0]["source"]["html"].as_str().expect("markup");
        assert!(markup.contains("FUNCUT"), "{markup}");
        // And the personality's colour is worn: punchy's hot grade, under
        // the comp's own words.
        let grade = planned.document["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .find(|layer| layer["id"] == json!("s0.funcut-grade"))
            .expect("the style's grade landed");
        let grade_css = grade["clips"][0]["source"]["css"].as_str().expect("css");
        assert!(
            grade_css.contains("background:rgba(255,120,60,0.26);"),
            "{grade_css}"
        );
    }

    /// The scene's own grade beats the style's colour: the personality
    /// dresses the type and the motion, and the brand's grade is still
    /// the grade.
    #[test]
    fn a_scene_grade_wins_over_the_styles_colour() {
        let storyboard = crate::storyboard::Storyboard::parse(
            r##"{ "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 5.0,
                     "plate": "back", "style": "punchy",
                     "grade": { "temperature": "cold" } } ],
               "assets": { "back": "assets/plate_dark.png" } }"##,
        )
        .expect("a storyboard");
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let planned = plan(&geneva(), &sequence).expect("a plan");
        let layers = planned.document["layers"].as_array().expect("layers");
        // Exactly one grade box, and it is the cold one.
        let grades: Vec<&Json> = layers
            .iter()
            .filter(|layer| {
                layer["id"]
                    .as_str()
                    .is_some_and(|id| id.contains("funcut-grade"))
            })
            .collect();
        assert_eq!(grades.len(), 1, "{layers:?}");
        let css = grades[0]["clips"][0]["source"]["css"]
            .as_str()
            .expect("css");
        assert!(css.contains("background:rgba(24,68,102,0.28);"), "{css}");
        // And the type still wears the personality.
        let mark = layers
            .iter()
            .find(|layer| layer["id"] == json!("s0.mark"))
            .expect("the wordmark");
        let mark_css = mark["clips"][0]["source"]["css"].as_str().expect("css");
        assert!(mark_css.contains("font-weight: 900"), "{mark_css}");
    }

    #[test]
    fn a_layer_with_only_one_fade_writes_strictly_increasing_keys() {
        // A fade out with no fade in (and the other way round) used to
        // write two opacity keys at the same time - "0s" twice, or the
        // span's end twice - which the engine refuses for the whole
        // render (E303). Every shape here must strictly increase.
        for (fade_in, fade_out) in [(0.0, 0.8), (0.5, 0.0), (0.5, 0.8)] {
            let mut sequence = Sequence::new(1920, 1080, 30.0);
            sequence.asset(
                "plate",
                Asset {
                    src: "assets/plate_dark.jpg".into(),
                    kind: None,
                },
            );
            let scene = Scene::new(presets::build("funcut.sting.wordmark").expect("a preset"))
                .holding(4.0)
                .layer(SceneLayer::new("plate").fading(fade_in, fade_out));
            sequence.push(scene);
            let plan = plan(&geneva(), &sequence).expect("plans");
            let plate = layers(&plan)
                .into_iter()
                .find(|layer| layer["id"] == json!("s0.plate"))
                .expect("the plate");
            let keys = plate["clips"][0]["opacity"]["keyframes"]
                .as_array()
                .cloned()
                .expect("keyframes");
            let times: Vec<f64> = keys
                .iter()
                .map(|key| key[0].as_str().and_then(parse_seconds).expect("a time"))
                .collect();
            for pair in times.windows(2) {
                assert!(pair[0] < pair[1], "{keys:?} for ({fade_in}, {fade_out})");
            }
        }
    }

    #[test]
    fn a_scene_that_places_itself_through_another_is_refused() {
        // Two scenes, each placing the other: a cycle, not a composition.
        // The storyboard door refuses it in validate; the plan door says
        // so too, rather than walking into itself forever.
        let card = |id: &str, places: &str| {
            serde_json::from_value(serde_json::json!({
                "id": id,
                "preset": "funcut.chapter.chip",
                "layers": [ { "scene": places } ]
            }))
            .expect("a card")
        };
        let mut sequence = Sequence::new(1920, 1080, 30.0);
        sequence.scene_cards = vec![card("a", "b"), card("b", "a")];
        let scene = sequence.scene_cards[0]
            .scene(0, Path::new("."))
            .expect("a scene");
        sequence.push(scene);
        let error = plan(&geneva(), &sequence).expect_err("a cycle is refused");
        assert!(error.to_string().contains("places itself"), "{error}");
    }

    /// The farm: `--parts N` rides on the engine call as the parallel
    /// render path, the timeline is the same document the plain render
    /// takes, and a parts count under one is clamped to one worker.
    #[test]
    fn the_farm_renders_with_the_parts_flag_on_the_engine_call() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("a scratch dir");
        let stub = dir.path().join("geneva-stub");
        std::fs::write(
            &stub,
            concat!(
                "#!/bin/sh\n",
                "echo \"$*\" >> \"$(dirname \"$0\")/calls.log\"\n",
                "echo '{\"ok\":true,\"diagnostics\":[],\"summary\":{}}'\n",
            ),
        )
        .expect("the stub is written");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
        let geneva = Geneva::at(&stub);
        let log = dir.path().join("calls.log");

        let sequence = reel(vec![
            Scene::new(presets::build("funcut.sting.wordmark").expect("a preset")).holding(4.0),
        ]);
        let planned = plan(&geneva, &sequence).expect("a plan");
        let render_dir = dir.path().join("render");
        let out = dir.path().join("reel.mp4");
        planned
            .render_parts(&geneva, &render_dir, &out, 4)
            .expect("farms");

        let calls = std::fs::read_to_string(&log).expect("the call was logged");
        assert!(calls.contains("farm"), "the farm verb: {calls}");
        assert!(calls.contains("timeline.json"), "{calls}");
        assert!(calls.contains("--parts 4"), "{calls}");
        assert!(
            calls.contains(&out.display().to_string()),
            "the output is named in full: {calls}"
        );

        // A parts count under one is one worker, not a refusal.
        let _ = std::fs::remove_file(&log);
        planned
            .render_parts(&geneva, &render_dir, &out, 0)
            .expect("clamped");
        let calls = std::fs::read_to_string(&log).expect("the call was logged");
        assert!(calls.contains("--parts 1"), "{calls}");

        // And the plain render still takes the plain verb: two doors,
        // one document.
        let _ = std::fs::remove_file(&log);
        planned.render(&geneva, &render_dir, &out).expect("renders");
        let calls = std::fs::read_to_string(&log).expect("the call was logged");
        assert!(calls.contains("render"), "{calls}");
        assert!(!calls.contains("farm"), "{calls}");
    }
}
