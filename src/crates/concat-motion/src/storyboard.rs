// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The storyboard: a whole reel as one JSON document.
//!
//! This is the contract every door agrees on. The CLI (`concat-motion-agent
//! reel`) eats it from a file; the MCP tool (`motion_reel`) eats it from a
//! tool call; both build the same [`Sequence`] through the same code, so a
//! storyboard that renders one way renders the other way too. Nothing here
//! renders anything - it resolves, and refuses what does not make sense
//! before a single frame is asked for, naming the scene it stopped at.
//!
//! ```json
//! { "width": 1920, "height": 1080, "fps": 30,
//!   "assets": { "back": "assets/back.jpg", "mid": "assets/mid.png",
//!               "leak": "assets/leak.png", "score": "assets/score.wav" },
//!   "scenes": [
//!     { "preset": "funcut.sting.wordmark", "hold": 4.0,
//!       "layers": [
//!         { "asset": "back", "zoom": 0.04 },
//!         { "asset": "mid", "pan": [-0.03, 0.0], "fade_in": 0.5, "fade_out": 0.8 },
//!         { "asset": "leak", "blend": "screen", "scale": 0.8, "offset": [-0.25, -0.2],
//!           "fade_in": 0.4, "fade_out": 0.4, "above": true }
//!       ],
//!       "ops": [ { "op": "set-slot", "slot": "Word", "value": "FUNCUT" } ] }
//!   ],
//!   "audio": [ { "asset": "score", "gain_db": -2.0 } ],
//!   "curtain": { "color": "#05060aff", "open": 0.6, "close": 0.8, "dip": 0.2, "depth": 1.0 } }
//! ```
//!
//! A scene's `layers` stack bottom first: plates under the comp, and - with
//! `above: true` - grain, vignettes, light leaks and foreground silhouettes
//! over it. The short form is still the whole story for a single plate:
//! `"plate": "back", "zoom": 0.04` is one layer under the comp and nothing
//! else, exactly what it always was. Every error names the scene it stopped
//! at, so a twelve-scene storyboard is debuggable from one refusal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use crate::authoring::Op;
use crate::comp::{Asset, Blend, Comp, Fit, Rgba};
use crate::presets;
use crate::sequence::{AudioBed, Curtain, Scene, SceneLayer, Sequence};

/// One problem a storyboard carries, found by [`Storyboard::validate`]
/// before anything is resolved or rendered: which scene it stopped at,
/// which layer when the problem is a layer's, and what is wrong - in the
/// storyboard's own words, so the fix is the message.
///
/// A refusal that names the scene is the contract every error in this
/// crate keeps; this one adds the layer, because a twelve-scene
/// storyboard with six layers a scene is debuggable from a refusal that
/// says exactly which of the seventy-two is wrong.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SceneError {
    /// The scene's index, from 0.
    pub scene: usize,
    /// The layer's asset id, when the problem is a layer's.
    pub layer: Option<String>,
    /// What is wrong.
    pub problem: String,
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.layer {
            Some(layer) => {
                write!(
                    f,
                    "scene {}, the layer {layer:?}: {}",
                    self.scene, self.problem
                )
            }
            None => write!(f, "scene {}: {}", self.scene, self.problem),
        }
    }
}

/// A storyboard: several scenes, in order, as one render.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct Storyboard {
    /// Frame width.
    #[serde(default = "default_width")]
    pub width: u32,
    /// Frame height.
    #[serde(default = "default_height")]
    pub height: u32,
    /// Frame rate.
    #[serde(default = "default_fps")]
    pub fps: f64,
    /// Where asset paths resolve from; the storyboard's own folder by
    /// default, which is the caller's `root`.
    #[serde(default)]
    pub source_dir: Option<PathBuf>,
    /// The assets the scenes and the sound name, by id: each value is a
    /// path, resolved from the root.
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    /// The scenes, in order.
    pub scenes: Vec<SceneCard>,
    /// The beds of sound under the whole thing.
    #[serde(default)]
    pub audio: Vec<AudioCard>,
    /// The dip between scenes; `None` cuts hard.
    #[serde(default)]
    pub curtain: Option<CurtainCard>,
}

/// One scene: a preset or a comp file, operations on it, layers stacked
/// into it, and where and how long it runs.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct SceneCard {
    /// The scene's own name, when another scene's layer places it as a
    /// nested composition: `"id": "intro"` and then, elsewhere,
    /// `"layers": [{"scene": "intro", ...}]`. Two scenes of one
    /// storyboard cannot share a name.
    #[serde(default)]
    pub id: Option<String>,
    /// A preset id from the library.
    #[serde(default)]
    pub preset: Option<String>,
    /// Or a comp document, relative to the storyboard's root.
    #[serde(default)]
    pub comp: Option<String>,
    /// Where it starts; omitted means after the scene before it.
    #[serde(default)]
    pub at: Option<f64>,
    /// How long it holds; omitted means the comp's own length.
    #[serde(default)]
    pub hold: Option<f64>,
    /// A still underneath it - the short form of one layer.
    #[serde(default)]
    pub plate: Option<String>,
    /// How much the plate grows over the scene.
    #[serde(default)]
    pub zoom: Option<f64>,
    /// The layers stacked into the scene, bottom first, after the short-form
    /// plate when one is named.
    #[serde(default)]
    pub layers: Vec<LayerCard>,
    /// Operations applied to the comp before it is placed.
    #[serde(default)]
    pub ops: Vec<Json>,
}

