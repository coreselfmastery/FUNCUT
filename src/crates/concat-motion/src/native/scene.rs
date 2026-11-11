// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The walk that turns one comp at one moment into one frame: nodes in
//! paint order under their composed transforms, each through the same
//! layer pipeline - content, mask, matte, effects, motion blur, then
//! the blend and opacity onto what is under it.
//!
//! The semantics here are the document's, which the Geneva sidecar
//! approximates on its flattened timeline: a parent's position, scale
//! and rotation are inherited by its children (a scaled or rotated
//! parent is a divergence Geneva refuses to draw, so the comps that
//! use them arrive here), percentages resolve against the nearest box
//! ancestor and otherwise the comp's frame, and every pixel blends in
//! linear light the way the project's compositor blends.

use std::collections::{HashMap, HashSet};

use tiny_skia::{Pixmap, Transform};

use crate::comp::{
    Blend, Comp, Effect, Fit, Length, Mask, MaskShape, MatteMode, Node, NodeKind, Paint, Point,
    Rgba,
};
use crate::native::images::Pictures;
use crate::native::raster;
use crate::native::shapes;
use crate::native::text;
use crate::track::Value;

/// One comp, one moment, one canvas: everything a frame needs, held
/// across the nodes that share it.
pub(crate) struct Scene<'a> {
    /// The document being drawn.
    pub comp: &'a Comp,
    /// Where the comp's asset paths resolve from.
    pub source_dir: std::path::PathBuf,
    /// The faces a text node is set in.
    pub fonts: &'a concat_text::Fonts,
    /// The stills this bake has decoded, shared across frames.
    pub pictures: &'a mut Pictures,
    /// What could not be drawn as written, named per node.
    pub warnings: &'a mut Vec<String>,
    /// The output canvas, in pixels.
    pub frame: (f64, f64),
    /// The uniform comp-to-output scale.
    pub scale: f64,
}

/// A node's box: its size and where the ink starts, both in output
/// pixels. The anchor fractions read the size; the origin is where the
/// box's top-left sits in the content's own pixels, which is `(0, 0)`
/// for everything the builders draw and the bounds' corner for a path.
type NodeBox = ((f64, f64), (f64, f64));

/// A stroke as the content carries it: colour, width in local pixels,
/// and the dash `(pattern, offset)` when it draws on.
type Pen = (Rgba, f64, Option<(Vec<f64>, f64)>);

/// A node's content, rendered and ready to place.
enum Content {
    /// A shape or path, in its own pixels: the path, what fills it,
    /// what strokes it, and whether a missing fill means white (the
    /// shapes' default) or nothing (a stroke-only path).
    Shape {
        /// The outline, local pixels.
        path: tiny_skia::Path,
        /// The fill, when it has one.
        fill: Option<Paint>,
        /// The stroke, when it has one: colour, width in local pixels,
        /// and the dash `(pattern, offset)` when it draws on.
        stroke: Option<Pen>,
        /// Whether the shape fills with white when its paint is `None`:
        /// true for rectangles and ellipses, false for a path, which is
        /// an outline first.
        default_fill: bool,
    },
    /// Text, as the passes that composite it.
    Text(Box<text::TextPasses>),
    /// A picture, blitted through a fit transform.
    Image {
        /// The picture, at its own pixels.
        pixmap: Pixmap,
        /// Maps picture pixels onto the node's box, which is the frame
        /// the comp draws: the placement the sequence path has always
        /// given a picture.
        fit: Transform,
    },
    /// A sub-comp, rendered at its own frame.
    Composition(Pixmap),
    /// Nothing of its own: a group, or a kind this engine declines.
    Nothing,
}

