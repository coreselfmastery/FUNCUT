// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The native engine: the project's own compositor, in process.
//!
//! One [`CompEngine`] implementation behind the same trait the Geneva
//! sidecar sits behind, drawing the document itself rather than handing
//! it to a binary: shapes, vector paths (fills, strokes, dash draw-ons,
//! trims, morphs), text through the shaped-run path `concat-text`
//! provides, stills, nested comps, masks, track mattes, adjustment
//! layers, the nine blends, multi-sample motion blur and the shadow,
//! glow, blur and aberration effects - every pixel composited in linear
//! light, the way `concat-render` composites on the GPU.
//!
//! What it does not draw it says in [`Baked::warnings`], never silently:
//! markup (Geneva's alone), Lottie features outside the rasterlottie support
//! profile, shader packages (the GPU catalogue's). What Geneva cannot draw - paths,
//! shape gradients, adjustments, mattes, a scaled or rotated parent -
//! this engine draws whole, which is the fallback story the preference
//! keeps: a machine without the sidecar still bakes every comp that
//! carries no markup.
//!
//! The engine is deterministic by construction - no clock, no
//! randomness, one thread, the same faces in the same order - so the
//! same comp, assets and frame rate bake the same bytes on every
//! machine that has the fonts the comp names. A comp that wants its
//! text pinned across machines pins the font as an asset, the same
//! rule titles follow.

mod images;
mod raster;
mod scene;
mod shapes;
mod text;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::comp::Comp;
use crate::engine::{
    BakeRequest, Baked, CompEngine, EngineKind, MotionError, Probe, bake_assets_key, bake_cache_hit,
};
use images::Pictures;

/// Bumped whenever the renderer's pixels could change, so a cache
/// entry an older revision wrote stops matching and re-bakes. The
/// package version alone is not enough - a fix to a blend's math is a
/// new render of every comp, without a new package.
const RENDERER_REVISION: u8 = 2;

/// The native engine. Owns the faces a bake shapes text with, loaded
/// once and kept - building one per bake would re-read the machine's
/// fonts every time, which is the slowest thing this engine does.
pub struct Native {
    /// The faces: the system's, the bundled set, and every font file
    /// the comps this engine has drawn named as an asset.
    fonts: Mutex<concat_text::Fonts>,
    /// The font files already registered, so a re-bake of the same
    /// comp does not register its faces twice and double the database.
    registered: Mutex<HashSet<PathBuf>>,
}

impl Default for Native {
    fn default() -> Self {
        Native::new()
    }
}

impl Native {
    /// The engine, with the machine's faces and the bundled set loaded.
    /// One per process is the intended lifetime; a [`CompEngine`] takes
    /// `&self`, so one instance serves every bake and every frame.
    pub fn new() -> Native {
        Native {
            fonts: Mutex::new(concat_text::Fonts::new()),
            registered: Mutex::new(HashSet::new()),
        }
    }

    /// The engine's version, which a cache entry records and matches.
    fn version() -> String {
        format!(
            "concat-motion {}+raster{RENDERER_REVISION}",
            env!("CARGO_PKG_VERSION")
        )
    }

    /// Every font file the comp tree names as an asset, registered
    /// once. A font that will not load is left to the shaped-run
    /// path's own fallback, which the warnings of the text itself
    /// already cover: a missing face is a fallback, not a hole.
    fn register_font_assets(&self, comp: &Comp, source_dir: &Path) {
        fn walk(engine: &Native, comp: &Comp, source_dir: &Path, seen: &mut HashSet<String>) {
            for asset in comp.assets.values() {
                if !matches!(asset.kind, Some(crate::comp::AssetKind::Font)) {
                    continue;
                }
                if !seen.insert(asset.src.clone()) {
                    continue;
                }
                let path = source_dir.join(&asset.src);
                if engine
                    .registered
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .insert(path.clone())
                {
                    engine
                        .fonts
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .add_file(&path);
                }
            }
            for sub in comp.comps.values() {
                walk(engine, sub, source_dir, seen);
            }
        }
        let mut seen = HashSet::new();
        walk(self, comp, source_dir, &mut seen);
    }

    /// One frame of one comp: the whole pipeline, background to
    /// composite, as a pixmap of the asked-for size.
    fn draw(
        &self,
        comp: &Comp,
        at: f64,
        width: u32,
        height: u32,
        source_dir: &Path,
        warnings: &mut Vec<String>,
        pictures: &mut Pictures,
    ) -> Result<tiny_skia::Pixmap, MotionError> {
        let scale = if comp.width > 0 {
            f64::from(width) / f64::from(comp.width)
        } else {
            1.0
        };
        let fonts = self
            .fonts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut scene = scene::Scene {
            comp,
            source_dir: source_dir.to_path_buf(),
            fonts: &fonts,
            pictures,
            warnings,
            frame: (f64::from(width), f64::from(height)),
            scale,
        };
        Ok(scene.draw(at))
    }
}

impl CompEngine for Native {
    fn kind(&self) -> EngineKind {
        EngineKind::Native
    }

    fn probe(&self) -> Probe {
        Probe {
            available: true,
            version: Some(Native::version()),
            detail: "the in-process raster compositor: tiny-skia paths, concat-text shaping, \
                     linear-light blending"
                .into(),
        }
    }