/// One layer of a scene, as a storyboard writes it. The source is either
/// an `asset` id or a named `scene`; every other field is optional, and
/// the defaults are [`SceneLayer`]'s own.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct LayerCard {
    /// An asset id from the storyboard's assets. Exactly one of `asset`
    /// and `scene` is written.
    #[serde(default)]
    pub asset: Option<String>,
    /// A named scene of this storyboard, placed as a nested composition:
    /// the scene renders once as a reusable composition and this layer
    /// shows it, with this layer's own timing, drift, blend and fades. A
    /// scene cannot be built from itself, directly or through another -
    /// the plan refuses the chain by name.
    #[serde(default)]
    pub scene: Option<String>,
    /// How much it grows over its span, as a scale delta.
    #[serde(default)]
    pub zoom: Option<f64>,
    /// How far it drifts over its span, as `[x, y]` fractions of the frame.
    #[serde(default)]
    pub pan: Option<(f64, f64)>,
    /// A static offset from the centre, as `[x, y]` fractions of the frame.
    #[serde(default)]
    pub offset: Option<(f64, f64)>,
    /// The base scale multiplier.
    #[serde(default)]
    pub scale: Option<f64>,
    /// How the picture is fitted: cover (default), contain, fill or none.
    #[serde(default)]
    pub fit: Option<String>,
    /// How it mixes with what is under it: normal (default), multiply,
    /// screen, overlay, darken, lighten, difference, soft-light or add.
    #[serde(default)]
    pub blend: Option<String>,
    /// Its steady opacity, 0 to 1.
    #[serde(default)]
    pub opacity: Option<f64>,
    /// Fade up from transparent, in seconds. (`fadeIn` also parses, for a
    /// hand that types camelCase.)
    #[serde(default, alias = "fadeIn")]
    pub fade_in: Option<f64>,
    /// Fade down to transparent, in seconds. (`fadeOut` also parses.)
    #[serde(default, alias = "fadeOut")]
    pub fade_out: Option<f64>,
    /// When it starts, from the scene's own start.
    #[serde(default)]
    pub start: Option<f64>,
    /// How long it holds; omitted runs to the scene's end.
    #[serde(default)]
    pub hold: Option<f64>,
    /// How fast it drifts, as a multiple of its own `zoom` and `pan` (or
    /// of the classic plate's drift, when it carries none): `2.0` twice
    /// as fast, `0.5` half - nearer layers move faster, which is
    /// parallax.
    #[serde(default)]
    pub parallax: Option<f64>,
    /// Whether it paints over the comp rather than under it.
    #[serde(default)]
    pub above: Option<bool>,
}

/// A bed of sound under the whole sequence.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct AudioCard {
    /// An asset id from the storyboard's assets.
    pub asset: String,
    /// Gain in decibels.
    #[serde(default)]
    pub gain_db: f64,
    /// Fade up from silence.
    #[serde(default)]
    pub fade_in: Option<f64>,
    /// Fade to silence.
    #[serde(default)]
    pub fade_out: Option<f64>,
}

/// The dip between scenes, as a storyboard writes it.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct CurtainCard {
    /// The colour dipped through, as hex.
    #[serde(default = "default_curtain")]
    pub color: String,
    /// Fade up from it at the top.
    #[serde(default = "default_open")]
    pub open: f64,
    /// Fade down into it at the end.
    #[serde(default = "default_close")]
    pub close: f64,
    /// Half-width of each dip, in seconds.
    #[serde(default = "default_dip")]
    pub dip: f64,
    /// How opaque it gets at the cut: 1 is a full cut to colour.
    #[serde(default = "default_depth")]
    pub depth: f64,
}

fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_fps() -> f64 {
    30.0
}
fn default_curtain() -> String {
    "#05060aff".into()
}
fn default_open() -> f64 {
    0.6
}
fn default_close() -> f64 {
    0.8
}
fn default_dip() -> f64 {
    0.2
}
fn default_depth() -> f64 {
    1.0
}

/// Whether a number is finite and not negative - the shape every timing
/// field in a storyboard must have. A NaN or an infinity from a program
/// (JSON itself cannot carry one) and a negative time are both refused by
/// the same check, so the message names the field and not the parser.
fn finite_non_negative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

/// The blend a name stands for, or `None` when it is not one of the nine
/// the engine blends with.
fn blend_of(name: &str) -> Option<Blend> {
    match name {
        "normal" => Some(Blend::Normal),
        "multiply" => Some(Blend::Multiply),
        "screen" => Some(Blend::Screen),
        "overlay" => Some(Blend::Overlay),
        "darken" => Some(Blend::Darken),
        "lighten" => Some(Blend::Lighten),
        "difference" => Some(Blend::Difference),
        "soft-light" => Some(Blend::SoftLight),
        "add" => Some(Blend::Add),
        _ => None,
    }
}

/// What a layer is made of: a picture from the assets, or another named
/// scene of this storyboard, placed as a nested composition.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LayerSource {
    /// An asset id the storyboard carries.
    Asset(String),
    /// A named scene of this storyboard.
    Scene(String),
}