impl Scene<'_> {
    /// The frame at `at` seconds: background, then every node in paint
    /// order through the layer pipeline.
    ///
    /// Two passes, because a track matte reads a source that may sit
    /// anywhere in paint order: the first walk renders every matte
    /// source aside - and, while it is at it, remembers every node's
    /// content and world so the second walk places rather than
    /// re-renders.
    pub fn draw(&mut self, at: f64) -> Pixmap {
        let mut frame = Pixmap::new(self.frame.0 as u32, self.frame.1 as u32)
            .unwrap_or_else(|| Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates"));
        if self.comp.background.a > 0.0 {
            frame.fill(shapes::color_skia(self.comp.background));
        }
        // A matte source draws nothing of its own while it is a matte;
        // its pixels are rendered aside and read as a stencil.
        let matte_sources: HashSet<String> = self
            .comp
            .nodes
            .iter()
            .filter_map(|node| node.matte.as_ref().map(|matte| matte.source.clone()))
            .collect();
        // Whose box someone else reads: a node with children places
        // them through its own transform, so its box has to be known
        // even when it draws nothing itself.
        let parents: HashSet<&str> = self
            .comp
            .nodes
            .iter()
            .filter_map(|node| node.parent.as_deref())
            .collect();
        let mut stencils: HashMap<String, Pixmap> = HashMap::new();
        let mut worlds: HashMap<String, Transform> = HashMap::new();
        let mut rendered: HashMap<String, Content> = HashMap::new();
        let mut boxes: HashMap<String, NodeBox> = HashMap::new();
        for node in self.comp.paint_order() {
            let parent_world = node
                .parent
                .as_deref()
                .and_then(|parent| worlds.get(parent))
                .copied()
                .unwrap_or(Transform::identity());
            let needs_content =
                parents.contains(node.id.as_str()) || matte_sources.contains(&node.id);
            if needs_content || node.covers(at, self.comp.duration) {
                let content = self.content_of(node, at, 0);
                let box_of = self.box_of(node, &content);
                let reference = node_transform_reference(self, node);
                let world =
                    parent_world.pre_concat(self.local_transform(node, at, reference, box_of));
                rendered.insert(node.id.clone(), content);
                boxes.insert(node.id.clone(), box_of);
                worlds.insert(node.id.clone(), world);
                if matte_sources.contains(&node.id) {
                    let content = rendered.remove(&node.id);
                    if let Some(stencil) = self.node_layer_at(
                        node,
                        at,
                        parent_world,
                        content,
                        box_of,
                        &HashMap::new(),
                        &mut rendered,
                        0,
                    ) {
                        stencils.insert(node.id.clone(), stencil);
                    }
                }
            } else {
                // Nothing places it and nothing hangs off it: the world
                // it would have had is read by no one.
                worlds.insert(node.id.clone(), parent_world);
            }
        }
        for node in self.comp.paint_order() {
            if matte_sources.contains(&node.id) {
                continue;
            }
            if !node.covers(at, self.comp.duration) {
                continue;
            }
            if matches!(node.kind, NodeKind::Adjustment) {
                self.apply_adjustment(node, at, &mut frame);
                continue;
            }
            let parent_world = node
                .parent
                .as_deref()
                .and_then(|parent| worlds.get(parent))
                .copied()
                .unwrap_or(Transform::identity());
            let content = rendered.remove(&node.id);
            let box_of = boxes
                .get(&node.id)
                .copied()
                .unwrap_or((self.frame, (0.0, 0.0)));
            self.place(
                node,
                at,
                parent_world,
                content,
                box_of,
                &mut frame,
                &stencils,
                &mut rendered,
                0,
            );
        }
        frame
    }

    /// One node onto a target: content into a layer, mask, matte,
    /// effects and motion blur through it, then the composite. The
    /// content arrives rendered when the walk already had it.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        node: &Node,
        at: f64,
        parent_world: Transform,
        content: Option<Content>,
        box_of: NodeBox,
        target: &mut Pixmap,
        stencils: &HashMap<String, Pixmap>,
        rendered: &mut HashMap<String, Content>,
        depth: usize,
    ) {
        // Motion blur wraps everything else the node does: the whole
        // subtree, sampled across the shutter and averaged.
        if let Some(blur) = &node.motion_blur {
            let samples = blur.samples.clamp(2, 64) as usize;
            let half = (blur.shutter.clamp(0.0, 2.0) / self.comp.fps.max(1e-6)) / 2.0;
            let mut sum: Option<Vec<f32>> = None;
            let mut size = (self.frame.0.max(1.0) as u32, self.frame.1.max(1.0) as u32);
            for sample in 0..samples {
                let t = at + half * (sample as f64 / (samples - 1) as f64 * 2.0 - 1.0);
                let Some(layer) = self.node_layer_at(
                    node,
                    t,
                    parent_world,
                    None,
                    box_of,
                    stencils,
                    rendered,
                    depth,
                ) else {
                    continue;
                };
                size = (layer.width(), layer.height());
                match sum.as_mut() {
                    None => {
                        sum = Some(layer.data().iter().map(|byte| f32::from(*byte)).collect());
                    }
                    Some(accumulated) => {
                        for (into, from) in accumulated.iter_mut().zip(layer.data()) {
                            *into += f32::from(*from);
                        }
                    }
                }
            }
            if let Some(accumulated) = sum {
                let averaged: Vec<u8> = accumulated
                    .iter()
                    .map(|value| (value / samples as f32).round().clamp(0.0, 255.0) as u8)
                    .collect();
                if let Some(size) = tiny_skia::IntSize::from_wh(size.0, size.1)
                    && let Some(layer) = Pixmap::from_vec(averaged, size)
                {
                    let opacity = node_opacity(node, at, self.comp.duration);
                    raster::composite(target, &layer, node.blend, opacity as f32);
                }
            }
            return;
        }
        if let Some(layer) = self.node_layer_at(
            node,
            at,
            parent_world,
            content,
            box_of,
            stencils,
            rendered,
            depth,
        ) {
            let opacity = node_opacity(node, at, self.comp.duration);
            raster::composite(target, &layer, node.blend, opacity as f32);
        }
    }

    /// The node's layer with its own transform folded in but the
    /// composite held back: what a matte source reads, what a motion
    /// blur sample is, and the body of [`Scene::place`]. The content
    /// arrives rendered when the walk already had it, and is drawn
    /// once - a text node's passes cost two renders a frame, not four.
    #[allow(clippy::too_many_arguments)]
    fn node_layer_at(
        &mut self,
        node: &Node,
        at: f64,
        parent_world: Transform,
        content: Option<Content>,
        box_of: NodeBox,
        stencils: &HashMap<String, Pixmap>,
        rendered: &mut HashMap<String, Content>,
        depth: usize,
    ) -> Option<Pixmap> {
        if depth > 8 {
            return None;
        }
        self.warn_unknown_tracks(node);
        let reference = node_transform_reference(self, node);
        let (content, box_of) = match content {
            Some(content) => (content, box_of),
            None => match rendered.remove(&node.id) {
                Some(content) => (content, box_of),
                None => {
                    let content = self.content_of(node, at, depth);
                    let box_of = self.box_of(node, &content);
                    (content, box_of)
                }
            },
        };
        let world = parent_world.pre_concat(self.local_transform(node, at, reference, box_of));
        self.layer_from(node, at, world, box_of, content, stencils, rendered, depth)
    }

    /// The drawn, masked, matted and effected layer, from a world
    /// transform, the node's box, and the content already rendered.
    #[allow(clippy::too_many_arguments)]
    fn layer_from(
        &mut self,
        node: &Node,
        at: f64,
        world: Transform,
        box_of: NodeBox,
        content: Content,
        stencils: &HashMap<String, Pixmap>,
        rendered: &mut HashMap<String, Content>,
        depth: usize,
    ) -> Option<Pixmap> {
        let box_size = box_of.0;
        let mut layer = Pixmap::new(self.frame.0 as u32, self.frame.1 as u32)?;
        match content {
            Content::Shape {
                path,
                fill,
                stroke,
                default_fill,
            } => {
                self.draw_shape(
                    &mut layer,
                    &path,
                    &world,
                    fill.as_ref(),
                    stroke.as_ref(),
                    default_fill,
                );
            }
            Content::Text(passes) => {
                // The canvas the text was rendered into maps onto the
                // node's box: its block centre is the box's centre.
                let to_box = Transform::from_translate(
                    (box_size.0 / 2.0 - passes.centre.0) as f32,
                    (box_size.1 / 2.0 - passes.centre.1) as f32,
                );
                let placed = world.pre_concat(to_box);
                if let Some(plate) = &passes.plate {
                    raster::blit(&mut layer, plate, placed, Blend::Normal, 1.0);
                }
                if let Some(shadow) = &passes.shadow {
                    raster::blit(&mut layer, shadow, placed, Blend::Normal, 1.0);
                }
                raster::blit(&mut layer, &passes.glyphs, placed, Blend::Normal, 1.0);
            }
            Content::Image { pixmap, fit } => {
                raster::blit(
                    &mut layer,
                    &pixmap,
                    world.pre_concat(fit),
                    Blend::Normal,
                    1.0,
                );
            }
            Content::Composition(rendered) => {
                let (sub_w, sub_h) = (f64::from(rendered.width()), f64::from(rendered.height()));
                let to_box = if sub_w > 0.0 && sub_h > 0.0 {
                    Transform::from_scale((box_size.0 / sub_w) as f32, (box_size.1 / sub_h) as f32)
                } else {
                    Transform::identity()
                };
                raster::blit(
                    &mut layer,
                    &rendered,
                    world.pre_concat(to_box),
                    Blend::Normal,
                    1.0,
                );
            }
            Content::Nothing => {
                // A group: its children, into the same layer, so the
                // group's blend and opacity take them as one.
                let children: Vec<&Node> = self
                    .comp
                    .nodes
                    .iter()
                    .filter(|child| child.parent.as_deref() == Some(node.id.as_str()))
                    .collect();
                for child in children {
                    if !child.covers(at, self.comp.duration) {
                        continue;
                    }
                    if !rendered.contains_key(&child.id) {
                        let content = self.content_of(child, at, depth + 1);
                        rendered.insert(child.id.clone(), content);
                    }
                    let (child_box, child_content) = match rendered.get(&child.id) {
                        Some(content) => (self.box_of(child, content), rendered.remove(&child.id)),
                        None => (((0.0, 0.0), (0.0, 0.0)), None),
                    };
                    self.place(
                        child,
                        at,
                        world,
                        child_content,
                        child_box,
                        &mut layer,
                        stencils,
                        rendered,
                        depth + 1,
                    );
                }
            }
        }
        // A mask in the node's own box: built under the same world.
        if let Some(mask) = &node.mask
            && let Some(stencil) = self.mask_stencil(node, mask, &world, box_of)
        {
            apply_stencil(&mut layer, &stencil);
        }
        // A track matte: the source's pixels, read as alpha or luma.
        if let Some(matte) = &node.matte {
            match stencils.get(matte.source.as_str()) {
                Some(source) => apply_matte(&mut layer, source, matte.mode),
                None => self.warnings.push(format!(
                    "node {:?}: its track matte source {:?} is not on the comp, so the node \
                     draws unmatted",
                    node.id, matte.source
                )),
            }
        }
        // Effects, in the order the document lists them.
        let under = self.apply_effects(node, at, &mut layer, box_size, world);
        if let Some(under) = under {
            let mut together = Pixmap::new(layer.width(), layer.height())?;
            raster::composite(&mut together, &under, Blend::Normal, 1.0);
            raster::composite(&mut together, &layer, Blend::Normal, 1.0);
            return Some(together);
        }
        Some(layer)
    }

    /// The node's effects over a layer: the blur on the layer itself,
    /// the shadow and glow as halos under it, the aberration as a
    /// channel split, the shader as a named refusal. Returns the
    /// underlay, when the effects built one.
    fn apply_effects(
        &mut self,
        node: &Node,
        at: f64,
        layer: &mut Pixmap,
        box_size: (f64, f64),
        world: Transform,
    ) -> Option<Pixmap> {
        let mut under: Option<Pixmap> = None;
        for effect in &node.effects {
            match effect {
                Effect::Blur { radius } => {
                    let tracked = node.property(
                        "blur",
                        at,
                        self.comp.duration,
                        Value::Number { value: *radius },
                    );
                    let radius = match tracked {
                        Value::Number { value } => value,
                        _ => *radius,
                    };
                    raster::blur(layer, radius * self.scale);
                }
                Effect::Shadow { x, y, blur, color } => {
                    let mut halo = raster::tinted_alpha(layer, *color);
                    raster::blur(&mut halo, *blur * self.scale);
                    let under_layer = under.get_or_insert_with(|| {
                        Pixmap::new(layer.width(), layer.height()).unwrap_or_else(|| {
                            Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates")
                        })
                    });
                    raster::blit(
                        under_layer,
                        &halo,
                        Transform::from_translate(
                            *x as f32 * self.scale as f32,
                            *y as f32 * self.scale as f32,
                        ),
                        Blend::Normal,
                        1.0,
                    );
                }
                Effect::Glow {
                    radius,
                    intensity,
                    color,
                } => {
                    let mut halo = raster::tinted_alpha(layer, *color);
                    raster::blur(&mut halo, *radius * self.scale);
                    let strength = intensity.clamp(0.0, 8.0) as f32;
                    let under_layer = under.get_or_insert_with(|| {
                        Pixmap::new(layer.width(), layer.height()).unwrap_or_else(|| {
                            Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates")
                        })
                    });
                    raster::composite(under_layer, &halo, Blend::Screen, strength);
                }
                Effect::Aberration { amount } => {
                    let mut centre = tiny_skia::Point {
                        x: (box_size.0 / 2.0) as f32,
                        y: (box_size.1 / 2.0) as f32,
                    };
                    world.map_point(&mut centre);
                    raster::aberrate(
                        layer,
                        amount * self.scale,
                        (f64::from(centre.x), f64::from(centre.y)),
                    );
                }
                Effect::Shader { id, .. } => {
                    self.warnings.push(format!(
                        "node {:?}: shader {id:?} is a project package the native engine does \
                         not draw; the node renders without it",
                        node.id
                    ));
                }
            }
        }
        under
    }

    /// An adjustment layer: its effects, over everything painted
    /// beneath it in this comp, in place.
    fn apply_adjustment(&mut self, node: &Node, at: f64, target: &mut Pixmap) {
        let mut graded = target.clone();
        let under = self.apply_effects(node, at, &mut graded, self.frame, Transform::identity());
        if let Some(under) = under {
            let mut together = Pixmap::new(graded.width(), graded.height())
                .unwrap_or_else(|| Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates"));
            raster::composite(&mut together, &under, Blend::Normal, 1.0);
            raster::composite(&mut together, &graded, Blend::Normal, 1.0);
            graded = together;
        }
        *target = graded;
    }

    /// Says which tracks this engine does not draw, once per node: the
    /// names neither the placement nor any effect reads.
    fn warn_unknown_tracks(&mut self, node: &Node) {
        for name in node.tracks.keys() {
            let known = matches!(
                name.as_str(),
                "position" | "scale" | "rotation" | "opacity" | "blur" | "dash" | "trim" | "morph"
            );
            if known {
                continue;
            }
            if name.starts_with("param:") {
                self.warnings.push(format!(
                    "node {:?}: the {name:?} track drives a shader package, which the native \
                     engine does not draw",
                    node.id
                ));
            } else {
                self.warnings.push(format!(
                    "node {:?}: the {name:?} track is not a property the native engine draws",
                    node.id
                ));
            }
        }
    }

    /// What a node draws, before any placing.
    fn content_of(&mut self, node: &Node, at: f64, depth: usize) -> Content {
        match &node.kind {
            NodeKind::Group { .. } | NodeKind::Adjustment => Content::Nothing,
            NodeKind::Rect {
                width,
                height,
                fill,
                radius,
            } => {
                let (reference, _) = self.reference_of(node);
                let w = shapes::resolve_px(*width, reference.0, self.scale, 0.0);
                let h = shapes::resolve_px(*height, reference.1, self.scale, 0.0);
                match shapes::rounded_rect(w, h, radius * self.scale) {
                    Some(path) => Content::Shape {
                        path,
                        fill: fill.clone(),
                        stroke: None,
                        default_fill: true,
                    },
                    None => {
                        self.warnings
                            .push(format!("node {:?}: its rectangle is degenerate", node.id));
                        Content::Nothing
                    }
                }
            }
            NodeKind::Ellipse {
                width,
                height,
                fill,
            } => {
                let (reference, _) = self.reference_of(node);
                let w = shapes::resolve_px(*width, reference.0, self.scale, 0.0);
                let h = shapes::resolve_px(*height, reference.1, self.scale, 0.0);
                match shapes::oval(w, h) {
                    Some(path) => Content::Shape {
                        path,
                        fill: fill.clone(),
                        stroke: None,
                        default_fill: true,
                    },
                    None => {
                        self.warnings
                            .push(format!("node {:?}: its ellipse is degenerate", node.id));
                        Content::Nothing
                    }
                }
            }
            NodeKind::Path {
                commands,
                fill,
                stroke,
                dash,
                trim,
                morph,
            } => {
                let mut commands = commands.clone();
                if let Some(target) = morph
                    && t_of(node, "morph", at, self.comp.duration) > 0.0
                {
                    commands = shapes::morphed(
                        &commands,
                        target,
                        t_of(node, "morph", at, self.comp.duration),
                    );
                }
                // A trim track moves the window the field opens: either
                // one is enough, and the track's value wins.
                if trim.is_some() || node.tracks.contains_key("trim") {
                    let (fallback_from, fallback_to) = trim.unwrap_or((0.0, 1.0));
                    let (from, to) = match node.property(
                        "trim",
                        at,
                        self.comp.duration,
                        Value::Point {
                            x: fallback_from,
                            y: fallback_to,
                        },
                    ) {
                        Value::Point { x, y } => (x, y),
                        _ => (fallback_from, fallback_to),
                    };
                    commands = shapes::trimmed(&commands, from, to);
                }
                let dashing = dash.map(|(on, off)| {
                    let offset = t_of(node, "dash", at, self.comp.duration);
                    (vec![on * self.scale, off * self.scale], offset * self.scale)
                });
                match shapes::path_of(&commands) {
                    Some(path) => Content::Shape {
                        path,
                        fill: fill.clone(),
                        stroke: stroke
                            .as_ref()
                            .map(|stroke| (stroke.color, stroke.width * self.scale, dashing)),
                        default_fill: false,
                    },
                    None => {
                        self.warnings.push(format!(
                            "node {:?}: its path has no commands that draw",
                            node.id
                        ));
                        Content::Nothing
                    }
                }
            }
            NodeKind::Text(inner) => Content::Text(Box::new(text::passes(
                node, inner, at, self.frame, self.scale, self.fonts,
            ))),
            NodeKind::Image { asset, fit } => match self.picture_of(node, asset) {
                Some(pixmap) => {
                    let (pw, ph) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
                    let (fw, fh) = self.frame;
                    let fit_transform = match fit {
                        Fit::None => Transform::identity(),
                        Fit::Fill => Transform::from_scale((fw / pw) as f32, (fh / ph) as f32),
                        Fit::Contain | Fit::Cover => {
                            let (sx, sy) = (fw / pw, fh / ph);
                            let pick = if matches!(fit, Fit::Contain) {
                                sx.min(sy)
                            } else {
                                sx.max(sy)
                            };
                            let overflow_x = (pw * pick - fw) / 2.0;
                            let overflow_y = (ph * pick - fh) / 2.0;
                            Transform::from_translate(-overflow_x as f32, -overflow_y as f32)
                                .pre_concat(Transform::from_scale(pick as f32, pick as f32))
                        }
                    };
                    Content::Image {
                        pixmap,
                        fit: fit_transform,
                    }
                }
                None => Content::Nothing,
            },
            NodeKind::Lottie { asset, r#loop } => {
                let source = self.comp.assets.get(asset).map(|found| found.src.clone());
                match source {
                    Some(source) => {
                        let path = self.source_dir.join(&source);
                        match self.pictures.lottie_frame(
                            &path,
                            (at - node.start).max(0.0),
                            *r#loop,
                        ) {
                            Ok(pixmap) => {
                                let (pw, ph) =
                                    (f64::from(pixmap.width()), f64::from(pixmap.height()));
                                let (fw, fh) = self.frame;
                                let scale = (fw / pw).min(fh / ph);
                                let fit = Transform::from_translate(
                                    ((fw - pw * scale) / 2.0) as f32,
                                    ((fh - ph * scale) / 2.0) as f32,
                                )
                                .pre_concat(Transform::from_scale(scale as f32, scale as f32));
                                Content::Image { pixmap, fit }
                            }
                            Err(error) => {
                                self.warnings.push(format!(
                                    "node {:?}: Lottie asset {:?} could not be rendered: {error}",
                                    node.id, source
                                ));
                                Content::Nothing
                            }
                        }
                    }
                    None => {
                        self.warnings.push(format!(
                            "node {:?}: Lottie asset {asset:?} is missing from the comp asset table",
                            node.id
                        ));
                        Content::Nothing
                    }
                }
            }
            NodeKind::Html { .. } => {
                self.warnings.push(format!(
                    "node {:?}: markup is drawn by the Geneva engine; the native engine draws \
                     the rest of the comp and leaves this node out",
                    node.id
                ));
                Content::Nothing
            }
            NodeKind::Composition { comp } => match self.comp.comps.get(comp) {
                Some(sub) if depth < 8 => {
                    let mut sub_scene = Scene {
                        comp: sub,
                        source_dir: self.source_dir.clone(),
                        fonts: self.fonts,
                        pictures: self.pictures,
                        warnings: self.warnings,
                        frame: (
                            f64::from(sub.width) * self.scale,
                            f64::from(sub.height) * self.scale,
                        ),
                        scale: self.scale,
                    };
                    Content::Composition(sub_scene.draw(at))
                }
                Some(_) => {
                    self.warnings.push(format!(
                        "node {:?}: the composition {comp:?} sits deeper than the engine's \
                         eight levels of nesting",
                        node.id
                    ));
                    Content::Nothing
                }
                None => {
                    self.warnings.push(format!(
                        "node {:?}: the composition {comp:?} is not one this comp carries",
                        node.id
                    ));
                    Content::Nothing
                }
            },
        }
    }

    /// The picture an image node names, decoded from the comp's assets.
    fn picture_of(&mut self, node: &Node, asset: &str) -> Option<Pixmap> {
        let found = self.comp.assets.get(asset)?;
        let path = self.source_dir.join(&found.src);
        match self.pictures.picture(&path) {
            Some(pixmap) => Some(pixmap.clone()),
            None => {
                self.warnings.push(format!(
                    "node {:?}: the image asset {:?} ({}) could not be read as a still",
                    node.id,
                    found.src,
                    path.display()
                ));
                None
            }
        }
    }

    /// The box a node occupies, in output pixels: what its anchor
    /// fractions read against and what a mask is inscribed in. The
    /// origin is where the box's top-left sits in the content's own
    /// pixels - `(0, 0)` for everything the builders draw from the
    /// corner, the tight bounds' corner for a path. A group has no box
    /// of its own: its transform is its position, rotation and scale,
    /// and its children are placed from its origin.
    fn box_of(&self, node: &Node, content: &Content) -> NodeBox {
        match (&node.kind, content) {
            (NodeKind::Group { .. }, _) | (NodeKind::Adjustment, _) => ((0.0, 0.0), (0.0, 0.0)),
            (NodeKind::Rect { width, height, .. }, _)
            | (NodeKind::Ellipse { width, height, .. }, _) => {
                let (reference, _) = self.reference_of(node);
                (
                    (
                        shapes::resolve_px(*width, reference.0, self.scale, 0.0),
                        shapes::resolve_px(*height, reference.1, self.scale, 0.0),
                    ),
                    (0.0, 0.0),
                )
            }
            (NodeKind::Path { commands, .. }, _) => {
                let bounds = shapes::path_of(commands).and_then(|path| path.compute_tight_bounds());
                match bounds {
                    Some(rect) => (
                        (
                            (f64::from(rect.right()) - f64::from(rect.left())).max(0.0)
                                * self.scale,
                            (f64::from(rect.bottom()) - f64::from(rect.top())).max(0.0)
                                * self.scale,
                        ),
                        (
                            f64::from(rect.left()) * self.scale,
                            f64::from(rect.top()) * self.scale,
                        ),
                    ),
                    None => ((0.0, 0.0), (0.0, 0.0)),
                }
            }
            (_, Content::Text(passes)) => (passes.block, (0.0, 0.0)),
            (_, Content::Image { .. }) => (self.frame, (0.0, 0.0)),
            (NodeKind::Composition { comp }, _) => {
                let sub = self.comp.comps.get(comp);
                (
                    (
                        sub.map(|sub| f64::from(sub.width))
                            .unwrap_or(self.frame.0 / self.scale.max(1e-9))
                            * self.scale,
                        sub.map(|sub| f64::from(sub.height))
                            .unwrap_or(self.frame.1 / self.scale.max(1e-9))
                            * self.scale,
                    ),
                    (0.0, 0.0),
                )
            }
            _ => (self.frame, (0.0, 0.0)),
        }
    }

    /// The frame a node's lengths and position resolve against: its
    /// nearest box ancestor's box, the comp's frame otherwise. In comp
    /// pixels.
    fn reference_of(&self, node: &Node) -> ((f64, f64), bool) {
        let mut parent = node.parent.as_deref();
        let mut depth = 0;
        while let Some(id) = parent {
            depth += 1;
            if depth > 32 {
                break;
            }
            match self.comp.node(id) {
                Some(found) => match &found.kind {
                    NodeKind::Rect { width, height, .. }
                    | NodeKind::Ellipse { width, height, .. } => {
                        return (
                            (
                                resolve_reference(*width, f64::from(self.comp.width)),
                                resolve_reference(*height, f64::from(self.comp.height)),
                            ),
                            true,
                        );
                    }
                    _ => parent = found.parent.as_deref(),
                },
                None => break,
            }
        }
        (
            (f64::from(self.comp.width), f64::from(self.comp.height)),
            false,
        )
    }

    /// The node's own placement: the anchor point of its box at its
    /// position, rotated and scaled about it. A group has no box, so
    /// its anchor is its own origin and its default position is the
    /// parent's origin rather than the parent's centre - a group with
    /// nothing set leaves its children exactly where they were, which
    /// is what a grouping is for.
    fn local_transform(
        &self,
        node: &Node,
        at: f64,
        reference: (f64, f64),
        box_of: NodeBox,
    ) -> Transform {
        let model = &node.transform;
        let grouped = matches!(node.kind, NodeKind::Group { .. } | NodeKind::Adjustment);
        let fallback = if grouped {
            Point { x: 0.0, y: 0.0 }
        } else {
            Point {
                x: reference.0 / 2.0,
                y: reference.1 / 2.0,
            }
        };
        let (mut px, mut py) = match model.position {
            Some(point) => (point.x, point.y),
            None => (fallback.x, fallback.y),
        };
        let mut scale = model.scale;
        let mut rotation = model.rotation;
        let fraction = node.fraction(at, self.comp.duration);
        if let Some(track) = node.tracks.get("position")
            && let Some(Value::Point { x, y }) = track.sample(fraction)
        {
            px = x;
            py = y;
        }
        if let Some(track) = node.tracks.get("scale")
            && let Some(Value::Number { value }) = track.sample(fraction)
        {
            scale = value;
        }
        if let Some(track) = node.tracks.get("rotation")
            && let Some(Value::Number { value }) = track.sample(fraction)
        {
            rotation = value;
        }
        let anchor = model.anchor.unwrap_or(Point { x: 0.5, y: 0.5 });
        let (ax, ay) = (
            box_of.1.0 + anchor.x * box_of.0.0,
            box_of.1.1 + anchor.y * box_of.0.1,
        );
        let place =
            Transform::from_translate(px as f32 * self.scale as f32, py as f32 * self.scale as f32);
        // Degrees: tiny-skia's `from_rotate` converts from degrees
        // itself, clockwise on the screen.
        let spin = Transform::from_rotate(rotation as f32);
        let grow = Transform::from_scale(scale as f32, scale as f32);
        let to_anchor = Transform::from_translate(-ax as f32, -ay as f32);
        place
            .pre_concat(spin)
            .pre_concat(grow)
            .pre_concat(to_anchor)
    }

    /// The stencil of a mask: the shape in the node's own box, expanded
    /// or shrunk, feathered, inverted.
    fn mask_stencil(
        &mut self,
        node: &Node,
        mask: &Mask,
        world: &Transform,
        box_of: NodeBox,
    ) -> Option<Pixmap> {
        let box_size = box_of.0;
        let mut stencil = Pixmap::new(self.frame.0 as u32, self.frame.1 as u32)?;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        paint.anti_alias = true;
        let shape = match &mask.shape {
            MaskShape::Rect => {
                shapes::rounded_rect(box_size.0, box_size.1, mask.radius * self.scale)?
            }
            MaskShape::Ellipse => shapes::oval(box_size.0, box_size.1)?,
            MaskShape::Path { commands } => shapes::path_of(commands)?,
        };
        // The mask sits in the node's own box, ink included: a path
        // whose commands start away from the origin masks from there.
        let world = world.pre_concat(Transform::from_translate(
            box_of.1.0 as f32,
            box_of.1.1 as f32,
        ));
        if mask.expansion > 0.01 {
            // Grow: the shape plus a stroke around it, together.
            let pen = tiny_skia::Stroke {
                width: (mask.expansion * 2.0 * self.scale) as f32,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..tiny_skia::Stroke::default()
            };
            stencil
                .as_mut()
                .stroke_path(&shape, &paint, &pen, world, None);
        }
        stencil
            .as_mut()
            .fill_path(&shape, &paint, tiny_skia::FillRule::Winding, world, None);
        if mask.expansion < -0.01 {
            // Shrink: a stroke around the shape, taken away from it.
            let mut grown = Pixmap::new(self.frame.0 as u32, self.frame.1 as u32)?;
            let pen = tiny_skia::Stroke {
                width: (-mask.expansion * 2.0 * self.scale) as f32,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..tiny_skia::Stroke::default()
            };
            grown
                .as_mut()
                .stroke_path(&shape, &paint, &pen, world, None);
            let carved = stencil.data_mut();
            for (fill, cut) in carved.chunks_exact_mut(4).zip(grown.data().chunks_exact(4)) {
                let keep = u16::from(fill[3]).saturating_sub(u16::from(cut[3]));
                fill[0] = ((u16::from(fill[0]) * keep + 127) / 255) as u8;
                fill[1] = ((u16::from(fill[1]) * keep + 127) / 255) as u8;
                fill[2] = ((u16::from(fill[2]) * keep + 127) / 255) as u8;
                fill[3] = keep.clamp(0, 255) as u8;
            }
        }
        if mask.feather > 0.0 {
            raster::blur(&mut stencil, mask.feather * self.scale);
        }
        if mask.invert {
            for pixel in stencil.data_mut().chunks_exact_mut(4) {
                let alpha = 255 - pixel[3];
                pixel[0] = ((u16::from(pixel[0]) * u16::from(alpha) + 127) / 255) as u8;
                pixel[1] = ((u16::from(pixel[1]) * u16::from(alpha) + 127) / 255) as u8;
                pixel[2] = ((u16::from(pixel[2]) * u16::from(alpha) + 127) / 255) as u8;
                pixel[3] = alpha;
            }
        }
        let _ = node;
        Some(stencil)
    }

    /// Draws a shape into a layer: the fill, then the stroke over it.
    fn draw_shape(
        &mut self,
        layer: &mut Pixmap,
        path: &tiny_skia::Path,
        world: &Transform,
        fill: Option<&Paint>,
        stroke: Option<&Pen>,
        default_fill: bool,
    ) {
        match fill {
            Some(fill) => {
                if let Some(shader) = shapes::shader_of(fill, self.frame.0, self.frame.1) {
                    let paint = tiny_skia::Paint {
                        shader,
                        anti_alias: true,
                        ..tiny_skia::Paint::default()
                    };
                    layer.as_mut().fill_path(
                        path,
                        &paint,
                        tiny_skia::FillRule::Winding,
                        *world,
                        None,
                    );
                }
            }
            None if default_fill => {
                // A rectangle or an ellipse without a paint is white,
                // the default the document names.
                let mut paint = tiny_skia::Paint::default();
                paint.set_color(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
                paint.anti_alias = true;
                layer
                    .as_mut()
                    .fill_path(path, &paint, tiny_skia::FillRule::Winding, *world, None);
            }
            None => {}
        }
        if let Some((color, width, dash)) = stroke {
            let paint = tiny_skia::Paint {
                shader: tiny_skia::Shader::SolidColor(shapes::color_skia(*color)),
                anti_alias: true,
                ..tiny_skia::Paint::default()
            };
            // A motion-graphics stroke rounds where it turns and where
            // it ends: an underline that ends square looks cut, not
            // drawn.
            let mut pen = tiny_skia::Stroke {
                width: *width as f32,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..tiny_skia::Stroke::default()
            };
            if let Some((array, offset)) = dash
                && array.len() >= 2
                && array.iter().all(|value| *value >= 0.0)
            {
                let pattern: Vec<f32> = array.iter().map(|value| *value as f32).collect();
                pen.dash = tiny_skia::StrokeDash::new(pattern, *offset as f32);
            }
            layer.as_mut().stroke_path(path, &paint, &pen, *world, None);
        }
    }
}

/// A node's opacity, from its field and its track.
fn node_opacity(node: &Node, at: f64, duration: f64) -> f64 {
    match node.property(
        "opacity",
        at,
        duration,
        Value::Number {
            value: node.opacity,
        },
    ) {
        Value::Number { value } => value,
        _ => node.opacity,
    }
}

/// A number track's value at `at`, or zero.
fn t_of(node: &Node, name: &str, at: f64, duration: f64) -> f64 {
    match node.property(name, at, duration, Value::Number { value: 0.0 }) {
        Value::Number { value } => value,
        _ => 0.0,
    }
}

/// The frame a percentage length resolves against, in comp pixels:
/// percentages pass through, pixels and autos read against the comp's
/// own frame so the reference is never zero.
fn resolve_reference(length: Length, frame: f64) -> f64 {
    match length {
        Length::Px(value) => value,
        Length::Percent(value) => frame * value / 100.0,
        Length::Auto => frame,
    }
}

/// The reference a node's *transform* resolves against, which is its
/// own reference frame: the nearest box ancestor's box when it has
/// one, the comp's frame otherwise.
fn node_transform_reference(scene: &Scene<'_>, node: &Node) -> (f64, f64) {
    scene.reference_of(node).0
}

/// Multiplies a layer's alpha by a stencil's alpha: the mask. The
/// colour channels scale with it, because a half-kept pixel keeps half
/// its ink - an alpha trim on premultiplied bytes is a colour trim too.
fn apply_stencil(layer: &mut Pixmap, stencil: &Pixmap) {
    let into = layer.data_mut();
    for (below, cover) in into.chunks_exact_mut(4).zip(stencil.data().chunks_exact(4)) {
        let keep = (u16::from(below[3]) * u16::from(cover[3]) + 127) / 255;
        below[0] = ((u16::from(below[0]) * keep + 127) / 255) as u8;
        below[1] = ((u16::from(below[1]) * keep + 127) / 255) as u8;
        below[2] = ((u16::from(below[2]) * keep + 127) / 255) as u8;
        below[3] = keep.clamp(0, 255) as u8;
    }
}

/// The luma of a premultiplied pixel, straight: what a luma matte reads.
fn luma_of(r: u8, g: u8, b: u8, a: u8) -> f64 {
    let alpha = f64::from(a) / 255.0;
    if alpha <= 0.0 {
        return 0.0;
    }
    let straight = |pre: u8| f64::from(pre) / 255.0 / alpha;
    0.2126 * straight(r) + 0.7152 * straight(g) + 0.0722 * straight(b)
}

/// Multiplies a layer's alpha by a matte source, read as the mode says.
fn apply_matte(layer: &mut Pixmap, source: &Pixmap, mode: MatteMode) {
    let into = layer.data_mut();
    for (below, from) in into.chunks_exact_mut(4).zip(source.data().chunks_exact(4)) {
        let keep = match mode {
            MatteMode::Alpha => u16::from(from[3]),
            MatteMode::AlphaInverted => 255 - u16::from(from[3]),
            MatteMode::Luma => {
                (luma_of(from[0], from[1], from[2], from[3]).clamp(0.0, 1.0) * 255.0) as u16
            }
            MatteMode::LumaInverted => {
                ((1.0 - luma_of(from[0], from[1], from[2], from[3]).clamp(0.0, 1.0)) * 255.0) as u16
            }
        };
        let masked = (u16::from(below[3]) * keep + 127) / 255;
        below[0] = ((u16::from(below[0]) * masked + 127) / 255) as u8;
        below[1] = ((u16::from(below[1]) * masked + 127) / 255) as u8;
        below[2] = ((u16::from(below[2]) * masked + 127) / 255) as u8;
        below[3] = masked.clamp(0, 255) as u8;
    }
}
