// SPDX-License-Identifier: AGPL-3.0-or-later
// FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The looks: texture over the picture, as CSS the engine draws.
//!
//! The project's effect catalogue owns the pixel shaders - bloom, neon,
//! the chroma keys - and they run on footage through the renderer's own
//! compositor. A motion comp cannot carry one: it is a document, and the
//! engine that bakes it is a CSS raster with no shader stage. What a comp
//! *can* carry is the CSS that approximates the three looks every
//! template reaches for, and this module writes that CSS, as nodes the
//! engine draws in the comp's own paint order:
//!
//! - [`Look::Grain`] - a seeded field of motes over everything, ink
//!   included, shimmering on the node's own timeline track. Film grain is
//!   halide; this is a few hundred deterministic motes, capped where the
//!   engine's layout stays quick, and it says so.
//! - [`Look::Glass`] - a full-bleed frost first in the paint order, under
//!   the comp's ink: `backdrop-filter` blurs and saturates whatever the
//!   scene has under the comp, and a faint white wash lifts it. The words
//!   stay sharp; the world behind them goes glassy. Alone - a comp baked
//!   with no plate under it - it frosts nothing, and says so in the plan's
//!   warnings, the way the grade does.
//! - [`Look::Chromatic`] - the edge fringe: a warm fall-off on the left,
//!   a cool one on the right, over everything. This is the honest
//!   approximation a browserless raster can draw; a true channel split
//!   needs per-channel masks the engine has none of, and the module says
//!   so rather than pretending.
//! - [`Look::Vignette`] - the corners fall, the centre holds: a dark
//!   fall-off from all four edges over everything, the same one-gradient-
//!   per-edge shape the fringe is, four times. A true radial falloff needs
//!   stop positions the engine does not read; the four straight edges
//!   carry the vignette read without them.
//! - [`Look::Letterbox`] - the cinema bars: solid black bars that hold the
//!   frame to a wider ratio than the comp's own (2.39 on a 16:9 comp),
//!   top and bottom; a ratio under the comp's own is the pillarbox, the
//!   same bars on the sides. The bars are percentages of the frame, so
//!   the look holds at every size the comp bakes at.
//!
//! ```text
//! {"kind": "grain", "amount": 0.4, "size": 3, "seed": 7}
//! {"kind": "glass", "blur": 8, "saturate": 1.4}
//! {"kind": "chromatic", "amount": 0.35}
//! {"kind": "vignette", "amount": 0.4}
//! {"kind": "letterbox", "ratio": 2.39}
//! ```
//!
//! Measured against the engine (1.6.0) at 640x360 over a neutral grey
//! plate: the glass wash (+22 of 255 on grey) and both fringe gradients
//! draw; two-colour gradients without stop positions carry the spatial
//! fall-off, so each fringe is one gradient per edge and the grain is one
//! div per mote - the engine reads gradient colours but not stop
//! positions, and multiple backgrounds on one element drew nothing. And
//! markup animations are static per bake - measured, zero pixels moved
//! between two frames under a linear-timed keyframe set - so the grain's
//! shimmer rides the node's own `position` track, twelve steps-curved
//! keys, which the engine does animate per frame: measured again, the
//! field moved between frames the moment the track carried it. The
//! timeline's animation is the timeline's; the stylesheet's is a still.

use serde::{Deserialize, Serialize};

use crate::comp::{Blend, Comp, Length, Node, NodeKind};
use crate::curve::{Curve, StepPosition};
use crate::noise::Noise;
use crate::track::{Key, Track, Value};

/// The motes a full-strength grain field carries. Capped here because the
/// engine lays out every div in every frame: past this the bake slows
/// long before the grain reads any denser, and a look that costs more
/// than the picture is a look refused.
const GRAIN_MOTES: usize = 480;

/// The keys the grain's shimmer is walked in. Twelve steps a span reads
/// as a flicker at any frame rate the comps ship at; fewer reads as a
/// crawl, and the last key returns to the authored place so the span ends
/// where it began.
const GRAIN_KEYS: usize = 12;