impl LayerCard {
    /// The layer's source, refusing a card that names neither an asset
    /// nor a scene, or both at once.
    pub fn source(&self) -> Result<LayerSource, String> {
        match (&self.asset, &self.scene) {
            (Some(asset), None) => Ok(LayerSource::Asset(asset.clone())),
            (None, Some(scene)) => Ok(LayerSource::Scene(scene.clone())),
            (Some(_), Some(_)) => {
                Err("a layer names both an `asset` and a `scene`; one or the other".to_owned())
            }
            (None, None) => {
                Err("a layer needs an `asset` (an id from `assets`) or a `scene` (a named scene of this storyboard)".to_owned())
            }
        }
    }
}

impl Storyboard {
    /// Parse a storyboard from its text, refusing what does not make sense
    /// as a document.
    pub fn parse(text: &str) -> Result<Storyboard, String> {
        serde_json::from_str(text).map_err(|error| format!("the storyboard: {error}"))
    }

    /// Parse a storyboard from a JSON value a tool call hands over.
    pub fn from_value(value: &Json) -> Result<Storyboard, String> {
        serde_json::from_value(value.clone()).map_err(|error| format!("the storyboard: {error}"))
    }

    /// Every problem the document carries, checked against the contract
    /// before anything is resolved or rendered: a missing asset, a blend
    /// or a fit the engine does not know, a negative time, a number that
    /// is not finite, an opacity outside its range, a comp path that
    /// climbs out of the storyboard's folder.
    ///
    /// This is the step before the bake: every problem is found and
    /// returned together - one refusal that lists all of them, each with
    /// its scene (and its layer, when it is a layer's), instead of a
    /// render that stops at the first and has to be run again for the
    /// next. [`Storyboard::sequence`] calls this first, so both doors -
    /// the CLI's `reel` verb and the MCP's `motion_reel` tool - refuse an
    /// invalid storyboard with the same list.
    pub fn validate(&self) -> Result<(), Vec<SceneError>> {
        let mut problems: Vec<SceneError> = Vec::new();
        let mut problem = |scene: usize, layer: Option<String>, what: String| {
            problems.push(SceneError {
                scene,
                layer,
                problem: what,
            });
        };

        if self.width == 0 || self.height == 0 {
            problem(
                0,
                None,
                format!(
                    "the frame is {}x{}, and neither side may be zero",
                    self.width, self.height
                ),
            );
        }
        if !self.fps.is_finite() || self.fps <= 0.0 {
            problem(
                0,
                None,
                format!(
                    "the frame rate is {}, and it must be a positive number",
                    self.fps
                ),
            );
        }

        for (index, card) in self.scenes.iter().enumerate() {
            // What the scene is drawn from.
            match (&card.preset, &card.comp) {
                (None, None) => {
                    problem(index, None, "the scene needs a `preset` or a `comp`".into())
                }
                (Some(_), Some(_)) => problem(
                    index,
                    None,
                    "the scene names both a `preset` and a `comp`; one or the other".into(),
                ),
                _ => {}
            }
            if let Some(comp) = &card.comp
                && (comp.starts_with('/') || comp.contains(".."))
            {
                problem(
                    index,
                    None,
                    format!(
                        "the comp {comp:?} is a name under the storyboard's folder, not a path out of it"
                    ),
                );
            }
            // The scene's own timing.
            if let Some(at) = card.at
                && !finite_non_negative(at)
            {
                problem(
                    index,
                    None,
                    format!("`at` is {at}, and it cannot be negative"),
                );
            }
            if let Some(hold) = card.hold
                && !finite_non_negative(hold)
            {
                problem(
                    index,
                    None,
                    format!("`hold` is {hold}, and it cannot be negative"),
                );
            }
            // The short form's plate.
            if let Some(plate) = &card.plate {
                if !self.assets.contains_key(plate) {
                    problem(
                        index,
                        Some(plate.clone()),
                        format!(
                            "the plate names the asset {plate:?}, which the storyboard does not carry"
                        ),
                    );
                }
                if let Some(zoom) = card.zoom
                    && !zoom.is_finite()
                {
                    problem(
                        index,
                        Some(plate.clone()),
                        format!("the plate's `zoom` is {zoom}, which is not a finite number"),
                    );
                }
            }
            // Every layer, in full.
            for layer in &card.layers {
                let name = layer
                    .asset
                    .clone()
                    .or_else(|| layer.scene.clone())
                    .unwrap_or_else(|| "?".to_owned());
                match layer.source() {
                    Err(why) => problem(index, Some(name.clone()), why),
                    Ok(LayerSource::Asset(asset)) => {
                        if !self.assets.contains_key(&asset) {
                            problem(
                                index,
                                Some(name.clone()),
                                format!(
                                    "the layer names the asset {asset:?}, which the storyboard does not carry"
                                ),
                            );
                        }
                    }
                    Ok(LayerSource::Scene(scene)) => {
                        // A layer naming a scene: the scene must exist,
                        // and no chain of them may come back to itself.
                        let target = self
                            .scenes
                            .iter()
                            .position(|card| card.id.as_deref() == Some(scene.as_str()));
                        match target {
                            None => problem(
                                index,
                                Some(name.clone()),
                                format!(
                                    "the layer names the scene {scene:?}, which this storyboard does not define"
                                ),
                            ),
                            Some(target) => {
                                if let Some(chain) =
                                    self.scene_cycle(target, index, &mut Vec::new())
                                {
                                    problem(
                                        index,
                                        Some(name.clone()),
                                        format!("a scene cannot be built from itself: {chain}"),
                                    );
                                }
                            }
                        }
                    }
                }
                if let Some(blend) = &layer.blend
                    && blend_of(blend).is_none()
                {
                    problem(
                        index,
                        Some(name.clone()),
                        format!(
                            "the blend {blend:?} is not one of normal, multiply, screen, overlay, darken, lighten, difference, soft-light or add"
                        ),
                    );
                }
                if let Some(fit) = &layer.fit
                    && !matches!(fit.as_str(), "cover" | "contain" | "fill" | "none")
                {
                    problem(
                        index,
                        Some(name.clone()),
                        format!("the fit {fit:?} is not one of cover, contain, fill or none"),
                    );
                }
                for (field, value) in [
                    ("zoom", layer.zoom),
                    ("scale", layer.scale),
                    ("opacity", layer.opacity),
                    ("fade_in", layer.fade_in),
                    ("fade_out", layer.fade_out),
                    ("start", layer.start),
                    ("hold", layer.hold),
                    ("parallax", layer.parallax),
                ] {
                    if let Some(value) = value
                        && !value.is_finite()
                    {
                        problem(
                            index,
                            Some(name.clone()),
                            format!("`{field}` is {value}, which is not a finite number"),
                        );
                    }
                }
                for (field, value) in [("pan", layer.pan), ("offset", layer.offset)] {
                    if let Some((x, y)) = value
                        && !(x.is_finite() && y.is_finite())
                    {
                        problem(
                            index,
                            Some(name.clone()),
                            format!("`{field}` is [{x}, {y}], and both must be finite numbers"),
                        );
                    }
                }
                for (field, value) in [
                    ("start", layer.start),
                    ("hold", layer.hold),
                    ("fade_in", layer.fade_in),
                    ("fade_out", layer.fade_out),
                ] {
                    if let Some(value) = value
                        && value < 0.0
                    {
                        problem(
                            index,
                            Some(name.clone()),
                            format!("`{field}` is {value}, and it cannot be negative"),
                        );
                    }
                }
                if let Some(opacity) = layer.opacity
                    && opacity.is_finite()
                    && !(0.0..=1.0).contains(&opacity)
                {
                    problem(
                        index,
                        Some(name.clone()),
                        format!("`opacity` is {opacity}, and it is a fraction between 0 and 1"),
                    );
                }
                if let Some(rate) = layer.parallax
                    && rate.is_finite()
                    && rate <= 0.0
                {
                    problem(
                        index,
                        Some(name.clone()),
                        format!(
                            "`parallax` is {rate}, and it is a positive multiple of the layer's drift"
                        ),
                    );
                }
            }
        }

        // The beds of sound under the whole thing.
        for bed in &self.audio {
            if !self.assets.contains_key(&bed.asset) {
                problem(
                    0,
                    Some(bed.asset.clone()),
                    format!(
                        "the audio bed names the asset {:?}, which the storyboard does not carry",
                        bed.asset
                    ),
                );
            }
            if !bed.gain_db.is_finite() {
                problem(
                    0,
                    Some(bed.asset.clone()),
                    format!("`gain_db` is {}, which is not a finite number", bed.gain_db),
                );
            }
            for (field, value) in [("fade_in", bed.fade_in), ("fade_out", bed.fade_out)] {
                if let Some(value) = value
                    && !finite_non_negative(value)
                {
                    problem(
                        0,
                        Some(bed.asset.clone()),
                        format!("`{field}` is {value}, and it cannot be negative"),
                    );
                }
            }
        }

        // The curtain, when there is one.
        if let Some(curtain) = &self.curtain {
            for (field, value) in [
                ("open", curtain.open),
                ("close", curtain.close),
                ("dip", curtain.dip),
                ("depth", curtain.depth),
            ] {
                if !finite_non_negative(value) {
                    problem(
                        0,
                        None,
                        format!("the curtain's `{field}` is {value}, and it cannot be negative"),
                    );
                }
            }
            if curtain.depth.is_finite() && !(0.0..=1.0).contains(&curtain.depth) {
                problem(
                    0,
                    None,
                    format!(
                        "the curtain's `depth` is {}, and it is a fraction between 0 and 1",
                        curtain.depth
                    ),
                );
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// Whether the scene at `index` can be reached from itself through
    /// named scenes, and the chain that closes the loop, as a sentence.
    /// `from` is the scene whose layer started the walk.
    fn scene_cycle(&self, index: usize, from: usize, seen: &mut Vec<usize>) -> Option<String> {
        if seen.contains(&index) || index == from {
            // The chain reads from the scene that erred: a uses b uses a.
            let name = |i: usize| {
                self.scenes
                    .get(i)
                    .and_then(|c| c.id.clone())
                    .unwrap_or_else(|| format!("scene {i}"))
            };
            let mut chain = vec![name(from)];
            chain.extend(seen.iter().map(|i| name(*i)));
            chain.push(name(index));
            return Some(chain.join(" uses "));
        }
        let card = self.scenes.get(index)?;
        let uses_scenes: Vec<usize> = card
            .layers
            .iter()
            .filter_map(|l| l.scene.as_ref())
            .filter_map(|name| {
                self.scenes
                    .iter()
                    .position(|card| card.id.as_deref() == Some(name.as_str()))
            })
            .collect();
        seen.push(index);
        for next in uses_scenes {
            if let Some(chain) = self.scene_cycle(next, from, seen) {
                return Some(chain);
            }
        }
        seen.pop();
        None
    }

    /// The sequence this storyboard lays out, its assets resolved from
    /// `root`.
    ///
    /// Every scene is resolved and every operation parsed before anything is
    /// built, so a storyboard that does not make sense comes back as one
    /// error naming the scene - not as a broken render.
    pub fn sequence(&self, root: &Path) -> Result<Sequence, String> {
        // The contract first: every problem the document carries, all at
        // once, before a preset is resolved or a file is read. Both doors
        // eat the same refusal, and an invalid storyboard never reaches
        // an engine.
        if let Err(problems) = self.validate() {
            let list = problems
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<String>>()
                .join("; ");
            return Err(list);
        }
        let source = self
            .source_dir
            .clone()
            .unwrap_or_else(|| root.to_path_buf());
        let mut sequence = Sequence::new(self.width, self.height, self.fps);
        sequence.source_dir = source.clone();
        sequence.scene_cards = self.scenes.clone();
        for (id, src) in &self.assets {
            sequence.asset(
                id.clone(),
                Asset {
                    src: src.clone(),
                    kind: None,
                },
            );
        }
        if let Some(curtain) = &self.curtain {
            sequence.curtain = Some(Curtain {
                color: Rgba::hex(&curtain.color),
                open: curtain.open,
                close: curtain.close,
                dip: curtain.dip,
                depth: curtain.depth,
            });
        }
        for (index, card) in self.scenes.iter().enumerate() {
            sequence.push(card.scene(index, &source)?);
        }
        for bed in &self.audio {
            if !self.assets.contains_key(&bed.asset) {
                return Err(format!(
                    "the audio bed names the asset {:?}, which the storyboard does not carry",
                    bed.asset
                ));
            }
            let mut sound = AudioBed::new(bed.asset.clone()).at(bed.gain_db);
            if let Some(fade) = bed.fade_in {
                sound.fade_in = fade;
            }
            if let Some(fade) = bed.fade_out {
                sound.fade_out = fade;
            }
            sequence.bed(sound);
        }
        Ok(sequence)
    }
}

impl SceneCard {
    /// The scene this card describes: its comp resolved, its operations
    /// parsed, its layers stacked. Errors name the scene's index, so a
    /// twelve-scene storyboard is debuggable from one refusal.
    pub(crate) fn scene(&self, index: usize, root: &Path) -> Result<Scene, String> {
        let comp = match (&self.preset, &self.comp) {
            (Some(id), _) => presets::build(id)
                .ok_or_else(|| format!("scene {index}: no preset called {id:?}"))?,
            (None, Some(file)) => read_comp(index, root, file)?,
            (None, None) => return Err(format!("scene {index} needs a `preset` or a `comp`")),
        };
        let mut scene = Scene::new(comp).with_ops(self.ops(index)?);
        if let Some(at) = self.at {
            scene = scene.starting_at(at);
        }
        if let Some(hold) = self.hold {
            scene = scene.holding(hold);
        }
        // The short form first, so a scene that names both keeps its plate
        // on the bottom.
        if let Some(plate) = &self.plate {
            scene = scene.over(plate.clone(), self.zoom.unwrap_or(0.0));
        }
        for layer in &self.layers {
            scene = scene.layer(layer.layer(index)?);
        }
        Ok(scene)
    }

    /// The card's operations, parsed. A malformed one names its scene and
    /// its index in the batch.
    fn ops(&self, index: usize) -> Result<Vec<Op>, String> {
        self.ops
            .iter()
            .enumerate()
            .map(|(at, op)| {
                serde_json::from_value(op.clone())
                    .map_err(|error| format!("scene {index}, operation {at}: {error}"))
            })
            .collect()
    }
}

impl LayerCard {
    /// The layer this card describes, for a card whose source is an
    /// asset; a scene-source layer is placed by the planner, not turned
    /// into a picture layer.
    fn layer(&self, index: usize) -> Result<SceneLayer, String> {
        // A scene-source layer carries the scene's name as its asset: the
        // planner reads `scene` off it and places the composition, and
        // errors name the scene rather than an empty string.
        let mut layer = SceneLayer::new(
            self.scene
                .clone()
                .or_else(|| self.asset.clone())
                .unwrap_or_default(),
        );
        if let Some(scene) = &self.scene {
            layer.scene = Some(scene.clone());
        }
        if let Some(zoom) = self.zoom {
            layer = layer.zooming(zoom);
        }
        if let Some((x, y)) = self.pan {
            layer = layer.panning(x, y);
        }
        if let Some((x, y)) = self.offset {
            layer = layer.at_offset(x, y);
        }
        if let Some(scale) = self.scale {
            layer = layer.scaled(scale);
        }
        if let Some(fit) = &self.fit {
            layer = layer.fitted(match fit.as_str() {
                "cover" => Fit::Cover,
                "contain" => Fit::Contain,
                "fill" => Fit::Fill,
                "none" => Fit::None,
                other => {
                    return Err(format!(
                        "scene {index}, the layer {:?}: the fit {other:?} is not one of \
                             cover, contain, fill or none",
                        self.asset
                    ));
                }
            });
        }
        if let Some(blend) = &self.blend {
            layer = layer.blending(match blend.as_str() {
                "normal" => Blend::Normal,
                "multiply" => Blend::Multiply,
                "screen" => Blend::Screen,
                "overlay" => Blend::Overlay,
                "darken" => Blend::Darken,
                "lighten" => Blend::Lighten,
                "difference" => Blend::Difference,
                "soft-light" => Blend::SoftLight,
                "add" => Blend::Add,
                other => {
                    return Err(format!(
                        "scene {index}, the layer {:?}: the blend {other:?} is not one of \
                             normal, multiply, screen, overlay, darken, lighten, difference, \
                             soft-light or add",
                        self.asset
                    ));
                }
            });
        }
        if let Some(opacity) = self.opacity {
            layer = layer.opacity(opacity);
        }
        match (self.fade_in, self.fade_out) {
            (Some(in_seconds), Some(out_seconds)) => {
                layer = layer.fading(in_seconds, out_seconds);
            }
            (Some(in_seconds), None) => {
                layer.fade_in = in_seconds;
            }
            (None, Some(out_seconds)) => {
                layer.fade_out = out_seconds;
            }
            (None, None) => {}
        }
        if let Some(start) = self.start {
            layer = layer.starting_at(start);
        }
        if let Some(hold) = self.hold {
            layer = layer.holding(hold);
        }
        if let Some(rate) = self.parallax {
            layer = layer.parallax(rate);
        }
        if self.above == Some(true) {
            layer = layer.above();
        }
        Ok(layer)
    }
}

/// A comp document read from under the storyboard's root. A comp path that
/// climbs out of the root or names a whole disk is refused: a storyboard is
/// a document that travels, and one that reaches around its own folder does
/// not.
fn read_comp(index: usize, root: &Path, file: &str) -> Result<Comp, String> {
    if file.starts_with('/') || file.contains("..") {
        return Err(format!(
            "scene {index}: the comp {file:?} is a name under the storyboard's folder, not a path out of it"
        ));
    }
    let path = root.join(file);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("scene {index}: {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("scene {index}: {}: {error}", path.display()))
}

/// A content hash of a storyboard document, for cache naming: the same
/// storyboard, the same number, on every machine. FNV-1a over the
/// document's canonical JSON, the same scheme a comp's own hash uses. Not a
/// security hash - nothing here is adversarial.
pub fn hash(document: &Json) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(b"concat-motion/storyboard/1");
    match serde_json::to_string(document) {
        Ok(json) => eat(json.as_bytes()),
        Err(_) => eat(b"unserialisable"),
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn board(text: &str) -> Storyboard {
        Storyboard::parse(text).expect("a storyboard")
    }

    #[test]
    fn a_full_storyboard_builds_its_sequence_with_layers() {
        let storyboard = board(
            r##"{
              "width": 1080, "height": 1920, "fps": 30,
              "assets": { "back": "assets/back.jpg", "leak": "assets/leak.png",
                          "score": "assets/score.wav" },
              "scenes": [
                { "preset": "funcut.sting.wordmark", "hold": 4.0,
                  "layers": [
                    { "asset": "back", "zoom": 0.04 },
                    { "asset": "leak", "blend": "screen", "scale": 0.8,
                      "offset": [-0.25, -0.2], "fadeIn": 0.4, "fadeOut": 0.4, "above": true }
                  ],
                  "ops": [ { "op": "set-slot", "slot": "Word", "value": "FUNCUT" } ] }
              ],
              "audio": [ { "asset": "score", "gain_db": -2.0, "fade_in": 0.4 } ],
              "curtain": { "color": "#101014ff", "open": 0.5, "close": 0.7, "dip": 0.2, "depth": 1.0 }
            }"##,
        );
        let root = Path::new("/tmp/nowhere");
        let sequence = storyboard.sequence(root).expect("a sequence");

        assert_eq!(sequence.width, 1080);
        assert_eq!(sequence.height, 1920);
        assert_eq!(sequence.source_dir, root);
        let scene = &sequence.scenes[0];
        assert_eq!(scene.comp.id, "funcut.sting.wordmark");
        assert_eq!(scene.hold, Some(4.0));
        assert_eq!(scene.layers.len(), 2);
        assert_eq!(scene.layers[0].asset, "back");
        assert_eq!(scene.layers[0].zoom, 0.04);
        assert!(!scene.layers[0].above);
        assert_eq!(scene.layers[1].asset, "leak");
        assert_eq!(scene.layers[1].blend, Blend::Screen);
        assert_eq!(scene.layers[1].scale, 0.8);
        assert_eq!(scene.layers[1].offset, (-0.25, -0.2));
        assert_eq!(scene.layers[1].fade_in, 0.4);
        assert!(scene.layers[1].above);
        // The operation reached the comp the plan will place.
        assert!(matches!(scene.ops.first(), Some(Op::SetSlot { .. })));
        // The curtain and the bed, as written.
        assert_eq!(sequence.curtain.as_ref().expect("a curtain").dip, 0.2);
        assert_eq!(sequence.audio[0].asset, "score");
        assert_eq!(sequence.audio[0].gain_db, -2.0);
        assert_eq!(sequence.audio[0].fade_in, 0.4);
    }

    #[test]
    fn the_short_form_plate_is_one_layer_on_the_bottom() {
        let storyboard = board(
            r##"{ "assets": { "back": "a.jpg", "front": "b.png" },
                 "scenes": [
                   { "preset": "funcut.chapter.chip", "plate": "back", "zoom": 0.05,
                     "layers": [ { "asset": "front", "pan": [0.02, 0.0] } ] }
                 ] }"##,
        );
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let scene = &sequence.scenes[0];
        assert_eq!(scene.layers.len(), 2);
        assert_eq!(scene.layers[0].asset, "back");
        assert_eq!(scene.layers[0].zoom, 0.05);
        assert_eq!(scene.layers[1].asset, "front");
        assert_eq!(scene.layers[1].pan, (0.02, 0.0));
    }

    #[test]
    fn a_scene_without_a_preset_or_comp_is_refused_by_its_index() {
        let storyboard = board(r##"{ "scenes": [ {}, { "preset": "nope" } ] }"##);
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains("scene 0"), "{error}");

        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip" }, { "preset": "nope" } ] }"##,
        );
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains("scene 1"), "{error}");
        assert!(error.contains("nope"), "{error}");
    }

