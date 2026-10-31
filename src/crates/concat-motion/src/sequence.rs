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

use serde_json::{Map, Value as Json, json};

use crate::authoring::{self, Op};
use crate::comp::{Asset, Blend, Comp, Fit, Rgba};
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

    /// How long the sequence runs: the end of the last scene.
    pub fn duration(&self) -> f64 {
        self.layout().last().map(|(_, _, end)| *end).unwrap_or(0.0)
    }

    /// Each scene's start and length, with `at` and `hold` resolved.
    ///
    /// A scene that names no start follows the one before it, so a reel is
    /// written in order without arithmetic; a scene that names no hold is as
    /// long as its comp.
    fn layout(&self) -> Vec<(usize, f64, f64)> {
        let mut out = Vec::with_capacity(self.scenes.len());
        let mut clock = 0.0;
        for (index, scene) in self.scenes.iter().enumerate() {
            let start = scene.at.unwrap_or(clock);
            let length = scene.hold.unwrap_or(scene.comp.duration).max(0.1);
            out.push((index, start, start + length));
            clock = start + length;
        }
        out
    }
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
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
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
fn layer_clip(
    layer: &SceneLayer,
    width: f64,
    height: f64,
    start: f64,
    span: f64,
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

    // Where the layer sits, in pixels from the frame's corner: the centre,
    // plus its offset as a fraction of the frame, plus its drift.
    let placed = |x: f64, y: f64| {
        json!([
            round(width / 2.0 + x * width),
            round(height / 2.0 + y * height)
        ])
    };

    // The least scale that keeps a cover picture covering while it drifts.
    let reach = |from: f64, to: f64| from.abs().max(to.abs());
    let mut base = layer.scale.max(0.0);
    if matches!(layer.fit, Fit::Cover) && base >= 1.0 {
        let travel = 2.0 * reach(offset.0, offset.0 + pan.0).max(reach(offset.1, offset.1 + pan.1));
        base = base.max(1.0 + travel);
    }

    let mut transform = Map::new();
    if pan != (0.0, 0.0) {
        transform.insert(
            "position".into(),
            json!({ "keyframes": [
                [seconds(0.0), placed(offset.0, offset.1)],
                [seconds(span), placed(offset.0 + pan.0, offset.1 + pan.1)],
            ] }),
        );
    } else if offset != (0.0, 0.0) {
        transform.insert("position".into(), placed(offset.0, offset.1));
    }
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

    // Opacity: a steady number, or a fade up, a hold and a fade down. Each
    // fade is held to half the span so the two never cross, and the hold
    // key is dropped when they would land on the same time - a track whose
    // times do not strictly increase is refused by the engine.
    if layer.fade_in > 1e-9 || layer.fade_out > 1e-9 {
        let fade_in = layer.fade_in.min(span / 2.0).max(0.0);
        let fade_out = layer.fade_out.min(span / 2.0).max(0.0);
        let mut keys = vec![
            json!([seconds(0.0), 0.0]),
            json!([seconds(fade_in), round(layer.opacity)]),
        ];
        if span - fade_out > fade_in {
            keys.push(json!([seconds(span - fade_out), round(layer.opacity)]));
        }
        keys.push(json!([seconds(span), 0.0]));
        clip.insert("opacity".into(), json!({ "keyframes": keys }));
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

/// A number the timeline can carry cleanly: three decimals, the same
/// precision the comp path writes its keyframe values at.
fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Lay a sequence out as one Geneva timeline.
///
/// Every scene's comp is validated and its operations applied first: a shot
/// that does not make sense stops the plan and names the scene, rather than
/// reaching a render. The layers keep the comp's own ids, prefixed with the
/// scene's index, so two scenes using the same template never collide.
pub fn plan(geneva: &Geneva, sequence: &Sequence) -> Result<Plan, MotionError> {
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
        let problems = comp.validate();
        if !problems.is_empty() {
            return Err(MotionError::Invalid {
                diagnostics: problems,
            });
        }
        let start = scene
            .at
            .unwrap_or_else(|| scene_ends.last().copied().unwrap_or(0.0));
        let hold = scene.hold.unwrap_or(comp.duration).max(0.1);
        scene_ends.push(start + hold);

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
                let mut transform = Map::new();
                if layer.offset != (0.0, 0.0) {
                    transform.insert(
                        "position".into(),
                        json!([
                            round(
                                f64::from(sequence.width) / 2.0
                                    + layer.offset.0 * f64::from(sequence.width)
                            ),
                            round(
                                f64::from(sequence.height) / 2.0
                                    + layer.offset.1 * f64::from(sequence.height)
                            )
                        ]),
                    );
                }
                if (layer.scale - 1.0).abs() > 1e-9 && layer.scale > 0.0 {
                    transform.insert("scale".into(), json!(round(layer.scale)));
                }
                if !transform.is_empty() {
                    clip.insert("transform".into(), Json::Object(transform));
                }
                if layer.opacity < 0.999 {
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
            let clip = layer_clip(
                layer,
                f64::from(sequence.width),
                f64::from(sequence.height),
                start + span_start,
                span,
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

    // The curtain: over everything, opaque at the ends, dipping at each cut
    // and clear in between.
    if let Some(curtain) = sequence.curtain
        && sequence.scenes.len() > 1
    {
        // Both fades are held to a quarter of the reel, as the dips are held
        // to an eighth: a fade that outlasts a short reel would otherwise
        // land its far key at or before zero, where the dedup below takes it
        // for the reel's opening key - and a reel that opens clear has lost
        // the fade up it was promised.
        let open = curtain.open.min(duration / 4.0);
        let close = curtain.close.min(duration / 4.0);
        let mut marks: Vec<(f64, f64)> = vec![(0.0, 1.0), (open, 0.0)];
        for end in &scene_ends[..scene_ends.len() - 1] {
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
    collect_compositions(geneva, sequence, &sequence.scene_cards, &mut compositions)?;

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
fn collect_compositions(
    geneva: &Geneva,
    sequence: &Sequence,
    cards: &[crate::storyboard::SceneCard],
    compositions: &mut Map<String, Json>,
) -> Result<(), MotionError> {
    // Only the scenes this sequence's layers actually place: a named
    // scene nobody places is not emitted (an unused composition is a
    // warning), and - the reason this matters more - a sub-plan of one
    // named scene does not re-emit that scene, which is where a
    // careless "every named scene" walked into itself forever.
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
        if compositions.contains_key(&key) {
            continue;
        }
        // A sub-sequence of this one scene: same frame, same assets, no
        // curtain, its own layers at relative times.
        let mut sub = Sequence::new(sequence.width, sequence.height, sequence.fps);
        sub.source_dir = sequence.source_dir.clone();
        sub.assets = sequence.assets.clone();
        sub.scene_cards = sequence.scene_cards.clone();
        sub.push(card.scene(index, &sequence.source_dir).map_err(|message| {
            MotionError::Engine {
                code: None,
                message,
                path: None,
                help: None,
            }
        })?);
        let planned = plan(geneva, &sub)?;
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
        compositions.insert(
            key,
            json!({
                "width": sequence.width,
                "height": sequence.height,
                "background": "transparent",
                "layers": planned.document["layers"].clone(),
            }),
        );
    }
    Ok(())
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

/// Seconds as the timeline writes them: `2s`, `0.5s`, `7.2s`.
fn seconds(value: f64) -> String {
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
    fn a_single_scene_needs_no_curtain() {
        let plan = plan(
            &geneva(),
            &reel(vec![Scene::new(
                presets::build("funcut.chapter.chip").expect("a preset"),
            )]),
        )
        .expect("plans");
        assert!(
            !layers(&plan)
                .iter()
                .any(|layer| layer["id"] == json!("curtain"))
        );
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
        // Bottom to top: back, mid, the comp, the light leak, the grain.
        // One scene, so no curtain: it dips between scenes only.
        assert!(back_at < mid_at, "{order:?}");
        assert!(mid_at < comp_at, "{order:?}");
        assert!(comp_at < leak_at, "{order:?}");
        assert!(leak_at < grain_at, "{order:?}");
        assert_eq!(order.last().unwrap(), "s0.overlay2", "{order:?}");
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
}