/// One texture over the picture, as a storyboard writes it on the scene.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Look {
    /// A seeded field of motes over the whole frame, the comp's ink
    /// included, shimmering on the node's own position track.
    Grain {
        /// How hard the grain is put on, 0 to 1. Scales both the mote
        /// count and each mote's opacity; 0.25 to 0.6 is where grain
        /// reads on footage.
        amount: f64,
        /// One mote's side, in output pixels, 2 to 8. Three is the size
        /// that reads as texture at 1080p; past eight the field is a
        /// polka dot.
        size: f64,
        /// The whole grain's seed: the motes' places, their light or
        /// dark, and the shimmer's offsets all come from it. The same
        /// seed is the same grain on every machine and every re-render -
        /// the determinism contract - and a different seed is a different
        /// grain, never a drift.
        seed: u64,
    },
    /// A full-bleed frost first in the paint order: `backdrop-filter`
    /// blurs and saturates whatever the scene has under the comp - the
    /// plate, the lower layers - and a faint white wash lifts it. The
    /// comp's own ink is never frosted; that is the point of frosting
    /// under the words rather than over the render.
    Glass {
        /// The frost's radius, in output pixels, 0 to 24. Past 24 the
        /// pane reads as fog, not glass.
        blur: f64,
        /// The saturation the frost carries, 0.5 to 2. `1` is the blur
        /// alone; 1.3 to 1.6 is where glass reads most like glass.
        saturate: f64,
    },
    /// The chromatic fringe: a warm fall-off on the left edge, a cool one
    /// on the right, over everything. The approximation a browserless
    /// raster can draw honestly - a true channel split needs per-channel
    /// masks the engine has none of, and this says so by being a fringe
    /// rather than pretending to be an aberration.
    Chromatic {
        /// How hard the fringe is put on, 0 to 1. 0.2 to 0.4 is where
        /// the frame reads as a lens rather than a border.
        amount: f64,
    },
    /// The corners fall, the centre holds: a dark fall-off from all four
    /// edges over everything - the same one-gradient-per-edge shape the
    /// fringe is, four times, in black.
    Vignette {
        /// How hard the corners fall, 0 to 1. 0.3 to 0.6 is where the
        /// frame draws the eye inward without wearing sunglasses.
        amount: f64,
    },
    /// The cinema bars: solid black bars holding the frame to `ratio`
    /// (width over height) - 2.39 on a 16:9 comp is the widescreen
    /// cinema; a ratio under the comp's own is the pillarbox, the same
    /// bars on the sides.
    Letterbox {
        /// The ratio the frame is held to, 1 to 3. The comp's own aspect
        /// baked nothing (that is the frame as it stands); 2.39 is the
        /// scope screen, 1.0 the square.
        ratio: f64,
    },
}

impl Look {
    /// Every wrong number, in the look's own words, for the storyboard to
    /// name the scene. Empty is wearable.
    pub fn problems(&self) -> Vec<String> {
        let mut said = Vec::new();
        match self {
            Look::Grain { amount, size, .. } => {
                if !(0.0..=1.0).contains(amount) {
                    said.push(format!("the grain's amount is {amount}, outside 0 to 1"));
                }
                if !(2.0..=8.0).contains(size) {
                    said.push(format!("the grain's size is {size} px, outside 2 to 8"));
                }
            }
            Look::Glass { blur, saturate } => {
                if !(0.0..=24.0).contains(blur) {
                    said.push(format!(
                        "the glass's blur is {blur} px, outside 0 to 24 - past that it reads as fog"
                    ));
                }
                if !(0.5..=2.0).contains(saturate) {
                    said.push(format!(
                        "the glass's saturate is {saturate}, outside 0.5 to 2"
                    ));
                }
            }
            Look::Chromatic { amount } => {
                if !(0.0..=1.0).contains(amount) {
                    said.push(format!(
                        "the chromatic's amount is {amount}, outside 0 to 1"
                    ));
                }
            }
            Look::Vignette { amount } => {
                if !(0.0..=1.0).contains(amount) {
                    said.push(format!("the vignette's amount is {amount}, outside 0 to 1"));
                }
            }
            Look::Letterbox { ratio } => {
                if !(1.0..=3.0).contains(ratio) {
                    said.push(format!(
                        "the letterbox's ratio is {ratio}, outside 1 to 3 - the scope screen is 2.39"
                    ));
                }
            }
        }
        said
    }

