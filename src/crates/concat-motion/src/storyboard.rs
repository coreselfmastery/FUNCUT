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

/// A storyboard: several scenes, in order, as one render.
#[derive(serde::Deserialize)]
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
#[derive(serde::Deserialize)]
pub struct SceneCard {
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

/// One layer of a scene, as a storyboard writes it. Every field but
/// `asset` is optional; the defaults are [`SceneLayer`]'s own.
#[derive(serde::Deserialize)]
pub struct LayerCard {
    /// An asset id from the storyboard's assets.
    pub asset: String,
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
    /// Whether it paints over the comp rather than under it.
    #[serde(default)]
    pub above: Option<bool>,
}

/// A bed of sound under the whole sequence.
#[derive(serde::Deserialize)]
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
#[derive(serde::Deserialize)]
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

    /// The sequence this storyboard lays out, its assets resolved from
    /// `root`.
    ///
    /// Every scene is resolved and every operation parsed before anything is
    /// built, so a storyboard that does not make sense comes back as one
    /// error naming the scene - not as a broken render.
    pub fn sequence(&self, root: &Path) -> Result<Sequence, String> {
        let source = self
            .source_dir
            .clone()
            .unwrap_or_else(|| root.to_path_buf());
        let mut sequence = Sequence::new(self.width, self.height, self.fps);
        sequence.source_dir = source.clone();
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
    fn scene(&self, index: usize, root: &Path) -> Result<Scene, String> {
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
    /// The layer this card describes.
    fn layer(&self, index: usize) -> Result<SceneLayer, String> {
        let mut layer = SceneLayer::new(self.asset.clone());
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
    fn the_hash_is_stable_and_differs_on_a_change() {
        let a = serde_json::json!({ "scenes": [ { "preset": "funcut.chapter.chip" } ] });
        let same = serde_json::json!({ "scenes": [ { "preset": "funcut.chapter.chip" } ] });
        let other = serde_json::json!({ "scenes": [ { "preset": "funcut.sting.wordmark" } ] });
        assert_eq!(hash(&a), hash(&same));
        assert_ne!(hash(&a), hash(&other));
    }
}