    fn bake(&self, comp: &Comp, request: &BakeRequest, dir: &Path) -> Result<Baked, MotionError> {
        let problems = comp.validate();
        if !problems.is_empty() {
            return Err(MotionError::Invalid {
                diagnostics: problems,
            });
        }
        let version = Native::version();
        let source_dir = if request.source_dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            request.source_dir.clone()
        };
        // The cache answers before anything is rendered: an unchanged
        // comp at the same size and rate has nothing new to say.
        if let Some(hit) = bake_cache_hit(dir, comp, request, EngineKind::Native, &version) {
            return Ok(hit);
        }
        self.register_font_assets(comp, &source_dir);
        let (width, height) = (request.width(comp), request.height(comp));
        let fps = request.fps(comp);
        let count = ((comp.duration * fps).round() as u32).max(1);
        // A ranged bake renders its shard; the manifest is a whole
        // bake's promise, and a shard's manifest would count as one -
        // a cache entry ten frames deep in a sixty-frame comp. Shards
        // write frames only, and the merge that asked for them knows
        // what it asked for.
        let (first, last) = request
            .frames
            .map(|(from, to)| (from.max(1), to.min(count)))
            .unwrap_or((1, count));

        let frames = dir.join("frames");
        // A bake counts what the render wrote, so a stale `frames/`
        // left by an interrupted run must not survive into the count.
        let _ = std::fs::remove_dir_all(&frames);
        std::fs::create_dir_all(&frames).map_err(|source| MotionError::Io {
            doing: "creating".into(),
            path: frames.clone(),
            source,
        })?;
        let mut warnings: Vec<String> = Vec::new();
        // Still assets are immutable for the duration of a bake. Keep
        // decoded pixels across frames instead of rebuilding the cache
        // for every call to draw().
        let mut pictures = Pictures::default();
        let mut written = 0_usize;
        for index in first..=last.max(first) {
            let at = f64::from(index - 1) / fps;
            let frame = self.draw(
                comp,
                at,
                width,
                height,
                &source_dir,
                &mut warnings,
                &mut pictures,
            )?;
            let path = frames.join(Baked::frame_name(index));
            frame.save_png(&path).map_err(|source| MotionError::Io {
                doing: "writing".into(),
                path,
                source: std::io::Error::other(source.to_string()),
            })?;
            written += 1;
        }
        if written == 0 {
            return Err(MotionError::Engine {
                code: None,
                message: "the render wrote no frames".into(),
                path: None,
                help: Some(
                    "the bake directory is left as it is; baking into a fresh one answers \
                     whether the comp is at fault"
                        .into(),
                ),
            });
        }
        let baked = Baked {
            dir: frames.clone(),
            frames: written,
            width,
            height,
            fps,
            hash: comp.hash(),
            assets: bake_assets_key(comp, &source_dir),
            engine: EngineKind::Native,
            engine_version: version,
            alpha: request.alpha,
            warnings,
            cached: false,
        };
        if request.frames.is_some() {
            // A shard: no manifest, for the reason the range above
            // gives. The caller that asked for shards merges them.
            return Ok(baked);
        }
        let manifest = serde_json::to_string_pretty(&baked).unwrap_or_default();
        // The manifest is the bake's sentinel: written after the
        // frames, and renamed in from a temp file, so a concurrent
        // render either sees no manifest or a whole one.
        let manifest_path = dir.join("manifest.json");
        let manifest_part = crate::cache::part_path(&manifest_path);
        std::fs::write(&manifest_part, manifest).map_err(|source| MotionError::Io {
            doing: "writing the bake manifest".into(),
            path: manifest_path.clone(),
            source,
        })?;
        crate::cache::publish(&manifest_part, &manifest_path).map_err(|source| {
            MotionError::Io {
                doing: "publishing the bake manifest".into(),
                path: manifest_path.clone(),
                source,
            }
        })?;
        Ok(baked)
    }

    fn frame(
        &self,
        comp: &Comp,
        at: f64,
        width: Option<u32>,
        source_dir: &Path,
    ) -> Result<PathBuf, MotionError> {
        let problems = comp.validate();
        if !problems.is_empty() {
            return Err(MotionError::Invalid {
                diagnostics: problems,
            });
        }
        self.register_font_assets(comp, source_dir);
        let width = width.unwrap_or(comp.width).max(1);
        let height = ((f64::from(width) / f64::from(comp.width.max(1))) * f64::from(comp.height))
            .round()
            .max(1.0) as u32;
        // One scratch directory a call, so two frames asked for one
        // comp do not write the same file over each other.
        static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let scratch = std::env::temp_dir().join(format!(
            "concat-motion-native-{}-{}-{}",
            comp.hash(),
            std::process::id(),
            CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&scratch).map_err(|source| MotionError::Io {
            doing: "creating".into(),
            path: scratch.clone(),
            source,
        })?;
        let mut warnings = Vec::new();
        let mut pictures = Pictures::default();
        let drawn = self.draw(
            comp,
            at,
            width,
            height,
            source_dir,
            &mut warnings,
            &mut pictures,
        )?;
        let out = scratch.join("frame.png");
        drawn.save_png(&out).map_err(|source| MotionError::Io {
            doing: "writing".into(),
            path: out.clone(),
            source: std::io::Error::other(source.to_string()),
        })?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{
        Asset, AssetKind, Blend, Effect, Fit, Length, Mask, MaskShape, Node, NodeKind, Paint,
        Point, Rgba, Stroke, TextNode, Transform,
    };
    use crate::track::{Key, Track, Value};

    /// One engine for the whole test module: the faces load once.
    fn native() -> &'static Native {
        static ENGINE: std::sync::OnceLock<Native> = std::sync::OnceLock::new();
        ENGINE.get_or_init(Native::new)
    }

    fn comp_of(id: &str, nodes: Vec<Node>) -> Comp {
        Comp {
            format: crate::comp::FORMAT,
            id: id.into(),
            name: String::new(),
            width: 64,
            height: 48,
            fps: 10.0,
            duration: 1.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            nodes,
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    fn node(id: &str, kind: NodeKind) -> Node {
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

    fn rect(fill: Rgba) -> NodeKind {
        NodeKind::Rect {
            width: Length::Px(32.0),
            height: Length::Px(24.0),
            fill: Some(Paint::Solid { color: fill }),
            radius: 0.0,
        }
    }

    /// Bakes into a kept temp dir and decodes frame 1, with the comp's
    /// own frame and rate. The directory is kept - promoted out of the
    /// tempdir's delete-on-drop - because a test that reads a later
    /// frame reads it after this returns.
    fn first_frame(comp: &Comp) -> (Baked, tiny_skia::Pixmap) {
        let engine = native();
        let dir = tempfile::tempdir().expect("a temp dir").keep();
        let baked = engine
            .bake(comp, &BakeRequest::default(), &dir)
            .expect("the bake");
        let png = std::fs::read(baked.frame_path(1)).expect("frame 1 on disk");
        let pixmap = tiny_skia::Pixmap::decode_png(&png).expect("a decodable frame");
        (baked, pixmap)
    }

    #[test]
    fn native_engine_renders_vector_lottie_assets() {
        const SOLID_LOTTIE: &str = r##"{
            "v":"5.7.6","fr":30,"ip":0,"op":30,"w":32,"h":32,
            "layers":[{"nm":"Red star","ind":1,"ty":4,"shapes":[{"ty":"gr","it":[
              {"ty":"sr","sy":1,"d":1,"pt":{"a":0,"k":5},"p":{"a":0,"k":[16,16]},
               "r":{"a":0,"k":0},"or":{"a":0,"k":14},"os":{"a":0,"k":0},
               "ir":{"a":0,"k":7},"is":{"a":0,"k":0}},
              {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
              {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},
               "s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
            ]}]}]
        }"##;
let dir = tempfile::tempdir().expect("asset directory");
        std::fs::write(dir.path().join("animation.json"), SOLID_LOTTIE).unwrap();
        let mut comp = comp_of(
            "lottie",
            vec![node(
                "animation",
                NodeKind::Lottie {
                    asset: "animation".into(),
                    r#loop: false,
                },
            )],
        );
        comp.assets.insert(
            "animation".into(),
            Asset {
                src: "animation.json".into(),
                kind: Some(AssetKind::Lottie),
            },
        );
        let output = native()
            .frame(&comp, 0.0, None, dir.path())
            .expect("native Lottie frame");
        let png = std::fs::read(output).expect("rendered PNG");
        let frame = tiny_skia::Pixmap::decode_png(&png).expect("valid PNG frame");
        let centre = frame.pixel(32, 24).expect("canvas centre");
        assert!(centre.red() > 240, "{centre:?}");
        assert!(centre.green() < 10, "{centre:?}");
        assert_eq!(centre.alpha(), 255);
    }

    #[test]
    fn the_probe_is_always_available_with_a_version() {
        let probe = native().probe();
        assert!(probe.available);
        assert!(
            probe
                .version
                .as_deref()
                .unwrap_or_default()
                .contains("concat-motion")
        );
    }

    #[test]
    fn a_rectangle_bakes_over_the_background() {
        let comp = comp_of(
            "r",
            vec![node(
                "box",
                NodeKind::Rect {
                    width: Length::Px(64.0),
                    height: Length::Px(48.0),
                    fill: Some(Paint::Solid {
                        color: Rgba::new(0.0, 1.0, 0.0, 1.0),
                    }),
                    radius: 0.0,
                },
            )],
        );
        let (baked, frame) = first_frame(&comp);
        assert_eq!(baked.frames, 10, "one second at ten frames a second");
        assert_eq!(frame.width(), 64);
        let centre = frame.pixel(32, 24).unwrap();
        assert!(centre.green() > 240, "{centre:?}");
        assert_eq!(centre.alpha(), 255);
    }

    #[test]
    fn an_offscreen_node_leaves_its_span_blank() {
        let mut late = node("late", rect(Rgba::new(1.0, 0.0, 0.0, 1.0)));
        late.start = 0.5;
        let comp = comp_of("late", vec![late]);
        let (baked, frame) = first_frame(&comp);
        assert_eq!(baked.frames, 10);
        // Frame 1 is at t=0, before the node's span: transparent.
        assert_eq!(frame.pixel(32, 24).unwrap().alpha(), 0);
        let png = std::fs::read(baked.frame_path(6)).expect("frame 6 on disk");
        let later = tiny_skia::Pixmap::decode_png(&png).unwrap();
        assert_eq!(later.pixel(32, 24).unwrap().red(), 255);
    }

    #[test]
    fn a_position_places_the_anchor() {
        let mut placed = node("box", rect(Rgba::new(1.0, 1.0, 1.0, 1.0)));
        placed.transform.position = Some(Point { x: 16.0, y: 12.0 });
        let comp = comp_of("p", vec![placed]);
        let (_, frame) = first_frame(&comp);
        // The default anchor is the box's centre, so the box runs
        // 0..32 across and 0..24 down: a 16x12 frame's centre is
        // inside it, the far corner is not.
        assert_eq!(frame.pixel(8, 6).unwrap().alpha(), 255);
        assert_eq!(frame.pixel(48, 36).unwrap().alpha(), 0);
    }

    #[test]
    fn a_rotation_spins_about_the_anchor() {
        let mut spun = node("box", rect(Rgba::new(1.0, 0.0, 0.0, 1.0)));
        spun.transform.rotation = 45.0;
        let comp = comp_of("spin", vec![spun]);
        let (_, frame) = first_frame(&comp);
        // A 32x24 box at the frame's centre, rotated 45 degrees: the
        // centre stays covered, and the old bottom-right corner of the
        // box is now outside it.
        assert_eq!(frame.pixel(32, 24).unwrap().alpha(), 255);
        assert!(frame.pixel(46, 34).unwrap().alpha() < 250);
    }

    #[test]
    fn an_opacity_track_fades_the_node_in() {
        let mut fading = node("box", rect(Rgba::new(1.0, 1.0, 1.0, 1.0)));
        fading.tracks.insert(
            "opacity".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::new(1.0, Value::number(1.0)),
            ]),
        );
        let comp = comp_of("fade", vec![fading]);
        let (baked, _) = first_frame(&comp);
        let read = |index: u32| {
            let png = std::fs::read(baked.frame_path(index)).unwrap();
            let frame = tiny_skia::Pixmap::decode_png(&png).unwrap();
            frame.pixel(32, 24).unwrap().alpha()
        };
        assert_eq!(read(1), 0, "transparent at its start");
        assert!(read(5) > 60 && read(5) < 200, "halfway there");
        // The last frame samples at 0.9s of a 1s span: nine tenths of
        // the way to opaque, which is what a frame at (n-1)/fps is.
        assert!((225..=235).contains(&read(10)), "nine tenths: {}", read(10));
    }

    #[test]
    fn a_parent_transform_carries_its_children() {
        let mut parent = node(
            "group",
            NodeKind::Group {
                children: vec!["child".into()],
            },
        );
        parent.transform.position = Some(Point { x: 32.0, y: 24.0 });
        let mut child = node("child", rect(Rgba::new(0.0, 0.0, 1.0, 1.0)));
        child.parent = Some("group".into());
        child.transform.position = Some(Point { x: 32.0, y: 24.0 });
        let comp = comp_of("nest", vec![parent, child]);
        let (_, frame) = first_frame(&comp);
        // A group's frame hangs off its position: the child's own
        // position (32,24) in the group's frame lands at the group's
        // position plus it in the comp's - (64,48) - with the 32x24
        // box pulled back toward the frame, so the far corner of the
        // box shows and the near corner does not.
        assert_eq!(frame.pixel(60, 40).unwrap().alpha(), 255);
        assert_eq!(frame.pixel(4, 4).unwrap().alpha(), 0);
    }

    #[test]
    fn a_vector_path_fills_and_strokes() {
        let path = node(
            "mark",
            NodeKind::Path {
                commands: vec![
                    crate::comp::PathCommand::MoveTo {
                        point: Point { x: 8.0, y: 8.0 },
                    },
                    crate::comp::PathCommand::LineTo {
                        point: Point { x: 56.0, y: 8.0 },
                    },
                    crate::comp::PathCommand::LineTo {
                        point: Point { x: 8.0, y: 40.0 },
                    },
                    crate::comp::PathCommand::Close,
                ],
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 0.5, 0.0, 1.0),
                }),
                stroke: Some(Stroke {
                    color: Rgba::new(0.0, 0.0, 0.0, 1.0),
                    width: 2.0,
                }),
                dash: None,
                trim: None,
                morph: None,
            },
        );
        let comp = comp_of("mark", vec![path]);
        let (_, frame) = first_frame(&comp);
        // The ink's bounds (8..56 by 8..40) centre on (32,24), so the
        // whole mark shifts +4 from its own pixels: the fill inside,
        // the stroke on the top edge at y=12.
        let inside = frame.pixel(24, 19).unwrap();
        assert!(inside.red() > 200 && inside.green() > 60, "{inside:?}");
        let edge = frame.pixel(30, 12).unwrap();
        assert!(edge.alpha() == 255);
    }

    #[test]
    fn a_trim_cuts_a_strokes_walk() {
        let stroked = |trim: Option<(f64, f64)>| {
            let mut node = node(
                "line",
                NodeKind::Path {
                    commands: vec![
                        crate::comp::PathCommand::MoveTo {
                            point: Point { x: 4.0, y: 24.0 },
                        },
                        crate::comp::PathCommand::LineTo {
                            point: Point { x: 60.0, y: 24.0 },
                        },
                    ],
                    fill: None,
                    stroke: Some(Stroke {
                        color: Rgba::new(1.0, 0.0, 0.0, 1.0),
                        width: 4.0,
                    }),
                    dash: None,
                    trim,
                    morph: None,
                },
            );
            node.tracks.insert(
                "trim".into(),
                Track::new(vec![Key::new(0.0, Value::Point { x: 0.0, y: 0.5 })]),
            );
            comp_of("trim", vec![node])
        };
        let (_, half) = first_frame(&stroked(Some((0.0, 1.0))));
        // The track holds 0..0.5: the left half drawn, the right not.
        // The bounds-anchored box keeps the line on its own y=24.
        assert_eq!(half.pixel(10, 24).unwrap().alpha(), 255);
        assert_eq!(half.pixel(50, 24).unwrap().alpha(), 0);
    }

    #[test]
    fn a_morph_blends_between_two_shapes() {
        let morphing = |t: f64| {
            let mut node = node(
                "morph",
                NodeKind::Path {
                    commands: vec![
                        crate::comp::PathCommand::MoveTo {
                            point: Point { x: 8.0, y: 8.0 },
                        },
                        crate::comp::PathCommand::LineTo {
                            point: Point { x: 56.0, y: 40.0 },
                        },
                    ],
                    fill: None,
                    stroke: Some(Stroke {
                        color: Rgba::new(0.0, 1.0, 0.0, 1.0),
                        width: 3.0,
                    }),
                    dash: None,
                    trim: None,
                    morph: Some(vec![
                        crate::comp::PathCommand::MoveTo {
                            point: Point { x: 8.0, y: 40.0 },
                        },
                        crate::comp::PathCommand::LineTo {
                            point: Point { x: 56.0, y: 8.0 },
                        },
                    ]),
                },
            );
            node.tracks.insert(
                "morph".into(),
                Track::new(vec![Key::new(0.0, Value::number(t))]),
            );
            comp_of("morph", vec![node])
        };
        let (_, start) = first_frame(&morphing(0.0));
        assert!(
            start.pixel(30, 23).unwrap().alpha() > 0,
            "on the first line"
        );
        let (_, end) = first_frame(&morphing(1.0));
        assert!(end.pixel(30, 25).unwrap().alpha() > 0, "on the second line");
    }

    #[test]
    fn a_mask_keeps_only_what_it_covers() {
        // A rectangle mask is the node's own box, so it masks nothing a
        // box-filling rect shows; a path mask smaller than the box is
        // where a mask earns its keep.
        let mut masked = node(
            "box",
            NodeKind::Rect {
                width: Length::Px(64.0),
                height: Length::Px(48.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 0.0, 1.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        masked.mask = Some(Mask {
            shape: MaskShape::Path {
                commands: vec![
                    crate::comp::PathCommand::MoveTo {
                        point: Point { x: 16.0, y: 8.0 },
                    },
                    crate::comp::PathCommand::LineTo {
                        point: Point { x: 32.0, y: 40.0 },
                    },
                    crate::comp::PathCommand::LineTo {
                        point: Point { x: 0.0, y: 40.0 },
                    },
                    crate::comp::PathCommand::Close,
                ],
            },
            radius: 0.0,
            feather: 0.0,
            invert: false,
            expansion: 0.0,
        });
        let comp = comp_of("masked", vec![masked]);
        let (_, frame) = first_frame(&comp);
        // The mask path is in the node's own pixels, and the rect's box
        // is the whole 64x48 frame from (0,0): the triangle keeps the
        // left of the frame, the right is cut away.
        assert_eq!(frame.pixel(16, 30).unwrap().alpha(), 255);
        assert_eq!(frame.pixel(52, 24).unwrap().alpha(), 0);
    }

    #[test]
    fn a_track_matte_stencils_through_its_source() {
        let mut fill = node(
            "fill",
            NodeKind::Rect {
                width: Length::Px(64.0),
                height: Length::Px(48.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(0.0, 1.0, 0.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        fill.matte = Some(crate::comp::Matte {
            source: "stencil".into(),
            mode: crate::comp::MatteMode::Alpha,
        });
        let mut stencil = node(
            "stencil",
            NodeKind::Rect {
                width: Length::Px(16.0),
                height: Length::Px(48.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        stencil.transform.position = Some(Point { x: 16.0, y: 24.0 });
        let comp = comp_of("matte", vec![fill, stencil]);
        let (_, frame) = first_frame(&comp);
        // The fill shows only where the stencil's alpha is: the left
        // quarter, centred on x=16.
        assert_eq!(frame.pixel(12, 24).unwrap().alpha(), 255);
        assert_eq!(frame.pixel(40, 24).unwrap().alpha(), 0);
    }

    #[test]
    fn a_glow_effect_halos_the_node() {
        let mut glowing = node(
            "box",
            NodeKind::Rect {
                width: Length::Px(8.0),
                height: Length::Px(8.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        glowing.effects.push(Effect::Glow {
            radius: 6.0,
            intensity: 1.0,
            color: Rgba::new(0.0, 0.0, 1.0, 1.0),
        });
        let comp = comp_of("glow", vec![glowing]);
        let (_, frame) = first_frame(&comp);
        // Beside the box, the halo: blue where the frame was empty.
        let beside = frame.pixel(30, 14).unwrap();
        assert!(beside.blue() > 8, "{beside:?}");
    }

    #[test]
    fn a_shadow_effect_falls_behind_the_node() {
        let mut shadowed = node(
            "box",
            NodeKind::Rect {
                width: Length::Px(20.0),
                height: Length::Px(20.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        shadowed.effects.push(Effect::Shadow {
            x: 6.0,
            y: 6.0,
            blur: 2.0,
            color: Rgba::new(0.0, 0.0, 0.0, 1.0),
        });
        let comp = comp_of("shadow", vec![shadowed]);
        let (_, frame) = first_frame(&comp);
        // Under the box's bottom-right edge, the shadow's darkness:
        // the pixel there is the shadow over transparency, grey not
        // than empty.
        let under = frame.pixel(40, 32).unwrap();
        assert!(under.alpha() > 10, "{under:?}");
    }

    #[test]
    fn an_adjustment_blurs_what_is_beneath_it() {
        let mut sharp = node(
            "box",
            NodeKind::Rect {
                width: Length::Px(4.0),
                height: Length::Px(4.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 0.0, 0.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        sharp.transform.position = Some(Point { x: 32.0, y: 24.0 });
        let mut grade = node("grade", NodeKind::Adjustment);
        grade.effects.push(Effect::Blur { radius: 4.0 });
        let comp = comp_of("graded", vec![sharp, grade]);
        let (_, frame) = first_frame(&comp);
        // A 4x4 box blurred by 4, twice over: the pixel six away from
        // its edge is no longer empty.
        let spread = frame.pixel(40, 24).unwrap();
        assert!(spread.alpha() > 0, "{spread:?}");
    }

    #[test]
    fn a_blend_multiplies_what_is_under_it() {
        let mut red = node(
            "red",
            NodeKind::Rect {
                width: Length::Px(64.0),
                height: Length::Px(48.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 0.0, 0.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        red.start = 0.0;
        let mut white = node(
            "white",
            NodeKind::Rect {
                width: Length::Px(64.0),
                height: Length::Px(48.0),
                fill: Some(Paint::Solid {
                    color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                }),
                radius: 0.0,
            },
        );
        white.blend = Blend::Multiply;
        white.start = 0.0;
        let comp = comp_of("blend", vec![red, white]);
        let (_, frame) = first_frame(&comp);
        let pixel = frame.pixel(32, 24).unwrap();
        assert!(
            pixel.red() > 200,
            "white multiplied over red keeps red: {pixel:?}"
        );
        assert!(pixel.green() < 30, "{pixel:?}");
    }

    #[test]
    fn text_draws_through_the_shaped_run_path() {
        let mut text = node(
            "words",
            NodeKind::Text(Box::new(TextNode {
                text: "HELLO".into(),
                font: "Bebas Neue".into(),
                size: 40.0,
                weight: 400,
                color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                ..TextNode::default()
            })),
        );
        text.transform.position = Some(Point { x: 32.0, y: 24.0 });
        let comp = comp_of("text", vec![text]);
        let (baked, frame) = first_frame(&comp);
        assert_eq!(baked.warnings.len(), 0, "{:?}", baked.warnings);
        // Ink somewhere near the centre: five glyphs at size 40 in a
        // 64-wide frame will clip, but they will draw.
        let mut ink = 0;
        for y in 0..frame.height() {
            for x in 0..frame.width() {
                if frame.pixel(x, y).unwrap().alpha() > 0 {
                    ink += 1;
                }
            }
        }
        assert!(ink > 40, "{ink} pixels of ink");
    }

    #[test]
    fn a_word_that_has_not_been_spoken_is_held_in_place() {
        let mut text = node(
            "karaoke",
            NodeKind::Text(Box::new(TextNode {
                words: vec![
                    crate::comp::Word {
                        text: "ONE".into(),
                        start: 0.0,
                        end: Some(0.5),
                    },
                    crate::comp::Word {
                        text: "TWO".into(),
                        start: 0.5,
                        end: None,
                    },
                ],
                font: "Bebas Neue".into(),
                size: 30.0,
                color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                ..TextNode::default()
            })),
        );
        text.transform.position = Some(Point { x: 32.0, y: 24.0 });
        let comp = comp_of("karaoke", vec![text]);
        let (baked, _) = first_frame(&comp);
        let read = |index: u32| {
            let png = std::fs::read(baked.frame_path(index)).unwrap();
            let frame = tiny_skia::Pixmap::decode_png(&png).unwrap();
            let mut ink = 0;
            for y in 0..frame.height() {
                for x in 0..frame.width() {
                    if frame.pixel(x, y).unwrap().alpha() > 0 {
                        ink += 1;
                    }
                }
            }
            ink
        };
        let early = read(1);
        let late = read(6);
        assert!(early > 20, "the first word is drawn: {early}");
        assert!(
            late > early + 20,
            "the second word arrived: {early} -> {late}"
        );
    }

    #[test]
    fn a_nested_comp_places_its_own_frame() {
        let mut sub = comp_of(
            "sub",
            vec![node("dot", rect(Rgba::new(0.0, 0.0, 1.0, 1.0)))],
        );
        sub.width = 32;
        sub.height = 24;
        sub.duration = 1.0;
        let mut comp = comp_of(
            "parent",
            vec![node("sting", NodeKind::Composition { comp: "sub".into() })],
        );
        comp.comps.insert("sub".into(), sub);
        let (_, frame) = first_frame(&comp);
        // The sub-comp draws its 32x24 blue frame into the parent's
        // 64x48, at its own frame's size, centre default.
        assert_eq!(frame.pixel(32, 24).unwrap().blue(), 255);
        assert_eq!(frame.pixel(4, 4).unwrap().alpha(), 0);
    }

    #[test]
    fn an_image_asset_is_drawn_from_the_source_dir() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let mut picture = tiny_skia::Pixmap::new(8, 8).unwrap();
        picture.fill(tiny_skia::Color::from_rgba8(255, 128, 0, 255));
        std::fs::write(dir.path().join("logo.png"), picture.encode_png().unwrap()).unwrap();
        let mut comp = comp_of(
            "pic",
            vec![node(
                "logo",
                NodeKind::Image {
                    asset: "logo".into(),
                    fit: Fit::Contain,
                },
            )],
        );
        comp.assets.insert(
            "logo".into(),
            crate::comp::Asset {
                src: "logo.png".into(),
                kind: Some(crate::comp::AssetKind::Image),
            },
        );
        let engine = native();
        let request = BakeRequest {
            source_dir: dir.path().into(),
            ..BakeRequest::default()
        };
        let baked = engine
            .bake(&comp, &request, dir.path().join("out").as_path())
            .expect("the bake");
        assert_eq!(baked.warnings.len(), 0, "{:?}", baked.warnings);
        let png = std::fs::read(baked.frame_path(1)).unwrap();
        let frame = tiny_skia::Pixmap::decode_png(&png).unwrap();
        let pixel = frame.pixel(32, 24).unwrap();
        assert!(pixel.red() > 200 && pixel.green() > 60, "{pixel:?}");
    }

    #[test]
    fn markup_and_lottie_and_shaders_are_refused_with_a_warning_not_a_hole() {
        let mut comp = comp_of(
            "warned",
            vec![
                node(
                    "html",
                    NodeKind::Html {
                        markup: "<div>x</div>".into(),
                        css: String::new(),
                        width: Length::Auto,
                        height: Length::Auto,
                    },
                ),
                node(
                    "lottie",
                    NodeKind::Lottie {
                        asset: "anim".into(),
                        r#loop: false,
                    },
                ),
                {
                    let mut shader = node("shaded", rect(Rgba::new(1.0, 1.0, 1.0, 1.0)));
                    shader.effects.push(Effect::Shader {
                        id: "concat.bloom-pulse".into(),
                        params: Default::default(),
                    });
                    shader
                },
                {
                    let mut tracked = node("tracked", rect(Rgba::new(1.0, 1.0, 1.0, 1.0)));
                    tracked
                        .tracks
                        .insert("wobble".into(), Track::constant(Value::number(1.0)));
                    tracked
                },
            ],
        );
        comp.assets.insert(
            "anim".into(),
            crate::comp::Asset {
                src: "anim.json".into(),
                kind: Some(crate::comp::AssetKind::Lottie),
            },
        );
        let (baked, _) = first_frame(&comp);
        let said = baked.warnings.join("\n");
        assert!(said.contains("markup"), "{said}");
        assert!(said.contains("Lottie"), "{said}");
        assert!(said.contains("shader"), "{said}");
        assert!(said.contains("wobble"), "{said}");
    }

    #[test]
    fn an_invalid_comp_is_refused_before_any_render() {
        let mut comp = comp_of("bad", vec![node("orphan", rect(Rgba::hex("#fff")))]);
        comp.duration = 0.0;
        let engine = native();
        let dir = tempfile::tempdir().expect("a temp dir");
        let error = engine
            .bake(&comp, &BakeRequest::default(), dir.path())
            .expect_err("an invalid comp is refused");
        assert!(matches!(error, MotionError::Invalid { .. }), "{error}");
    }

    #[test]
    fn a_second_bake_of_the_same_comp_is_served_from_the_cache() {
        let comp = comp_of("cached", vec![node("box", rect(Rgba::hex("#0f0")))]);
        let engine = native();
        let dir = tempfile::tempdir().expect("a temp dir");
        let first = engine
            .bake(&comp, &BakeRequest::default(), dir.path())
            .expect("bake one");
        assert!(!first.cached);
        let second = engine
            .bake(&comp, &BakeRequest::default(), dir.path())
            .expect("bake two");
        assert!(second.cached, "the manifest answers");
        assert_eq!(second.frames, first.frames);
        // A changed comp re-renders: the hash is the cache key.
        let mut moved = comp.clone();
        if let NodeKind::Rect { width, .. } = &mut moved.nodes[0].kind {
            *width = Length::Px(16.0);
        }
        let third = engine
            .bake(&moved, &BakeRequest::default(), dir.path())
            .expect("bake three");
        assert!(!third.cached);
    }

    #[test]
    fn two_bakes_of_one_comp_write_identical_frames() {
        let comp = comp_of("same", vec![node("box", rect(Rgba::hex("#00f")))]);
        let engine = native();
        let one = tempfile::tempdir().expect("a temp dir");
        let two = tempfile::tempdir().expect("a temp dir");
        let first = engine
            .bake(&comp, &BakeRequest::default(), one.path())
            .expect("bake one");
        let second = engine
            .bake(&comp, &BakeRequest::default(), two.path())
            .expect("bake two");
        for index in 1..=first.frames.min(second.frames) {
            let a = std::fs::read(first.frame_path(index as u32)).unwrap();
            let b = std::fs::read(second.frame_path(index as u32)).unwrap();
            assert_eq!(a, b, "frame {index} differs between two bakes");
        }
    }

    #[test]
    fn a_bake_at_half_width_renders_the_same_picture_small() {
        let comp = comp_of("half", vec![node("box", rect(Rgba::hex("#f00")))]);
        let engine = native();
        let dir = tempfile::tempdir().expect("a temp dir");
        let request = BakeRequest {
            width: Some(32),
            height: Some(24),
            ..BakeRequest::default()
        };
        let baked = engine.bake(&comp, &request, dir.path()).expect("the bake");
        assert_eq!((baked.width, baked.height), (32, 24));
        let png = std::fs::read(baked.frame_path(1)).unwrap();
        let frame = tiny_skia::Pixmap::decode_png(&png).unwrap();
        assert_eq!(frame.pixel(16, 12).unwrap().red(), 255);
    }

    #[test]
    fn a_ranged_bake_renders_its_shard_without_a_manifest() {
        let comp = comp_of("shard", vec![node("box", rect(Rgba::hex("#fff")))]);
        let engine = native();
        let dir = tempfile::tempdir().expect("a temp dir");
        let request = BakeRequest {
            frames: Some((4, 6)),
            ..BakeRequest::default()
        };
        let baked = engine.bake(&comp, &request, dir.path()).expect("the bake");
        assert_eq!(baked.frames, 3);
        assert!(dir.path().join("frames/0004.png").exists());
        assert!(dir.path().join("frames/0006.png").exists());
        assert!(
            !dir.path().join("frames/0003.png").exists(),
            "the shard renders only its range"
        );
        assert!(
            !dir.path().join("manifest.json").exists(),
            "a shard writes no manifest, which is a whole bake's promise"
        );
    }

    #[test]
    fn a_frame_call_answers_one_png_at_the_asked_width() {
        let comp = comp_of("frame", vec![node("box", rect(Rgba::hex("#ff0")))]);
        let engine = native();
        let out = engine
            .frame(&comp, 0.0, Some(32), Path::new("."))
            .expect("the frame");
        let png = std::fs::read(&out).expect("the png on disk");
        let frame = tiny_skia::Pixmap::decode_png(&png).expect("a decodable frame");
        assert_eq!(frame.width(), 32);
        assert_eq!(frame.height(), 24, "the comp's own aspect");
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn a_motion_blurred_move_smears_rather_than_strobes() {
        let mut moving = node("box", rect(Rgba::new(1.0, 0.0, 0.0, 1.0)));
        moving.motion_blur = Some(crate::comp::MotionBlur {
            samples: 8,
            shutter: 1.0,
        });
        moving.tracks.insert(
            "position".into(),
            Track::new(vec![
                Key::new(0.0, Value::Point { x: 20.0, y: 24.0 }),
                Key::new(1.0, Value::Point { x: 40.0, y: 24.0 }),
            ]),
        );
        let comp = comp_of("smeared", vec![moving.clone()]);
        let (baked, _) = first_frame(&comp);
        let _ = baked;
        // The fifth frame sits mid-comp, t=0.4s, where the shutter's
        // span [0.35, 0.45] is unclamped. The honest comparison is the
        // same move without the blur: the smear paints wider.
        let columns = |comp: &Comp| {
            let (baked, _) = first_frame(comp);
            let png = std::fs::read(baked.frame_path(5)).expect("frame 5 on disk");
            let frame = tiny_skia::Pixmap::decode_png(&png).expect("a decodable frame");
            (0..frame.width())
                .filter(|x| frame.pixel(*x, 24).unwrap().alpha() > 0)
                .count()
        };
        let mut sharp_moving = moving;
        sharp_moving.motion_blur = None;
        let sharp_comp = comp_of("sharp", vec![sharp_moving]);
        let smeared_columns = columns(&comp);
        let sharp_columns = columns(&sharp_comp);
        assert!(
            smeared_columns > sharp_columns,
            "the smear paints {smeared_columns}, the sharp box {sharp_columns}"
        );
        assert!(smeared_columns >= 34, "{smeared_columns} columns painted");
    }
}