    /// Whether the look, as written, changes anything: a grain at zero,
    /// a frost with no blur at unity saturation, a fringe at zero and a
    /// vignette at zero draw nothing, and baking them would only add
    /// nodes the engine carries for no picture. The letterbox is decided
    /// against the comp it is worn on - a ratio the comp already is, is
    /// the frame as it stands - which [`apply`] knows and this cannot.
    pub fn is_identity(&self) -> bool {
        match self {
            Look::Grain { amount, .. } => *amount <= 1e-9,
            Look::Glass { blur, saturate } => *blur <= 1e-9 && (saturate - 1.0).abs() <= 1e-9,
            Look::Chromatic { amount } => *amount <= 1e-9,
            Look::Vignette { amount } => *amount <= 1e-9,
            Look::Letterbox { .. } => false,
        }
    }
}

/// A number as CSS wants it: two decimals at most, so `0.44` never
/// becomes `0.44000000000000006`. Mirrors the grade's own, which is its
/// module's to give.
fn number(value: f64) -> String {
    let trimmed = (value * 100.0).round() / 100.0;
    if (trimmed - trimmed.round()).abs() < 1e-9 {
        format!("{}", trimmed.round() as i64)
    } else {
        format!("{trimmed}")
    }
}

/// The grain field: one div per mote, half light half dark, their places
/// and their polarities from the seed. Returned as the markup of one
/// `field` div the node draws.
fn grain_markup(comp: &Comp, amount: f64, size: f64, seed: u64) -> String {
    let noise = Noise::from_seed(seed);
    let count = ((GRAIN_MOTES as f64) * amount.clamp(0.0, 1.0)).round() as usize;
    let alpha = number(0.30 + 0.35 * amount.clamp(0.0, 1.0));
    let mut motes = String::new();
    for i in 0..count {
        // Two uncorrelated fields per axis: a mote's place is its own,
        // not a walk from the one before it.
        let x = (noise.noise2(i as f64 * 0.37 + 11.7, 4.2) * 0.5 + 0.5).clamp(0.0, 1.0);
        let y = (noise.noise2(9.1, i as f64 * 0.53 + 3.1) * 0.5 + 0.5).clamp(0.0, 1.0);
        let light = noise.noise2(i as f64 * 0.91 + 5.5, i as f64 * 0.29 + 9.2) > 0.0;
        let colour = if light {
            format!("rgba(255,255,255,{alpha})")
        } else {
            format!("rgba(0,0,0,{alpha})")
        };
        motes.push_str(&format!(
            "<div style=\"position:absolute;left:{:.0}px;top:{:.0}px;width:{size:.0}px;height:{size:.0}px;background:{colour}\"></div>",
            x * comp.width as f64,
            y * comp.height as f64,
        ));
    }
    format!(r#"<div class="field">{motes}</div>"#)
}

/// The grain's shimmer: a `position` track of twelve steps-curved keys
/// within a few pixels of the authored place. Markup animations are
/// static per bake - measured - so the shimmer rides the timeline, which
/// the engine animates per frame - measured too.
fn grain_track(amount: f64, seed: u64) -> Track {
    let noise = Noise::from_seed(seed ^ 0x5eed_0000_0000_0001);
    let reach = 1.0 + 2.0 * amount.clamp(0.0, 1.0);
    let mut keys = Vec::with_capacity(GRAIN_KEYS);
    for i in 0..GRAIN_KEYS {
        let at = i as f64 / (GRAIN_KEYS - 1) as f64;
        // First and last keys hold the authored place: the span starts
        // where it stands and ends where it began.
        let (x, y) = if i == 0 || i == GRAIN_KEYS - 1 {
            (0.0, 0.0)
        } else {
            let x = (noise.noise2(i as f64 * 1.7 + 0.3, 21.4) * reach).round() / 100.0;
            let y = (noise.noise2(17.9, i as f64 * 1.3 + 8.2) * reach).round() / 100.0;
            (x, y)
        };
        keys.push(Key {
            at,
            value: Value::Point { x, y },
            curve: if i == GRAIN_KEYS - 1 {
                None
            } else {
                Some(Curve::Steps {
                    count: 1,
                    position: StepPosition::JumpEnd,
                })
            },
        });
    }
    Track { keys }
}

/// The glass's stylesheet: one full-bleed rule, the frost and the wash.
fn glass_stylesheet(blur: f64, saturate: f64) -> String {
    format!(
        ".funcut-glass{{position:absolute;left:0;top:0;width:100%;height:100%;\
         backdrop-filter:blur({}px) saturate({});\
         background:linear-gradient(180deg,rgba(255,255,255,0.1),rgba(255,255,255,0.02));}}",
        number(blur),
        number(saturate),
    )
}

/// The chromatic fringe's stylesheet: one gradient per edge, no stop
/// positions - the engine reads gradient colours but not stop positions,
/// and the fall-off carries without them. Multiple backgrounds on one
/// element drew nothing when measured, so the edges are two elements.
fn chromatic_stylesheet(amount: f64) -> String {
    let alpha = number(0.12 + 0.30 * amount.clamp(0.0, 1.0));
    format!(
        ".edge-l{{position:absolute;inset:0;background:linear-gradient(90deg,rgba(255,0,60,{alpha}),rgba(0,0,0,0));}}\
         .edge-r{{position:absolute;inset:0;background:linear-gradient(270deg,rgba(0,255,220,{alpha}),rgba(0,0,0,0));}}"
    )
}

/// The vignette's stylesheet: the fringe's own shape, in black, from all
/// four edges - one gradient per edge, four elements, no stop positions
/// (the engine does not read them, and the straight-edged fall-off carries
/// the vignette read without a radial gradient).
fn vignette_stylesheet(amount: f64) -> String {
    let alpha = number(0.18 + 0.50 * amount.clamp(0.0, 1.0));
    format!(
        ".vig-t{{position:absolute;inset:0;background:linear-gradient(180deg,rgba(0,0,0,{alpha}),rgba(0,0,0,0));}}\
         .vig-b{{position:absolute;inset:0;background:linear-gradient(0deg,rgba(0,0,0,{alpha}),rgba(0,0,0,0));}}\
         .vig-l{{position:absolute;inset:0;background:linear-gradient(90deg,rgba(0,0,0,{alpha}),rgba(0,0,0,0));}}\
         .vig-r{{position:absolute;inset:0;background:linear-gradient(270deg,rgba(0,0,0,{alpha}),rgba(0,0,0,0));}}"
    )
}

/// The letterbox's stylesheet: solid black bars holding the frame to
/// `ratio`. The bar sizes are percentages of the frame, worked out from
/// the comp's own aspect once, here, so the look holds at every size the
/// comp bakes at - and a ratio the comp already is asks for no bars at
/// all, which [`apply`] answers before this is called.
fn letterbox_stylesheet(comp: &Comp, ratio: f64) -> String {
    let own = f64::from(comp.width) / f64::from(comp.height);
    if ratio > own {
        // Wider than the comp: bars top and bottom, each holding the
        // half of the frame's height the ratio gives back.
        let each = (1.0 - own / ratio) * 50.0;
        format!(
            ".bar-t{{position:absolute;left:0;top:0;width:100%;height:{}%;background:#000;}}\
             .bar-b{{position:absolute;left:0;bottom:0;width:100%;height:{}%;background:#000;}}",
            number(each),
            number(each),
        )
    } else {
        // Narrower than the comp: the pillarbox, the same bars on the
        // sides, each holding half the width the ratio gives back.
        let each = (1.0 - ratio / own) * 50.0;
        format!(
            ".bar-l{{position:absolute;top:0;left:0;height:100%;width:{}%;background:#000;}}\
             .bar-r{{position:absolute;top:0;right:0;height:100%;width:{}%;background:#000;}}",
            number(each),
            number(each),
        )
    }
}

/// The letterbox's markup: the bars the stylesheet sizes, named for the
/// half of the frame each holds.
fn letterbox_markup(comp: &Comp, ratio: f64) -> String {
    let own = f64::from(comp.width) / f64::from(comp.height);
    if ratio > own {
        r#"<div class="bar-t"></div><div class="bar-b"></div>"#.to_owned()
    } else {
        r#"<div class="bar-l"></div><div class="bar-r"></div>"#.to_owned()
    }
}

/// Bake a look into a comp. The grain and the fringe go on top, last in
/// paint order - film grain covers everything, and so does a lens. The
/// frost goes first, under the comp's ink, over whatever the scene has
/// under the comp - the plate, the lower layers - the same place the
/// grade sits, and for the same reason: the words stay sharp.
///
/// An identity look bakes nothing. A look that is wrong says so in its
/// own words, for the caller to name the scene.
pub fn apply(comp: &mut Comp, look: &Look) -> Result<(), String> {
    if let Some(problem) = look.problems().first() {
        return Err(problem.clone());
    }
    if look.is_identity() {
        return Ok(());
    }
    // The letterbox's identity is against the comp it is worn on: a ratio
    // the comp already is, is the frame as it stands, and bars of no
    // height would only add nodes the engine carries for no picture.
    if let Look::Letterbox { ratio } = look {
        let own = f64::from(comp.width) / f64::from(comp.height);
        if (ratio - own).abs() <= 1e-6 {
            return Ok(());
        }
    }
    // The node's name, unique in this comp: a second look on a nested
    // composition is not a collision to refuse, it is a name to have.
    let (base, over) = match look {
        Look::Grain { .. } => ("funcut-look-grain", true),
        Look::Glass { .. } => ("funcut-look-glass", false),
        Look::Chromatic { .. } => ("funcut-look-chromatic", true),
        Look::Vignette { .. } => ("funcut-look-vignette", true),
        Look::Letterbox { .. } => ("funcut-look-letterbox", true),
    };
    let mut id = base.to_owned();
    let mut suffix = 1;
    while comp.nodes.iter().any(|node| node.id == id) {
        suffix += 1;
        id = format!("{base}-{suffix}");
    }
    let (markup, css, tracks) = match look {
        Look::Grain { amount, size, seed } => (
            grain_markup(comp, *amount, *size, *seed),
            ".field{position:absolute;inset:0;}".to_owned(),
            [("position".to_owned(), grain_track(*amount, *seed))]
                .into_iter()
                .collect(),
        ),
        Look::Glass { blur, saturate } => (
            r#"<div class="funcut-glass"></div>"#.to_owned(),
            glass_stylesheet(*blur, *saturate),
            Default::default(),
        ),
        Look::Chromatic { amount } => (
            r#"<div class="edge-l"></div><div class="edge-r"></div>"#.to_owned(),
            chromatic_stylesheet(*amount),
            Default::default(),
        ),
        Look::Vignette { amount } => (
            r#"<div class="vig-t"></div><div class="vig-b"></div><div class="vig-l"></div><div class="vig-r"></div>"#
                .to_owned(),
            vignette_stylesheet(*amount),
            Default::default(),
        ),
        Look::Letterbox { ratio } => (
            letterbox_markup(comp, *ratio),
            letterbox_stylesheet(comp, *ratio),
            Default::default(),
        ),
    };
    let node = Node {
        id,
        parent: None,
        // The whole comp's span: `duration: 0` runs to the comp's end,
        // which the sequence clamps to the scene's hold.
        start: 0.0,
        duration: 0.0,
        transform: Default::default(),
        opacity: 1.0,
        blend: Blend::Normal,
        mask: None,
        effects: Vec::new(),
        tracks,
        kind: NodeKind::Html {
            markup,
            css,
            // A percentage box: the frame, whatever size the reel is,
            // because a look covers the picture, not the comp's authored
            // sheet of paper.
            width: Length::Percent(100.0),
            height: Length::Percent(100.0),
        },
    };
    if over {
        comp.nodes.push(node);
    } else {
        comp.nodes.insert(0, node);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, Rgba};
    use crate::engine::BakeRequest;
    use crate::geneva::Geneva;
    use std::collections::BTreeMap;

    fn empty_comp() -> Comp {
        Comp {
            format: FORMAT,
            id: "test.looks".into(),
            name: "Looks".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 4.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: BTreeMap::new(),
            curves: BTreeMap::new(),
            nodes: Vec::new(),
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    fn look_from(text: &str) -> Look {
        serde_json::from_str(text).expect("the look parses")
    }

    #[test]
    fn the_grain_field_is_seed_deterministic() {
        let one = {
            let mut comp = empty_comp();
            apply(
                &mut comp,
                &look_from(r#"{"kind":"grain","amount":0.5,"size":3,"seed":7}"#),
            )
            .expect("applies");
            comp
        };
        let two = {
            let mut comp = empty_comp();
            apply(
                &mut comp,
                &look_from(r#"{"kind":"grain","amount":0.5,"size":3,"seed":7}"#),
            )
            .expect("applies");
            comp
        };
        assert_eq!(one.hash(), two.hash(), "the same seed is the same grain");
        let other = {
            let mut comp = empty_comp();
            apply(
                &mut comp,
                &look_from(r#"{"kind":"grain","amount":0.5,"size":3,"seed":8}"#),
            )
            .expect("applies");
            comp
        };
        assert_ne!(
            one.hash(),
            other.hash(),
            "a different seed is a different grain"
        );
    }

    #[test]
    fn the_grain_counts_its_motes_and_caps_them() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"grain","amount":1.0,"size":3,"seed":1}"#),
        )
        .expect("applies");
        let node = comp
            .nodes
            .iter()
            .find(|node| node.id == "funcut-look-grain")
            .expect("the grain node");
        let NodeKind::Html { markup, .. } = &node.kind else {
            panic!("the grain is markup");
        };
        let motes = markup.matches("<div").count() - 1;
        assert_eq!(
            motes, GRAIN_MOTES,
            "full strength carries the whole capped field"
        );
    }

    #[test]
    fn the_grain_shimmers_on_the_timeline_not_in_the_stylesheet() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"grain","amount":0.5,"size":3,"seed":7}"#),
        )
        .expect("applies");
        let node = comp
            .nodes
            .iter()
            .find(|node| node.id == "funcut-look-grain")
            .expect("the grain node");
        let track = node
            .tracks
            .get("position")
            .expect("the shimmer rides the node's own position track");
        assert_eq!(track.keys.len(), GRAIN_KEYS);
        assert!(
            track.keys.iter().all(|key| (0.0..=1.0).contains(&key.at)),
            "the shimmer's keys stay inside the span"
        );
        assert_eq!(
            track.keys.first().map(|key| key.value.clone()),
            Some(Value::Point { x: 0.0, y: 0.0 }),
            "the span starts at the authored place"
        );
        assert!(
            track
                .keys
                .iter()
                .take(GRAIN_KEYS - 1)
                .all(|key| matches!(key.curve, Some(Curve::Steps { .. }))),
            "every segment jumps: the shimmer is a flicker, not a crawl"
        );
        let NodeKind::Html { css, .. } = &node.kind else {
            unreachable!();
        };
        assert!(
            !css.contains("animation"),
            "markup animations are static per bake - the grain carries none"
        );
    }

    #[test]
    fn the_frost_goes_under_the_ink_and_the_fringe_over_it() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"glass","blur":8.0,"saturate":1.4}"#),
        )
        .expect("applies");
        apply(
            &mut comp,
            &look_from(r#"{"kind":"chromatic","amount":0.35}"#),
        )
        .expect("applies");
        assert_eq!(
            comp.nodes.first().map(|node| node.id.as_str()),
            Some("funcut-look-glass"),
            "the frost is first in paint order, under the comp's ink"
        );
        assert_eq!(
            comp.nodes.last().map(|node| node.id.as_str()),
            Some("funcut-look-chromatic"),
            "the fringe is last, over everything"
        );
    }

    #[test]
    fn the_frost_carries_the_backdrop_and_the_fringe_two_gradients() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"glass","blur":8.0,"saturate":1.4}"#),
        )
        .expect("applies");
        let NodeKind::Html { css, .. } = &comp.nodes[0].kind else {
            unreachable!();
        };
        assert!(
            css.contains("backdrop-filter:blur(8px) saturate(1.4)"),
            "{css}"
        );
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"chromatic","amount":0.35}"#),
        )
        .expect("applies");
        let NodeKind::Html { ref css, .. } = comp.nodes.last().unwrap().kind else {
            unreachable!();
        };
        assert!(css.contains("linear-gradient(90deg"), "{css}");
        assert!(css.contains("linear-gradient(270deg"), "{css}");
        // Measured: stop positions are not read, and multiple backgrounds
        // on one element drew nothing. The fringe writes neither.
        assert!(!css.contains('%'), "no stop positions: {css}");
    }

    #[test]
    fn the_wrong_numbers_are_said_in_the_looks_own_words() {
        let grain = look_from(r#"{"kind":"grain","amount":1.5,"size":12,"seed":0}"#);
        let said = grain.problems();
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains("amount"), "{said:?}");
        assert!(said[1].contains("size"), "{said:?}");
        assert!(
            apply(&mut empty_comp(), &grain).is_err(),
            "a wrong look is refused, never half-worn"
        );
        let glass = look_from(r#"{"kind":"glass","blur":40.0,"saturate":1.0}"#);
        assert!(glass.problems().first().unwrap().contains("fog"));
        let fringe = look_from(r#"{"kind":"chromatic","amount":-1.0}"#);
        assert!(fringe.problems().first().unwrap().contains("amount"));
        let vignette = look_from(r#"{"kind":"vignette","amount":2.0}"#);
        assert!(vignette.problems().first().unwrap().contains("amount"));
        let bars = look_from(r#"{"kind":"letterbox","ratio":4.0}"#);
        assert!(bars.problems().first().unwrap().contains("ratio"));
    }

    #[test]
    fn an_identity_look_bakes_nothing() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"grain","amount":0.0,"size":3,"seed":7}"#),
        )
        .expect("applies");
        apply(
            &mut comp,
            &look_from(r#"{"kind":"glass","blur":0.0,"saturate":1.0}"#),
        )
        .expect("applies");
        apply(
            &mut comp,
            &look_from(r#"{"kind":"chromatic","amount":0.0}"#),
        )
        .expect("applies");
        apply(&mut comp, &look_from(r#"{"kind":"vignette","amount":0.0}"#)).expect("applies");
        // The comp is 16:9 already: a 16:9 letterbox is the frame as it
        // stands, and bakes nothing either.
        apply(
            &mut comp,
            &look_from(&format!(
                "{{\"kind\":\"letterbox\",\"ratio\":{}}}",
                1920.0 / 1080.0
            )),
        )
        .expect("applies");
        assert!(
            comp.nodes.is_empty(),
            "five identity looks left the comp as they found it"
        );
    }

    #[test]
    fn a_worn_look_comp_maps_clean_onto_a_timeline() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"grain","amount":0.4,"size":3,"seed":7}"#),
        )
        .expect("applies");
        apply(
            &mut comp,
            &look_from(r#"{"kind":"glass","blur":8.0,"saturate":1.4}"#),
        )
        .expect("applies");
        apply(
            &mut comp,
            &look_from(r#"{"kind":"chromatic","amount":0.35}"#),
        )
        .expect("applies");
        apply(&mut comp, &look_from(r#"{"kind":"vignette","amount":0.4}"#)).expect("applies");
        apply(
            &mut comp,
            &look_from(r#"{"kind":"letterbox","ratio":2.39}"#),
        )
        .expect("applies");
        comp.validate().into_iter().for_each(|d| panic!("{d:?}"));
        let (_document, warnings) = Geneva::at("geneva").timeline(&comp, &BakeRequest::default());
        assert!(
            warnings.is_empty(),
            "a worn look is drawn, not warned about: {warnings:?}"
        );
    }

    #[test]
    fn the_vignette_falls_from_all_four_edges_over_everything() {
        let mut comp = empty_comp();
        apply(&mut comp, &look_from(r#"{"kind":"vignette","amount":0.4}"#)).expect("applies");
        assert_eq!(
            comp.nodes.last().map(|node| node.id.as_str()),
            Some("funcut-look-vignette"),
            "the vignette is last in paint order, over everything"
        );
        let NodeKind::Html { css, markup, .. } = &comp.nodes.last().unwrap().kind else {
            unreachable!();
        };
        for edge in ["180deg", "0deg", "90deg", "270deg"] {
            assert!(css.contains(edge), "an edge is missing its fall-off: {css}");
        }
        assert!(
            css.contains("rgba(0,0,0,0.38)"),
            "the amount is the darkness: {css}"
        );
        assert!(!css.contains("%"), "no stop positions: {css}");
        assert_eq!(markup.matches("<div").count(), 4, "one element an edge");
    }

    #[test]
    fn the_letterbox_holds_the_ratio_the_frame_is_asked_for() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"letterbox","ratio":2.39}"#),
        )
        .expect("applies");
        let NodeKind::Html { css, markup, .. } = &comp.nodes.last().unwrap().kind else {
            unreachable!();
        };
        // 1 - (16/9)/2.39 is ~0.2562 of the height, half a bar each way:
        // 12.81%, in a percentage box, so the bars hold at every size.
        assert!(css.contains("height:12.81%"), "the scope bars: {css}");
        assert_eq!(markup.matches("<div").count(), 2, "a bar top, a bar bottom");
        assert!(css.contains("background:#000"), "the bars are solid: {css}");

        // A ratio under the comp's own is the pillarbox, the same bars on
        // the sides: 1 - 1/(16/9) is 43.75% of the width, half a bar each.
        let mut comp = empty_comp();
        apply(&mut comp, &look_from(r#"{"kind":"letterbox","ratio":1.0}"#)).expect("applies");
        let NodeKind::Html { css, .. } = &comp.nodes.last().unwrap().kind else {
            unreachable!();
        };
        assert!(css.contains("width:21.88%"), "the pillarbox: {css}");
        assert!(css.contains(".bar-l") && css.contains(".bar-r"), "{css}");
    }

    #[test]
    fn a_second_look_on_a_nested_comp_is_a_name_to_have() {
        let mut comp = empty_comp();
        apply(
            &mut comp,
            &look_from(r#"{"kind":"chromatic","amount":0.3}"#),
        )
        .expect("applies");
        apply(
            &mut comp,
            &look_from(r#"{"kind":"chromatic","amount":0.3}"#),
        )
        .expect("applies");
        let ids: Vec<&str> = comp.nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["funcut-look-chromatic", "funcut-look-chromatic-2"],
            "the second look is named, not refused"
        );
    }
}