    #[test]
    fn a_bad_fit_or_blend_names_the_layer_and_the_scene() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip",
                 "layers": [ { "asset": "x", "fit": "stretch" } ] } ] }"##,
        );
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains("scene 0"), "{error}");
        assert!(error.contains("stretch"), "{error}");

        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip",
                 "layers": [ { "asset": "x", "blend": "glow" } ] } ] }"##,
        );
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains("blend"), "{error}");
        assert!(error.contains("glow"), "{error}");
    }

    #[test]
    fn a_bed_naming_an_unknown_asset_is_refused() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip" } ],
                 "audio": [ { "asset": "nothing" } ] }"##,
        );
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains("nothing"), "{error}");
    }

    #[test]
    fn a_comp_path_out_of_the_folder_is_refused() {
        let storyboard = board(r##"{ "scenes": [ { "comp": "../elsewhere.json" } ] }"##);
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains(".."), "{error}");
    }

    #[test]
    fn a_malformed_operation_names_its_scene_and_index() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip",
                 "ops": [ { "op": "set-slot", "slot": "Chapter" } ] } ] }"##,
        );
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains("scene 0"), "{error}");
        assert!(error.contains("operation 0"), "{error}");
    }

    #[test]
    fn validation_refuses_a_layer_whose_asset_is_not_carried() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip",
                 "layers": [ { "asset": "nowhere" } ] } ] }"##,
        );
        let problems = storyboard.validate().expect_err("refused");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].scene, 0);
        assert_eq!(problems[0].layer.as_deref(), Some("nowhere"));
        assert!(
            problems[0].problem.contains("nowhere"),
            "{}",
            problems[0].problem
        );
        // The same refusal through the door a caller actually uses.
        let error = storyboard.sequence(Path::new(".")).expect_err("refused");
        assert!(error.contains("scene 0"), "{error}");
        assert!(error.contains("nowhere"), "{error}");
    }

    #[test]
    fn validation_names_every_problem_in_one_refusal() {
        // Three problems in three scenes: all three are named, so a
        // storyboard is debugged from one refusal instead of three
        // render attempts.
        let storyboard = board(
            r##"{ "scenes": [
                 { "preset": "funcut.chapter.chip",
                   "layers": [ { "asset": "gone" } ] },
                 { "preset": "funcut.chapter.chip",
                   "layers": [ { "asset": "gone", "blend": "glow", "start": -1.0 } ] },
                 { "preset": "funcut.sting.wordmark", "hold": -2.0 }
               ] }"##,
        );
        let problems = storyboard.validate().expect_err("refused");
        assert!(problems.len() >= 5, "{problems:?}");
        assert!(
            problems
                .iter()
                .any(|p| p.scene == 2 && p.problem.contains("hold"))
        );
        assert!(
            problems
                .iter()
                .any(|p| p.scene == 1 && p.problem.contains("blend") && p.problem.contains("glow"))
        );
        assert!(problems.iter().any(|p| p.scene == 1
            && p.problem.contains("start")
            && p.problem.contains("negative")));
    }

    #[test]
    fn validation_refuses_a_bad_fit_and_an_out_of_range_opacity() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip",
                 "layers": [ { "asset": "a", "fit": "stretch", "opacity": 1.5 } ] } ],
                 "assets": { "a": "a.png" } }"##,
        );
        let problems = storyboard.validate().expect_err("refused");
        assert!(
            problems.iter().any(|p| p.problem.contains("fit")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.problem.contains("opacity")),
            "{problems:?}"
        );
    }

    #[test]
    fn validation_refuses_a_number_that_is_not_finite() {
        // JSON cannot carry a NaN or an infinity; a program building the
        // document can, so the check is on the struct a program writes.
        let mut storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip",
                 "layers": [ { "asset": "a", "scale": 1.0 } ] } ],
                 "assets": { "a": "a.png" } }"##,
        );
        storyboard.scenes[0].layers[0].scale = Some(f64::INFINITY);
        let problems = storyboard.validate().expect_err("refused");
        assert!(
            problems
                .iter()
                .any(|p| p.problem.contains("scale") && p.problem.contains("finite")),
            "{problems:?}"
        );
    }

    #[test]
    fn validation_refuses_a_scene_naming_both_a_preset_and_a_comp() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.chapter.chip", "comp": "mine.json" } ] }"##,
        );
        let problems = storyboard.validate().expect_err("refused");
        assert!(
            problems.iter().any(|p| p.problem.contains("both")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_valid_storyboard_validates_clean() {
        let storyboard = board(
            r##"{ "width": 1080, "height": 1920, "fps": 30,
                 "assets": { "back": "assets/back.png", "score": "assets/score.wav" },
                 "scenes": [
                   { "preset": "funcut.sting.wordmark", "hold": 4.0,
                     "layers": [ { "asset": "back", "zoom": 0.04, "blend": "screen",
                                   "fade_in": 0.4, "start": 0.0, "opacity": 0.9 } ] } ],
                 "audio": [ { "asset": "score", "gain_db": -2.0 } ] }"##,
        );
        assert_eq!(storyboard.validate(), Ok(()));
    }

    #[test]
    fn a_scene_can_be_placed_inside_another_scene() {
        let storyboard = board(
            r##"{ "assets": { "back": "back.png" },
                 "scenes": [
                   { "id": "intro", "preset": "funcut.sting.wordmark", "hold": 3.0,
                     "layers": [ { "asset": "back", "zoom": 0.04 } ] },
                   { "preset": "funcut.chapter.chip", "hold": 5.0,
                     "layers": [ { "scene": "intro", "scale": 0.6, "offset": [0.0, 0.1],
                                   "start": 1.0, "hold": 2.5, "blend": "screen", "above": true } ] }
                 ] }"##,
        );
        let sequence = storyboard.sequence(Path::new(".")).expect("a sequence");
        let geneva = crate::geneva::Geneva::at("geneva");
        let planned = crate::sequence::plan(&geneva, &sequence).expect("a plan");
        let document = &planned.document;
        // The named scene is emitted once as a composition, its layers at
        // relative times.
        let composition = &document["compositions"]["s0.intro"];
        assert!(
            composition.is_object(),
            "{}",
            serde_json::to_string(document).unwrap_or_default()
        );
        assert_eq!(composition["width"], json!(1920));
        assert_eq!(composition["background"], json!("transparent"));
        assert!(
            composition["layers"].as_array().unwrap().len() >= 1,
            "the scene's own stack"
        );
        // The placing layer is a clip with a composition source, at the
        // parent's start plus the layer's own.
        let layers = document["layers"].as_array().unwrap();
        let placed = layers
            .iter()
            .find(|l| l["id"] == json!("s1.overlay"))
            .expect("the placing layer");
        let clip = &placed["clips"][0];
        assert_eq!(clip["source"]["kind"], json!("composition"));
        assert_eq!(clip["source"]["composition"], json!("s0.intro"));
        assert_eq!(
            clip["start"],
            json!("4s"),
            "the second scene starts at 3s, the layer at +1s"
        );
        assert_eq!(clip["duration"], json!("2.5s"));
        assert_eq!(clip["blend"], json!("screen"));
        assert_eq!(clip["transform"]["scale"], json!(0.6));
    }

    #[test]
    fn a_scene_cannot_be_built_from_itself_directly_or_through_another() {
        let direct = board(
            r##"{ "scenes": [
                 { "id": "ouroboros", "preset": "funcut.sting.wordmark",
                   "layers": [ { "scene": "ouroboros" } ] } ] }"##,
        );
        let problems = direct.validate().expect_err("refused");
        assert!(
            problems
                .iter()
                .any(|p| p.problem.contains("cannot be built from itself")),
            "{problems:?}"
        );
        let error = direct
            .sequence(Path::new("."))
            .expect_err("refused by sequence too");
        assert!(error.contains("cannot be built from itself"), "{error}");

        // Through another: a uses b, b uses a.
        let looped = board(
            r##"{ "scenes": [
                 { "id": "a", "preset": "funcut.sting.wordmark",
                   "layers": [ { "scene": "b" } ] },
                 { "id": "b", "preset": "funcut.chapter.chip",
                   "layers": [ { "scene": "a" } ] } ] }"##,
        );
        let problems = looped.validate().expect_err("refused");
        let chain = problems
            .iter()
            .find(|p| p.problem.contains("uses"))
            .expect("the chain is named");
        assert!(
            chain.problem.contains("a uses b uses a"),
            "{}",
            chain.problem
        );
    }

    #[test]
    fn a_layer_naming_an_undefined_scene_is_refused_by_name() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.sting.wordmark",
                 "layers": [ { "scene": "nowhere" } ] } ] }"##,
        );
        let problems = storyboard.validate().expect_err("refused");
        assert!(
            problems
                .iter()
                .any(|p| p.problem.contains("nowhere") && p.problem.contains("does not define")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_layer_naming_both_an_asset_and_a_scene_is_refused() {
        let storyboard = board(
            r##"{ "scenes": [ { "preset": "funcut.sting.wordmark",
                 "layers": [ { "asset": "back", "scene": "intro" } ] } ],
                 "assets": { "back": "b.png" } }"##,
        );
        let problems = storyboard.validate().expect_err("refused");
        assert!(
            problems.iter().any(|p| p.problem.contains("both")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_hash_is_stable_and_differs_on_a_change() {
        let a = serde_json::json!({ "scenes": [ { "preset": "funcut.chapter.chip" } ] });
        let same = serde_json::json!({ "scenes": [ { "preset": "funcut.chapter.chip" } ] });
        let other = serde_json::json!({ "scenes": [ { "preset": "funcut.sting.wordmark" } ] });
        assert_eq!(hash(&a), hash(&same));
        assert_ne!(hash(&a), hash(&other));
    }
}
