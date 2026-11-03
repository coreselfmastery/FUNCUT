// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The Lottie adapter: a bodymovin document becomes a comp.
//!
//! What converts, and what it becomes:
//!
//! | Lottie | Comp |
//! |---|---|
//! | the root (`fr`, `ip`, `op`, `w`, `h`) | the comp's `fps`, `duration`, `width`, `height` |
//! | a solid layer (`ty: 1`) | a `rect` node, `sc` its fill |
//! | an image layer (`ty: 2`) | an `image` node, the file registered as an asset |
//! | a shape layer (`ty: 4`) | a `group` node carrying the layer's `ks`, its shapes as children |
//! | a shape group (`gr`) | a `group` node carrying the group's own `tr` |
//! | `el` / `rc` / `sh` | `ellipse` / `rect` / `path` nodes |
//! | `fl` / `st` | the shapes' fill and stroke |
//! | a precomp layer (`ty: 0`) | the precomp's layers, nested under a group |
//! | a null layer (`ty: 3`) | a `group` node that draws nothing |
//! | `ks` - position, anchor, scale, rotation, opacity | the node's transform, its changes as tracks |
//! | keyframes, with `o`/`i` easing and `h` holds | track keys with bezier curves and `steps()` |
//! | `parent` (layer parenting) | the node's `parent`, by the node the layer became |
//! | `ip`/`op` (in and out points) | the node's `start` and `duration`, in seconds |
//!
//! Two order rules, both the ones the reference players render by: the
//! first layer of a `layers` array is the front (After Effects writes its
//! timeline top-first, so the walk reverses the list into the comp's
//! under-to-over node order), and within a shape list later items paint on
//! top (a `fl` paints the geometry accumulated before it in the same
//! group - the `[el, fl, tr]` pattern every file writes).
//!
//! What does not convert - a text layer, a matte, a mask, a trim path, a
//! gradient paint, a repeater, an effect, a 3D layer, a hidden layer - is
//! skipped with a note naming the layer and the feature, because the comp
//! vocabulary has no counterpart and a quiet guess is how a logo comes
//! back wrong. What converts nearly - a non-uniform scale, a curved motion
//! path, an animated shape size, a group paint over several shapes, an
//! elastic ease - is approximated and said.
//!
//! Paths are re-based to their own bounding box and the box is placed
//! where the source drew it, which is the convention the comp's `path`
//! node assumes; they are drawn by the native engine only, as any
//! hand-written path is, and the bake says so per node.
//!
//! A `.lottie` bundle is zipped: the caller unpacks it and passes the
//! `animation.json` inside, and a document without `layers` or `w`/`h`
//! is refused as not Lottie rather than converted into an empty comp. A
//! document whose every layer is skipped is refused with the comp's own
//! words - a comp with no nodes draws nothing - because an empty comp is
//! not a conversion either.

use std::collections::BTreeMap;

use concat_motion::comp::{
    Asset, AssetKind, Comp, Fit, Length, Node, NodeKind, Paint, PathCommand, Point, Rgba, Stroke,
    Transform,
};
use concat_motion::curve::{Curve, StepPosition};
use concat_motion::track::{Key, Track, Value};
use serde_json::Value as Json;

use crate::{AdapterError, Import, numbers};

/// How deep precomp nesting converts before the adapter says no. After
/// Effects allows more; a motion graphic eight precomps deep is a file the
/// designer should flatten, not a document an agent should unwrap live.
const DEPTH_LIMIT: usize = 8;

/// Convert a bodymovin document into a comp.
///
/// The document is read defensively - a field that is not the shape the
/// format says it should be reads as absent rather than failing the whole
/// conversion - and everything that was skipped, approximated or clamped
/// is said in the [`Import`]'s notes. A document that is not Lottie at all
/// is refused with [`AdapterError::NotTheFormat`], because an empty comp
/// is not a conversion.
pub fn import(document: &Json) -> Result<Import, AdapterError> {
    let not_lottie = |said: &str| AdapterError::NotTheFormat {
        format: "Lottie (bodymovin)",
        said: said.into(),
    };
    if document.get("layers").and_then(Json::as_array).is_none() {
        return Err(not_lottie(
            "the document has no `layers` array (a `.lottie` bundle is zipped: unpack it and pass the `animation.json` inside)",
        ));
    }
    let width = dim(document, "w")?;
    let height = dim(document, "h")?;
    let fps = document
        .get("fr")
        .and_then(Json::as_f64)
        .filter(|fps| *fps > 0.0)
        .ok_or_else(|| not_lottie("the frame rate (`fr`) is missing or zero"))?;
    let ip = document.get("ip").and_then(Json::as_f64).unwrap_or(0.0);
    let op = document.get("op").and_then(Json::as_f64).unwrap_or(0.0);
    if op <= ip {
        return Err(not_lottie(
            "`op` does not come after `ip`, so the comp has no length",
        ));
    }

    let mut import = Import::clean(Comp {
        format: 1,
        id: "lottie.import".into(),
        name: document
            .get("nm")
            .and_then(Json::as_str)
            .unwrap_or("A Lottie import")
            .into(),
        width,
        height,
        fps,
        duration: (op - ip) / fps,
        background: Rgba::new(0.0, 0.0, 0.0, 0.0),
        assets: BTreeMap::new(),
        curves: BTreeMap::new(),
        nodes: Vec::new(),
        comps: BTreeMap::new(),
        slots: Vec::new(),
    });

    let mut pictures = BTreeMap::new();
    let mut precomps = BTreeMap::new();
    for asset in document
        .get("assets")
        .and_then(Json::as_array)
        .unwrap_or(&Vec::new())
    {
        let Some(id) = asset.get("id").and_then(Json::as_str) else {
            continue;
        };
        if asset.get("layers").and_then(Json::as_array).is_some() {
            precomps.insert(id.to_string(), asset.clone());
        } else {
            pictures.insert(id.to_string(), asset.clone());
        }
    }

    let mut walk = Walk {
        fps,
        comp_ip: ip,
        pictures: &pictures,
        precomps: &precomps,
        seen: BTreeMap::new(),
        depth: 0,
    };
    let layers = document
        .get("layers")
        .and_then(Json::as_array)
        .unwrap()
        .clone();
    import.comp.nodes = walk
        .layers(&layers, "", None, import.comp.duration, &mut import)
        .nodes;

    let problems = import
        .comp
        .validate()
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect::<Vec<String>>();
    if !problems.is_empty() {
        return Err(AdapterError::Invalid { problems });
    }
    Ok(import)
}

/// What a walk of a layer list or a shape list produced: every node it
/// made, flat, and the ids of the list's *direct* children - the layers
/// themselves, or the top-level shape items - because a group's `children`
/// names its own layer, and a shape grandchild in that list would be a
/// parentage said twice.
#[derive(Default)]
struct Nested {
    /// The ids of the list's direct children, in paint order.
    direct: Vec<String>,
    /// Every node made under the list, in paint order, parents set.
    nodes: Vec<Node>,
}

/// The walk's shared state: the document's clock, its assets, the ids
/// already taken (a layer named twice gets a suffix, not a collision), and
/// how deep into precomps the walk already is.
struct Walk<'a> {
    fps: f64,
    comp_ip: f64,
    pictures: &'a BTreeMap<String, Json>,
    precomps: &'a BTreeMap<String, Json>,
    seen: BTreeMap<String, usize>,
    depth: usize,
}

impl<'a> Walk<'a> {
    /// Convert a list of layers into their nodes. The first layer of the
    /// array is the front, so the list is walked backwards into the comp's
    /// under-to-over node order. Ids are assigned in a first pass over the
    /// array as written, so a name's suffixes do not depend on the walk's
    /// direction and a child layer can point at its parent before the
    /// parent has been walked.
    ///
    /// `owner` is the group the list hangs under - `None` for the comp's
    /// own layer list, the precomp group's id for a precomp's - and every
    /// layer that does not name a parent of its own is parented to it.
    fn layers(
        &mut self,
        layers: &[Json],
        prefix: &str,
        owner: Option<&str>,
        duration: f64,
        import: &mut Import,
    ) -> Nested {
        let mut ids: BTreeMap<i64, String> = BTreeMap::new();
        for layer in layers {
            let Some(ind) = layer.get("ind").and_then(Json::as_i64) else {
                continue;
            };
            let name = layer_name(layer, ind);
            ids.insert(ind, self.unique_id(&format!("{prefix}{}", slug(&name))));
        }
        let mut walked = Nested::default();
        for layer in layers.iter().rev() {
            let mut made = self.layer(layer, &ids, duration, import);
            if let Some(first) = made.first_mut() {
                if first.parent.is_none() {
                    first.parent = owner.map(ToString::to_string);
                }
                walked.direct.push(first.id.clone());
            }
            walked.nodes.append(&mut made);
        }
        walked
    }

    /// Convert one layer into its node and its children's nodes (a shape
    /// layer is a group plus a child per shape), or note why it was
    /// skipped and convert nothing.
    fn layer(
        &mut self,
        layer: &Json,
        ids: &BTreeMap<i64, String>,
        duration: f64,
        import: &mut Import,
    ) -> Vec<Node> {
        let ind = layer.get("ind").and_then(Json::as_i64).unwrap_or(0);
        let name = layer_name(layer, ind);
        if layer.get("hd").and_then(Json::as_bool) == Some(true) {
            import.note(format!("skipped: layer {name:?} is hidden in the source"));
            return Vec::new();
        }
        let id = ids
            .get(&ind)
            .cloned()
            .unwrap_or_else(|| self.unique_id(&slug(&name)));

        // The layer's clock: in and out points are frames on the comp's own
        // timeline. A layer outside the comp's span is clamped with a note,
        // because the comp refuses a node that starts before it begins.
        let ip = layer
            .get("ip")
            .and_then(Json::as_f64)
            .unwrap_or(self.comp_ip);
        let op = layer
            .get("op")
            .and_then(Json::as_f64)
            .unwrap_or(self.comp_ip + 1.0);
        let (start, end) = self.window(ip, op, duration, &name, import);

        self.notes_of(layer, &name, import);

        let ty = layer.get("ty").and_then(Json::as_i64);
        let mut made = match ty {
            Some(1) => self
                .solid(layer, &id, start, end)
                .into_iter()
                .collect::<Vec<Node>>(),
            Some(2) => self
                .picture(layer, &id, start, end, &name, import)
                .into_iter()
                .collect(),
            Some(3) => vec![shell(
                &id,
                start,
                end,
                NodeKind::Group {
                    children: Vec::new(),
                },
            )],
            Some(4) => self.shape_layer(layer, &id, start, end, &name, import),
            Some(0) => self.precomp(layer, &id, start, end, &name, import),
            Some(5) => {
                import.note(format!(
                    "skipped: text layer {name:?}; the comp's text is a text node with its own style - write the words in by hand"
                ));
                Vec::new()
            }
            other => {
                import.note(format!(
                    "skipped: layer {name:?} is of type {other:?}, which the adapter does not read"
                ));
                Vec::new()
            }
        };
        if made.is_empty() {
            return Vec::new();
        }

        // Parenting: the layer's parent becomes the node's parent by id -
        // the id the first pass assigned, so the edge points at the node
        // the parent layer became whatever direction the walk reached it
        // from. A parent that was skipped leaves the edge dropped and said.
        if let Some(parent) = layer.get("parent").and_then(Json::as_i64) {
            match ids.get(&parent) {
                Some(parent_id) => made[0].parent = Some(parent_id.clone()),
                None => import.note(format!(
                    "skipped: layer {name:?} parents a layer that did not convert; the edge is dropped"
                )),
            }
        }

        let box_size = box_of(&made[0]);
        self.transform(
            layer.get("ks").unwrap_or(&Json::Null),
            &mut made[0],
            box_size,
            &name,
            import,
        );
        made
    }

    /// A solid layer: one rectangle, the comp's cheapest and most faithful
    /// conversion - a size, a colour, a place.
    fn solid(&mut self, layer: &Json, id: &str, start: f64, end: f64) -> Option<Node> {
        let width = layer.get("sw").and_then(Json::as_f64).unwrap_or(100.0);
        let height = layer.get("sh").and_then(Json::as_f64).unwrap_or(100.0);
        let color = layer
            .get("sc")
            .and_then(Json::as_str)
            .map(Rgba::hex)
            .unwrap_or_else(|| Rgba::new(1.0, 1.0, 1.0, 1.0));
        Some(shell(
            id,
            start,
            end,
            NodeKind::Rect {
                width: Length::Px(width),
                height: Length::Px(height),
                fill: Some(Paint::Solid { color }),
                radius: 0.0,
            },
        ))
    }

    /// An image layer: the file registered as a comp asset, referenced by
    /// an id no other asset shares.
    fn picture(
        &mut self,
        layer: &Json,
        id: &str,
        start: f64,
        end: f64,
        name: &str,
        import: &mut Import,
    ) -> Option<Node> {
        let ref_id = layer
            .get("refId")
            .and_then(Json::as_str)
            .map(ToString::to_string);
        match ref_id
            .as_deref()
            .and_then(|ref_id| self.pictures.get(ref_id))
        {
            Some(picture) => {
                let asset_id = format!("lottie.{}", slug(ref_id.as_deref().unwrap_or("asset")));
                let dir = picture.get("u").and_then(Json::as_str).unwrap_or("");
                let file = picture.get("p").and_then(Json::as_str).unwrap_or("");
                let extension = picture.get("e").and_then(Json::as_str).unwrap_or("");
                let mut src = format!("{dir}{file}");
                if !extension.is_empty() && !src.ends_with(extension) {
                    src = format!("{src}.{extension}");
                }
                import.comp.assets.insert(
                    asset_id.clone(),
                    Asset {
                        src,
                        kind: Some(AssetKind::Image),
                    },
                );
                Some(shell(
                    id,
                    start,
                    end,
                    NodeKind::Image {
                        asset: asset_id,
                        fit: Fit::Contain,
                    },
                ))
            }
            None => {
                import.note(format!(
                    "skipped: image layer {name:?} names an asset that is not in the document"
                ));
                None
            }
        }
    }

    /// A shape layer: the layer node is a group carrying the layer's own
    /// `ks`, and every shape item becomes a child parented to it. The
    /// group comes first in the list (it draws nothing, so its place in
    /// paint order is only bookkeeping) and the children follow in shape
    /// order, the later one on top.
    fn shape_layer(
        &mut self,
        layer: &Json,
        id: &str,
        start: f64,
        end: f64,
        name: &str,
        import: &mut Import,
    ) -> Vec<Node> {
        let shapes = layer.get("shapes").and_then(Json::as_array);
        let walked = match shapes {
            Some(shapes) if !shapes.is_empty() => self.shapes(shapes, id, start, end, import),
            _ => {
                import.note(format!("skipped: shape layer {name:?} carries no shapes"));
                return Vec::new();
            }
        };
        if walked.nodes.is_empty() {
            import.note(format!(
                "skipped: shape layer {name:?} carries shapes, none of which the adapter could read"
            ));
            return Vec::new();
        }
        let group = shell(
            id,
            start,
            end,
            NodeKind::Group {
                children: walked.direct,
            },
        );
        let mut made = vec![group];
        made.extend(walked.nodes);
        made
    }

    /// A precomp layer: the referenced composition's layers convert under
    /// a group of their own, their ids prefixed so two uses of the same
    /// precomp never collide.
    fn precomp(
        &mut self,
        layer: &Json,
        id: &str,
        start: f64,
        end: f64,
        name: &str,
        import: &mut Import,
    ) -> Vec<Node> {
        if self.depth >= DEPTH_LIMIT {
            import.note(format!(
                "skipped: precomp layer {name:?} nests {} deep, past the adapter's limit of {DEPTH_LIMIT}",
                self.depth
            ));
            return Vec::new();
        }
        let ref_id = layer
            .get("refId")
            .and_then(Json::as_str)
            .map(ToString::to_string);
        match ref_id
            .as_deref()
            .and_then(|ref_id| self.precomps.get(ref_id))
        {
            Some(precomp) => {
                let inner = precomp.get("layers").and_then(Json::as_array);
                let inner = match inner {
                    Some(inner) if !inner.is_empty() => inner.clone(),
                    _ => {
                        import.note(format!("skipped: precomp layer {name:?} has no layers"));
                        return Vec::new();
                    }
                };
                if precomp
                    .get("fr")
                    .and_then(Json::as_f64)
                    .is_some_and(|fr| fr != self.fps)
                {
                    import.note(format!(
                        "approximated: precomp layer {name:?} has a frame rate of its own; its times are read on the comp's clock"
                    ));
                }
                if layer
                    .get("st")
                    .and_then(Json::as_f64)
                    .is_some_and(|st| st != 0.0)
                {
                    import.note(format!(
                        "approximated: precomp layer {name:?} starts its source at an offset; the window is placed as written and the keys are not shifted"
                    ));
                }
                self.depth += 1;
                let prefix = format!("{id}/");
                let walked = self.layers(&inner, &prefix, Some(id), end - start, import);
                self.depth -= 1;
                if walked.nodes.is_empty() {
                    return Vec::new();
                }
                let group = shell(
                    id,
                    start,
                    end,
                    NodeKind::Group {
                        children: walked.direct,
                    },
                );
                let mut made = vec![group];
                made.extend(walked.nodes);
                made
            }
            None => {
                import.note(format!(
                    "skipped: precomp layer {name:?} names an asset that is not in the document"
                ));
                Vec::new()
            }
        }
    }

    /// Convert a shape list into child nodes under `owner`. Geometry items
    /// become nodes parented to the owner; paint items fill the geometry
    /// accumulated before them in the same group; a `gr` recurses with its
    /// own `tr`; anything the comp has no answer for is said.
    fn shapes(
        &mut self,
        items: &[Json],
        owner: &str,
        start: f64,
        end: f64,
        import: &mut Import,
    ) -> Nested {
        self.shapes_at(items, owner, start, end, true, import)
    }

    /// The walk itself, `top` saying whether this list is a layer's own
    /// shape list (where a `tr` is the layer-level transform, which the
    /// adapter does not stack) or a group's `it` (where the `tr` belongs
    /// to the group the caller is building, and is read there).
    fn shapes_at(
        &mut self,
        items: &[Json],
        owner: &str,
        start: f64,
        end: f64,
        top: bool,
        import: &mut Import,
    ) -> Nested {
        let mut walked = Nested::default();
        let mut unpainted: Vec<usize> = Vec::new();
        let paint = |nodes: &mut [Node],
                     unpainted: &mut Vec<usize>,
                     import: &mut Import,
                     fill: Option<Paint>,
                     stroke: Option<Stroke>| {
            if unpainted.is_empty() {
                return;
            }
            if unpainted.len() > 1 {
                import.note(format!(
                    "approximated: a group paint over {} shapes is split per shape; where they overlap, two half-opaque shapes darken more than one union would",
                    unpainted.len()
                ));
            }
            for index in unpainted.iter().copied() {
                if let Some(node) = nodes.get_mut(index) {
                    match &mut node.kind {
                        NodeKind::Rect { fill: into, .. }
                        | NodeKind::Ellipse { fill: into, .. } => {
                            if let Some(fill) = fill.clone() {
                                *into = Some(fill);
                            }
                        }
                        NodeKind::Path {
                            fill: into,
                            stroke: as_stroke,
                            ..
                        } => {
                            if let Some(fill) = fill.clone() {
                                *into = Some(fill);
                            }
                            if let Some(stroke) = stroke.clone() {
                                *as_stroke = Some(stroke);
                            }
                        }
                        _ => {}
                    }
                }
            }
            unpainted.clear();
        };

        for item in items {
            match item.get("ty").and_then(Json::as_str) {
                Some("gr") => {
                    let group_id = self.unique_id(&format!(
                        "{owner}/{}",
                        slug(item.get("nm").and_then(Json::as_str).unwrap_or("group"))
                    ));
                    let inner = item
                        .get("it")
                        .and_then(Json::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let mut children = self.shapes_at(&inner, &group_id, start, end, false, import);
                    let mut group = shell(
                        &group_id,
                        start,
                        end,
                        NodeKind::Group {
                            children: children.direct.clone(),
                        },
                    );
                    group.parent = Some(owner.into());
                    if let Some(tr) = inner
                        .iter()
                        .rev()
                        .find(|item| item.get("ty").and_then(Json::as_str) == Some("tr"))
                    {
                        self.transform(tr, &mut group, None, &group_id, import);
                    }
                    walked.direct.push(group.id.clone());
                    walked.nodes.push(group);
                    walked.nodes.append(&mut children.nodes);
                }
                Some("el") => {
                    let (width, height) = sized(item, import, "ellipse");
                    let mut node = shell(
                        &self.next_id(owner, "ellipse"),
                        start,
                        end,
                        NodeKind::Ellipse {
                            width: Length::Px(width),
                            height: Length::Px(height),
                            fill: None,
                        },
                    );
                    node.parent = Some(owner.into());
                    self.placed(item, &mut node, import);
                    unpainted.push(walked.nodes.len());
                    walked.direct.push(node.id.clone());
                    walked.nodes.push(node);
                }
                Some("rc") => {
                    let (width, height) = sized(item, import, "rect");
                    let radius = prop(item, "r")
                        .and_then(|prop| prop.static_number())
                        .unwrap_or(0.0);
                    let mut node = shell(
                        &self.next_id(owner, "rect"),
                        start,
                        end,
                        NodeKind::Rect {
                            width: Length::Px(width),
                            height: Length::Px(height),
                            fill: None,
                            radius,
                        },
                    );
                    node.parent = Some(owner.into());
                    self.placed(item, &mut node, import);
                    unpainted.push(walked.nodes.len());
                    walked.direct.push(node.id.clone());
                    walked.nodes.push(node);
                }
                Some("sh") => {
                    let id = self.next_id(owner, "path");
                    let (commands, bounds) = path_commands(item, import, &id);
                    let mut node = shell(
                        &id,
                        start,
                        end,
                        NodeKind::Path {
                            commands,
                            fill: None,
                            stroke: None,
                            dash: None,
                        },
                    );
                    node.parent = Some(owner.into());
                    if let Some((x, y)) = bounds {
                        node.transform.position = Some(Point::new(x, y));
                    }
                    unpainted.push(walked.nodes.len());
                    walked.direct.push(node.id.clone());
                    walked.nodes.push(node);
                }
                Some("tr") if top => {
                    import.note(format!(
                        "skipped: a shape-level transform at the top of {owner:?}'s shapes; the layer's own transform carries it and a second stack is not built"
                    ));
                }
                Some("tr") => {}
                Some("fl") => {
                    let fill = Paint::Solid {
                        color: paint_color(item, "c", "o", import),
                    };
                    paint(&mut walked.nodes, &mut unpainted, import, Some(fill), None);
                }
                Some("st") => {
                    let color = paint_color(item, "c", "o", import);
                    let width = prop(item, "w")
                        .and_then(|prop| prop.static_number())
                        .unwrap_or(1.0);
                    if matches!(prop(item, "w"), Some(Prop::Keyed(_))) {
                        import.note(
                            "approximated: a stroke's width is animated; it is held at its first value",
                        );
                    }
                    paint(
                        &mut walked.nodes,
                        &mut unpainted,
                        import,
                        None,
                        Some(Stroke { color, width }),
                    );
                }
                Some(other) => {
                    let said = match other {
                        "gf" | "gs" => "a gradient paint",
                        "tm" => "a trim path",
                        "rd" => "rounded corners",
                        "mm" => "a merge",
                        "rp" => "a repeater",
                        "pb" => "a pucker and bloat",
                        "zz" => "a zigzag",
                        "sr" => "a star or polygon",
                        _ => "a shape modifier the adapter does not read",
                    };
                    import.note(format!("skipped: {said} inside {owner:?}"));
                }
                None => {}
            }
        }
        for index in unpainted {
            import.note(format!(
                "approximated: shape {} has no fill or stroke in its group; it is kept unpainted, as the source renders it",
                walked.nodes[index].id
            ));
        }
        walked
    }

    /// Place a geometry item: its `p` (the shape's own place in the group)
    /// becomes the node's transform, static or animated.
    fn placed(&mut self, item: &Json, node: &mut Node, import: &mut Import) {
        match prop(item, "p") {
            Some(Prop::Static(vec)) if vec.len() >= 2 => {
                node.transform.position = Some(Point::new(vec[0], vec[1]));
            }
            Some(Prop::Keyed(keys)) => {
                if let (Some(x), Some(y)) = (
                    keys.first().and_then(|key| key.value.first().copied()),
                    keys.first().and_then(|key| key.value.get(1).copied()),
                ) {
                    node.transform.position = Some(Point::new(x, y));
                }
                self.track_of(&keys, node, "position", import, |vec| {
                    vec.first()
                        .zip(vec.get(1))
                        .map(|(x, y)| Value::Point { x: *x, y: *y })
                });
            }
            _ => {}
        }
    }

    /// Convert a transform block (`ks`, or a group's `tr`) into the node's
    /// transform and tracks. Static values land in the transform; animated
    /// ones key a track from the node's own span, which is what every comp
    /// track is keyed against.
    fn transform(
        &mut self,
        ks: &Json,
        node: &mut Node,
        box_size: Option<(f64, f64)>,
        name: &str,
        import: &mut Import,
    ) {
        // Opacity, 0..100 in Lottie, 0..1 in a comp.
        match prop(ks, "o") {
            Some(Prop::Static(vec)) => {
                node.opacity = vec.first().copied().unwrap_or(100.0) / 100.0;
            }
            Some(Prop::Keyed(keys)) => {
                node.opacity = keys
                    .first()
                    .and_then(|key| key.value.first().copied())
                    .unwrap_or(100.0)
                    / 100.0;
                self.track_of(&keys, node, "opacity", import, |vec| {
                    vec.first().map(|v| Value::number(v / 100.0))
                });
            }
            None => {}
        }
        // Rotation, degrees clockwise in both.
        match prop(ks, "r") {
            Some(Prop::Static(vec)) => {
                node.transform.rotation = vec.first().copied().unwrap_or(0.0);
            }
            Some(Prop::Keyed(keys)) => {
                node.transform.rotation = keys
                    .first()
                    .and_then(|key| key.value.first().copied())
                    .unwrap_or(0.0);
                self.track_of(&keys, node, "rotation", import, |vec| {
                    vec.first().copied().map(Value::number)
                });
            }
            None => {}
        }
        // Position: `[x, y]`, or split `px`/`py` when the layer says so.
        let position = prop(ks, "p").or_else(|| {
            match (
                prop(ks, "px").and_then(Prop::into_static_number),
                prop(ks, "py").and_then(Prop::into_static_number),
            ) {
                (Some(x), Some(y)) => Some(Prop::Static(vec![x, y])),
                _ => None,
            }
        });
        match position {
            Some(Prop::Static(vec)) if vec.len() >= 2 => {
                node.transform.position = Some(Point::new(vec[0], vec[1]));
            }
            Some(Prop::Keyed(keys)) => {
                if let (Some(x), Some(y)) = (
                    keys.first().and_then(|key| key.value.first().copied()),
                    keys.first().and_then(|key| key.value.get(1).copied()),
                ) {
                    node.transform.position = Some(Point::new(x, y));
                }
                if keys.iter().any(|key| key.to.is_some() || key.ti.is_some()) {
                    import.note(format!(
                        "approximated: layer {name:?} moves along a curved path; the comp track moves it in a straight line between keys"
                    ));
                }
                self.track_of(&keys, node, "position", import, |vec| {
                    vec.first()
                        .zip(vec.get(1))
                        .map(|(x, y)| Value::Point { x: *x, y: *y })
                });
            }
            _ => {}
        }
        // The anchor: pixels in the layer's own box, fractions of the box
        // in a comp - exact for a node with a size of its own, approximated
        // (and said) for a group, which has no box to be a fraction of.
        if let Some(Prop::Static(vec)) = prop(ks, "a")
            && vec.len() >= 2
            && (vec[0].abs() > 1e-9 || vec[1].abs() > 1e-9)
        {
            match box_size {
                Some((width, height)) if width > 0.0 && height > 0.0 => {
                    node.transform.anchor = Some(Point::new(vec[0] / width, vec[1] / height));
                }
                _ => {
                    import.note(format!(
                        "approximated: layer {name:?} rotates and scales about an anchor of its own; a group has no box, so the comp turns it about its place"
                    ));
                }
            }
        }
        // Scale: percent in Lottie, a fraction in a comp - and one number,
        // so a non-uniform scale is the mean of the two axes, and says so.
        match prop(ks, "s") {
            Some(Prop::Static(vec)) => {
                let mean = scale_mean(&vec);
                self.note_non_uniform(&vec, name, import);
                node.transform.scale = mean / 100.0;
            }
            Some(Prop::Keyed(keys)) => {
                node.transform.scale = scale_mean(
                    &keys
                        .first()
                        .map(|key| key.value.clone())
                        .unwrap_or_default(),
                ) / 100.0;
                if keys.iter().any(|key| {
                    key.value
                        .first()
                        .zip(key.value.get(1))
                        .is_some_and(|(x, y)| (x - y).abs() > 1e-9)
                }) {
                    import.note(format!(
                        "approximated: layer {name:?} scales non-uniformly while animated; the comp's scale is one number, so both axes take the mean of each key"
                    ));
                }
                self.track_of(&keys, node, "scale", import, |vec| {
                    Some(Value::number(scale_mean(vec) / 100.0))
                });
            }
            None => {}
        }
    }

    /// Note a static non-uniform scale.
    fn note_non_uniform(&mut self, vec: &[f64], name: &str, import: &mut Import) {
        if let (Some(x), Some(y)) = (vec.first(), vec.get(1))
            && (x - y).abs() > 1e-9
        {
            import.note(format!(
                "approximated: layer {name:?} scales {x:.0}% by {y:.0}%; the comp's scale is one number, so both are {:.0}%",
                scale_mean(vec)
            ));
        }
    }

    /// Key a track from bodymovin keyframes: frame times become fractions
    /// of the node's span, values pass through `make`, eases become bezier
    /// curves and holds become steps.
    fn track_of(
        &mut self,
        keys: &[PropKey],
        node: &mut Node,
        name: &str,
        _import: &mut Import,
        make: impl Fn(&[f64]) -> Option<Value>,
    ) {
        let span = if node.duration > 0.0 {
            node.duration
        } else {
            1.0
        };
        let fraction = |frame: f64| -> f64 {
            (((frame - self.comp_ip) / self.fps - node.start) / span).clamp(0.0, 1.0)
        };
        let mut track: Vec<Key> = Vec::new();
        for key in keys {
            let Some(value) = make(&key.value) else {
                continue;
            };
            let at = fraction(key.t);
            let curve = if key.hold {
                Some(Curve::Steps {
                    count: 1,
                    position: StepPosition::JumpEnd,
                })
            } else {
                key.out.zip(key.into).map(|(out, into)| Curve::Bezier {
                    x1: out.0,
                    y1: out.1,
                    x2: into.0,
                    y2: into.1,
                })
            };
            track.push(Key { at, value, curve });
        }
        if track.len() < 2 {
            return;
        }
        // One key per instant - the later one, which a modern file repeats
        // as the next key's `s` - then in order.
        track.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        let mut deduped: Vec<Key> = Vec::new();
        for key in track {
            match deduped.last_mut() {
                Some(last) if (last.at - key.at).abs() < 1e-9 => *last = key,
                _ => deduped.push(key),
            }
        }
        node.tracks.insert(name.into(), Track::new(deduped));
    }

    /// The layer's window on the comp's clock, clamped to the comp with a
    /// note when it overruns either end.
    fn window(
        &mut self,
        ip: f64,
        op: f64,
        duration: f64,
        name: &str,
        import: &mut Import,
    ) -> (f64, f64) {
        let mut start = (ip - self.comp_ip) / self.fps;
        let mut end = (op - self.comp_ip) / self.fps;
        if start < 0.0 {
            import.note(format!(
                "clamped: layer {name:?} starts {:.2}s before the comp does; the comp begins at 0",
                -start
            ));
            start = 0.0;
        }
        if end > duration {
            import.note(format!(
                "clamped: layer {name:?} ends {:.2}s in, past the comp's {:.2}s; it is held to the comp's end",
                end, duration
            ));
            end = duration;
        }
        (start, end.max(start + 1.0 / self.fps.max(1.0)))
    }

    /// The per-layer notes a file can raise: mattes, masks, effects, 3D,
    /// auto-orient and time remap.
    fn notes_of(&mut self, layer: &Json, name: &str, import: &mut Import) {
        if layer.get("ddd").and_then(Json::as_i64) == Some(1) {
            import.note(format!(
                "approximated: layer {name:?} is 3D; the comp is a 2D document and the z axis is dropped"
            ));
        }
        if layer.get("tt").is_some() || layer.get("td").is_some() {
            import.note(format!(
                "skipped: layer {name:?} is a matte participant; the comp vocabulary has no matte"
            ));
        }
        if layer
            .get("masksProperties")
            .and_then(Json::as_array)
            .is_some_and(|masks| !masks.is_empty())
        {
            import.note(format!(
                "skipped: layer {name:?} carries {} mask(s); a comp mask is a rounded rect or an ellipse over the whole node",
                layer.get("masksProperties").and_then(Json::as_array).map(Vec::len).unwrap_or(0)
            ));
        }
        if layer
            .get("ef")
            .and_then(Json::as_array)
            .is_some_and(|effects| !effects.is_empty())
        {
            import.note(format!(
                "skipped: layer {name:?} carries {} After Effects effect(s); the comp effects are blur, shadow and the shader catalogue",
                layer.get("ef").and_then(Json::as_array).map(Vec::len).unwrap_or(0)
            ));
        }
        if layer.get("ao").and_then(Json::as_i64) == Some(1) {
            import.note(format!(
                "approximated: layer {name:?} auto-orients its rotation to its path; the comp has no auto-orient"
            ));
        }
        if layer.get("tm").is_some() {
            import.note(format!(
                "skipped: layer {name:?} remaps time; a comp node's span is its own clock"
            ));
        }
    }

    /// An id no other node in this conversion has taken: the slug itself
    /// when it is free, and `name-2`, `name-3` when it is not.
    fn unique_id(&mut self, wanted: &str) -> String {
        let taken = self.seen.entry(wanted.to_string()).or_insert(0);
        *taken += 1;
        if *taken == 1 {
            wanted.to_string()
        } else {
            format!("{wanted}-{taken}")
        }
    }

    /// The next shape id under an owner.
    fn next_id(&mut self, owner: &str, kind: &str) -> String {
        self.unique_id(&format!("{owner}/{kind}"))
    }
}

/// A bodymovin property: static, or keyed.
enum Prop {
    /// `a: 0` - one value for the whole span (or a single key, which means
    /// the same thing).
    Static(Vec<f64>),
    /// `a: 1` - keyframes, in document order.
    Keyed(Vec<PropKey>),
}

impl Prop {
    /// The single number a property holds when it is not animated.
    fn static_number(&self) -> Option<f64> {
        match self {
            Prop::Static(vec) => vec.first().copied(),
            Prop::Keyed(keys) => keys.first().and_then(|key| key.value.first().copied()),
        }
    }

    /// A static property as a plain number, or `None` when keyed.
    fn into_static_number(self) -> Option<f64> {
        match self {
            Prop::Static(vec) => vec.first().copied(),
            Prop::Keyed(_) => None,
        }
    }
}

/// One keyframe of a bodymovin property.
struct PropKey {
    /// When, in frames on the comp's clock.
    t: f64,
    /// The value the segment starts at.
    value: Vec<f64>,
    /// The ease out of this key, as bezier control points.
    out: Option<(f64, f64)>,
    /// The ease into the next key.
    into: Option<(f64, f64)>,
    /// A hold: the value steps at the next key rather than easing to it.
    hold: bool,
    /// A spatial tangent out (a curved path through space).
    to: Option<Vec<f64>>,
    /// A spatial tangent in.
    ti: Option<Vec<f64>>,
}

/// Read a property by name from a transform or a shape item.
fn prop(parent: &Json, name: &str) -> Option<Prop> {
    let property = parent.get(name)?;
    let k = property.get("k")?;
    if property.get("a").and_then(Json::as_i64) != Some(1) {
        return Some(Prop::Static(numbers(k)));
    }
    let keys = k.as_array()?;
    let mut parsed = Vec::new();
    for key in keys {
        let value = key.get("s").map(numbers).filter(|v| !v.is_empty())?;
        parsed.push(PropKey {
            t: key.get("t").and_then(Json::as_f64).unwrap_or(0.0),
            value,
            out: handles(key, "o"),
            into: handles(key, "i"),
            hold: key.get("h").and_then(Json::as_i64) == Some(1),
            to: key.get("to").map(numbers).filter(|v| !v.is_empty()),
            ti: key.get("ti").map(numbers).filter(|v| !v.is_empty()),
        });
    }
    match parsed.len() {
        0 => None,
        1 => Some(Prop::Static(parsed.remove(0).value)),
        _ => Some(Prop::Keyed(parsed)),
    }
}

/// The bezier handles of a keyframe's ease, `(x, y)`.
fn handles(key: &Json, side: &str) -> Option<(f64, f64)> {
    let hand = key.get(side)?;
    let x = crate::number_or_first(hand.get("x")?)?;
    let y = crate::number_or_first(hand.get("y")?)?;
    Some((x, y))
}

/// A paint's colour with its own opacity folded in: bodymovin keeps the
/// colour as 0..1 sRGB and the opacity as 0..100 beside it, and a comp
/// wants the one number that says both.
fn paint_color(item: &Json, color: &str, opacity: &str, import: &mut Import) -> Rgba {
    let color = match prop(item, color) {
        Some(Prop::Static(vec)) => vec,
        Some(Prop::Keyed(keys)) => {
            import
                .note("approximated: a paint's colour is animated; it is held at its first value");
            keys.first()
                .map(|key| key.value.clone())
                .unwrap_or_default()
        }
        None => Vec::new(),
    };
    let opacity = prop(item, opacity)
        .and_then(|prop| prop.static_number())
        .unwrap_or(100.0)
        / 100.0;
    let mut rgba = Rgba::new(
        color.first().copied().unwrap_or(1.0),
        color.get(1).copied().unwrap_or(1.0),
        color.get(2).copied().unwrap_or(1.0),
        1.0,
    );
    rgba.a *= opacity;
    rgba
}

/// A geometry item's size, held at its first value when animated.
fn sized(item: &Json, import: &mut Import, kind: &str) -> (f64, f64) {
    match prop(item, "s") {
        Some(Prop::Static(vec)) => (
            vec.first().copied().unwrap_or(0.0),
            vec.get(1).copied().unwrap_or(0.0),
        ),
        Some(Prop::Keyed(keys)) => {
            import.note(format!(
                "approximated: a {kind}'s size is animated; it is held at its first value"
            ));
            let first = keys.first();
            (
                first
                    .and_then(|key| key.value.first().copied())
                    .unwrap_or(0.0),
                first
                    .and_then(|key| key.value.get(1).copied())
                    .unwrap_or(0.0),
            )
        }
        None => (0.0, 0.0),
    }
}

/// The mean of a scale value's two axes, as one number the comp's scale
/// field is honest about. A one-element value means both.
fn scale_mean(vec: &[f64]) -> f64 {
    match (vec.first(), vec.get(1)) {
        (Some(x), Some(y)) => (x + y) / 2.0,
        (Some(x), None) => *x,
        _ => 100.0,
    }
}

/// A `sh` path item's bezier, re-based to its own bounding box so the
/// node's own pixels start at its top-left corner - the box then placed
/// where the source drew it. An animated path is held at its first shape
/// and says so.
fn path_commands(
    item: &Json,
    import: &mut Import,
    id: &str,
) -> (Vec<PathCommand>, Option<(f64, f64)>) {
    let Some(ks) = item.get("ks") else {
        return (Vec::new(), None);
    };
    let animated = ks.get("a").and_then(Json::as_i64) == Some(1);
    let path = if animated {
        let shape = ks
            .get("k")
            .and_then(Json::as_array)
            .and_then(|keys| keys.first())
            .and_then(|key| key.get("s"));
        if shape.is_some() {
            import.note(format!(
                "approximated: path {id:?} is animated; the comp holds its first shape and the morph is lost"
            ));
        }
        shape
    } else {
        ks.get("k")
    };
    let Some(path) = path else {
        return (Vec::new(), None);
    };
    bezier_of(path)
}

/// The parallel arrays of a bodymovin bezier as path commands, re-based to
/// the shape's own bounding box. Returns the commands and the box's centre
/// in the coordinates the source used - the position the node takes.
fn bezier_of(shape: &Json) -> (Vec<PathCommand>, Option<(f64, f64)>) {
    let point_at = |value: Option<&Json>| -> Option<(f64, f64)> {
        let pair = numbers(value?);
        Some((pair.first().copied()?, pair.get(1).copied()?))
    };
    let vertices: Vec<(f64, f64)> = shape
        .get("v")
        .and_then(Json::as_array)
        .map(|points| {
            points
                .iter()
                .filter_map(|point| point_at(Some(point)))
                .collect()
        })
        .unwrap_or_default();
    if vertices.is_empty() {
        return (Vec::new(), None);
    }
    let tangent = |field: &str| -> Vec<(f64, f64)> {
        shape
            .get(field)
            .and_then(Json::as_array)
            .map(|points| {
                points
                    .iter()
                    .filter_map(|point| point_at(Some(point)))
                    .collect()
            })
            .unwrap_or_default()
    };
    let out = tangent("o");
    let into = tangent("i");
    let closed = shape.get("c").and_then(Json::as_bool).unwrap_or(false);

    // The bounds the vertices draw place the node; the control points can
    // leave them, and a node placed by its vertices' footprint is the
    // honest reading of "where the source drew it".
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in &vertices {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    let centre = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);

    let mut commands = Vec::new();
    commands.push(PathCommand::MoveTo {
        point: Point::new(vertices[0].0 - min_x, vertices[0].1 - min_y),
    });
    for index in 0..vertices.len() {
        let next = (index + 1) % vertices.len();
        if next == 0 && !closed {
            break;
        }
        let from = vertices[index];
        let to = vertices[next];
        let c1 = out
            .get(index)
            .map(|(x, y)| Point::new(from.0 + x - min_x, from.1 + y - min_y))
            .unwrap_or_else(|| Point::new(from.0 - min_x, from.1 - min_y));
        let c2 = into
            .get(next)
            .map(|(x, y)| Point::new(to.0 + x - min_x, to.1 + y - min_y))
            .unwrap_or_else(|| Point::new(to.0 - min_x, to.1 - min_y));
        commands.push(PathCommand::CubicTo {
            c1,
            c2,
            point: Point::new(to.0 - min_x, to.1 - min_y),
        });
    }
    if closed {
        commands.push(PathCommand::Close);
    }
    (commands, Some(centre))
}

/// A node's shell: everything a converted node starts from, with its span
/// and kind set.
fn shell(id: &str, start: f64, end: f64, kind: NodeKind) -> Node {
    Node {
        id: id.into(),
        parent: None,
        start,
        duration: (end - start).max(1.0 / 1000.0),
        transform: Transform::default(),
        opacity: 1.0,
        blend: concat_motion::comp::Blend::Normal,
        mask: None,
        effects: Vec::new(),
        tracks: BTreeMap::new(),
        kind,
    }
}

/// The box a node's anchor is a fraction of, when it has one.
fn box_of(node: &Node) -> Option<(f64, f64)> {
    match &node.kind {
        NodeKind::Rect { width, height, .. } | NodeKind::Ellipse { width, height, .. } => {
            match (width, height) {
                (Length::Px(w), Length::Px(h)) => Some((*w, *h)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A required pixel dimension of the root.
fn dim(document: &Json, field: &str) -> Result<u32, AdapterError> {
    document
        .get(field)
        .and_then(Json::as_f64)
        .filter(|v| *v > 0.0)
        .map(|v| v.round() as u32)
        .ok_or_else(|| AdapterError::NotTheFormat {
            format: "Lottie (bodymovin)",
            said: format!(
                "the {field} of the frame is missing or zero (a `.lottie` bundle is zipped: unpack it and pass the `animation.json` inside)"
            ),
        })
}

/// A layer's name, from `nm` when it has one, from its index when it does
/// not - every diagnostic names the layer by this.
fn layer_name(layer: &Json, ind: i64) -> String {
    layer
        .get("nm")
        .and_then(Json::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("layer-{ind}"))
}

/// A layer name as a node id: lowercase, runs of anything else as one dash.
fn slug(name: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if dash && !slug.is_empty() {
                slug.push('-');
            }
            dash = false;
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() {
            dash = true;
        }
    }
    if slug.is_empty() {
        "layer".into()
    } else {
        slug
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The root of every fixture: a 512×512 comp at 30fps, 0 to 90 frames
    /// (three seconds), with the layers given.
    fn lottie(layers: Vec<Json>) -> Json {
        json!({
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 90, "w": 512, "h": 512,
            "nm": "fixture", "layers": layers,
        })
    }

    /// A solid layer as After Effects writes one.
    fn solid(ind: i64, name: &str, color: &str) -> Json {
        json!({
            "ind": ind, "ty": 1, "nm": name, "ip": 0, "op": 90, "st": 0,
            "sw": 200, "sh": 100, "sc": color,
            "ks": {
                "o": {"a": 0, "k": 100},
                "r": {"a": 0, "k": 0},
                "p": {"a": 0, "k": [256, 256, 0]},
                "a": {"a": 0, "k": [100, 50, 0]},
                "s": {"a": 0, "k": [100, 100, 100]},
            },
        })
    }

    #[test]
    fn the_root_becomes_the_comp() {
        let import = import(&lottie(vec![solid(1, "plate", "#ff0000")])).expect("converts");
        assert_eq!(import.comp.width, 512);
        assert_eq!(import.comp.height, 512);
        assert_eq!(import.comp.fps, 30.0);
        assert!((import.comp.duration - 3.0).abs() < 1e-9);
        assert_eq!(import.comp.nodes.len(), 1);
        assert!(import.notes.is_empty(), "{:?}", import.notes);
    }

    #[test]
    fn a_solid_becomes_a_filled_rect() {
        let import = import(&lottie(vec![solid(1, "plate", "#336699")])).expect("converts");
        let node = &import.comp.nodes[0];
        assert_eq!(node.id, "plate");
        let NodeKind::Rect {
            width,
            height,
            fill,
            radius,
        } = &node.kind
        else {
            panic!("a solid becomes a rect, found {:?}", node.kind);
        };
        assert_eq!(width, &Length::Px(200.0));
        assert_eq!(height, &Length::Px(100.0));
        assert_eq!(*radius, 0.0);
        let Some(Paint::Solid { color }) = fill else {
            panic!("the solid has a fill");
        };
        assert_eq!(color, &Rgba::hex("#336699"));
        // The position places the anchor, and the anchor is the fraction of
        // the box the source named: [100, 50] in a 200×100 box is its
        // centre, exactly as the defaults would have it.
        assert_eq!(node.transform.position, Some(Point::new(256.0, 256.0)));
        assert_eq!(node.transform.anchor, Some(Point::new(0.5, 0.5)));
        assert_eq!(node.opacity, 1.0);
        assert_eq!(node.start, 0.0);
        assert!((node.duration - 3.0).abs() < 1e-9);
    }

    #[test]
    fn the_first_layer_is_the_front() {
        let import = import(&lottie(vec![
            solid(1, "front", "#ffffff"),
            solid(2, "back", "#000000"),
        ]))
        .expect("converts");
        // The comp paints earlier under later, so the first Lottie layer -
        // the front - is the last node.
        assert_eq!(import.comp.nodes[0].id, "back");
        assert_eq!(import.comp.nodes[1].id, "front");
    }

    #[test]
    fn two_layers_of_one_name_get_their_own_ids() {
        let import = import(&lottie(vec![
            solid(1, "plate", "#ffffff"),
            solid(2, "plate", "#000000"),
        ]))
        .expect("converts");
        // The walk reverses the list into under-to-over order, so the
        // second layer - the back - is the first node.
        assert_eq!(import.comp.nodes[0].id, "plate-2");
        assert_eq!(import.comp.nodes[1].id, "plate");
    }

    #[test]
    fn a_hidden_layer_is_skipped_and_said() {
        let mut layer = solid(1, "ghost", "#ffffff");
        layer["hd"] = json!(true);
        // Every layer skipped is an empty comp, and the comp's own
        // validation refuses it - honestly, rather than shipping a comp
        // that draws nothing.
        match import(&lottie(vec![layer])) {
            Err(AdapterError::Invalid { problems }) => assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("draws nothing")),
                "{problems:?}"
            ),
            other => panic!("an all-hidden document is refused, got {other:?}"),
        }
    }

    #[test]
    fn a_text_layer_is_refused_rather_than_guessed() {
        let mut layer = solid(1, "title", "#ffffff");
        layer["ty"] = json!(5);
        match import(&lottie(vec![layer])) {
            Err(AdapterError::Invalid { problems }) => assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("draws nothing")),
                "{problems:?}"
            ),
            other => panic!("a text-only document is refused, got {other:?}"),
        }
    }

    #[test]
    fn masks_mattes_effects_and_3d_are_said() {
        let mut layer = solid(1, "busy", "#ffffff");
        layer["masksProperties"] = json!([{ "nm": "m" }]);
        layer["tt"] = json!(1);
        layer["ef"] = json!([{ "ty": 21 }]);
        layer["ddd"] = json!(1);
        let import = import(&lottie(vec![layer])).expect("converts");
        assert_eq!(import.comp.nodes.len(), 1, "the layer still converts");
        let said: String = import.notes.join(" ");
        assert!(said.contains("mask"), "{said}");
        assert!(said.contains("matte"), "{said}");
        assert!(said.contains("effect"), "{said}");
        assert!(said.contains("3D"), "{said}");
    }

    #[test]
    fn a_layer_before_the_comp_is_clamped() {
        let mut layer = solid(1, "early", "#ffffff");
        layer["ip"] = json!(-30);
        layer["op"] = json!(30);
        let import = import(&lottie(vec![layer])).expect("converts");
        assert_eq!(import.comp.nodes[0].start, 0.0);
        assert!(
            import
                .notes
                .iter()
                .any(|note| note.starts_with("clamped: ")),
            "{:?}",
            import.notes
        );
    }

    #[test]
    fn a_layer_past_the_comp_is_held_to_its_end() {
        let mut layer = solid(1, "late", "#ffffff");
        layer["ip"] = json!(60);
        layer["op"] = json!(180);
        let import = import(&lottie(vec![layer])).expect("converts");
        let node = &import.comp.nodes[0];
        assert!((node.start - 2.0).abs() < 1e-9);
        assert!((node.start + node.duration - 3.0).abs() < 1e-9);
    }

    #[test]
    fn opacity_scales_from_percent_to_fraction() {
        let mut layer = solid(1, "half", "#ffffff");
        layer["ks"]["o"]["k"] = json!(40);
        let import = import(&lottie(vec![layer])).expect("converts");
        assert!((import.comp.nodes[0].opacity - 0.4).abs() < 1e-9);
    }

    #[test]
    fn a_non_uniform_scale_takes_the_mean_and_says_it() {
        let mut layer = solid(1, "wide", "#ffffff");
        layer["ks"]["s"]["k"] = json!([120, 80, 100]);
        let import = import(&lottie(vec![layer])).expect("converts");
        assert!((import.comp.nodes[0].transform.scale - 1.0).abs() < 1e-9);
        assert!(
            import
                .notes
                .iter()
                .any(|note| note.starts_with("approximated: ") && note.contains("scales")),
            "{:?}",
            import.notes
        );
    }

    #[test]
    fn a_rotating_anchor_becomes_a_fraction_of_the_box() {
        let mut layer = solid(1, "pinned", "#ffffff");
        layer["ks"]["a"]["k"] = json!([50, 25, 0]);
        layer["ks"]["r"]["k"] = json!(15);
        let import = import(&lottie(vec![layer])).expect("converts");
        let node = &import.comp.nodes[0];
        assert_eq!(node.transform.anchor, Some(Point::new(0.25, 0.25)));
        assert_eq!(node.transform.rotation, 15.0);
    }

    #[test]
    fn parenting_points_at_the_node_the_parent_became() {
        let child = {
            let mut layer = solid(2, "child", "#ffffff");
            layer["parent"] = json!(1);
            layer
        };
        let import = import(&lottie(vec![solid(1, "parent", "#ffffff"), child])).expect("converts");
        let child = import
            .comp
            .nodes
            .iter()
            .find(|node| node.id == "child")
            .expect("the child converted");
        assert_eq!(child.parent.as_deref(), Some("parent"));
    }

    #[test]
    fn keyframes_become_a_track_of_fractions_with_the_ease() {
        // An opacity that fades in over the first 30 frames of a 3-second
        // layer, with the classic ease both sides.
        let mut layer = solid(1, "fade", "#ffffff");
        layer["ks"]["o"] = json!({
            "a": 1,
            "k": [
                { "t": 0, "s": [0], "o": {"x": [0.333], "y": [0]}, "i": {"x": [0.667], "y": [1]} },
                { "t": 30, "s": [100] },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        let node = &import.comp.nodes[0];
        let track = node.tracks.get("opacity").expect("an opacity track");
        assert_eq!(track.keys.len(), 2);
        assert_eq!(track.keys[0].at, 0.0);
        assert!((track.keys[1].at - 1.0 / 3.0).abs() < 1e-9);
        let Value::Number { value } = track.keys[0].value else {
            panic!("an opacity key is a number");
        };
        assert_eq!(value, 0.0);
        let Value::Number { value } = track.keys[1].value else {
            panic!("an opacity key is a number");
        };
        assert_eq!(value, 1.0);
        let Some(Curve::Bezier { x1, y1, x2, y2 }) = &track.keys[0].curve else {
            panic!("the ease is a bezier, found {:?}", track.keys[0].curve);
        };
        assert!((*x1 - 0.333).abs() < 1e-9);
        assert!((*y1 - 0.0).abs() < 1e-9);
        assert!((*x2 - 0.667).abs() < 1e-9);
        assert!((*y2 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_hold_keyframe_becomes_a_step() {
        let mut layer = solid(1, "blink", "#ffffff");
        layer["ks"]["o"] = json!({
            "a": 1,
            "k": [
                { "t": 0, "s": [100], "h": 1 },
                { "t": 60, "s": [0] },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        let track = import.comp.nodes[0]
            .tracks
            .get("opacity")
            .expect("an opacity track");
        assert!(matches!(
            &track.keys[0].curve,
            Some(Curve::Steps {
                count: 1,
                position: StepPosition::JumpEnd
            })
        ));
    }

    #[test]
    fn a_curved_motion_path_is_said() {
        let mut layer = solid(1, "drift", "#ffffff");
        layer["ks"]["p"] = json!({
            "a": 1,
            "k": [
                { "t": 0, "s": [100, 100, 0], "to": [50, 0, 0], "ti": [0, 50, 0] },
                { "t": 60, "s": [300, 200, 0] },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        assert!(
            import.notes.iter().any(|note| note.contains("curved path")),
            "{:?}",
            import.notes
        );
        let track = import.comp.nodes[0]
            .tracks
            .get("position")
            .expect("a position track");
        let Value::Point { x, y } = &track.keys[1].value else {
            panic!("a position key is a point");
        };
        assert_eq!((*x, *y), (300.0, 200.0));
    }

    #[test]
    fn a_shape_layer_becomes_a_group_with_painted_children() {
        let layer = json!({
            "ind": 1, "ty": 4, "nm": "badge", "ip": 0, "op": 90, "st": 0,
            "ks": {
                "o": {"a": 0, "k": 100},
                "r": {"a": 0, "k": 0},
                "p": {"a": 0, "k": [256, 256, 0]},
                "a": {"a": 0, "k": [0, 0, 0]},
                "s": {"a": 0, "k": [100, 100, 100]},
            },
            "shapes": [
                { "ty": "gr", "it": [
                    { "ty": "el", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [120, 120]} },
                    { "ty": "fl", "c": {"a": 0, "k": [1, 0.2, 0.4, 1]}, "o": {"a": 0, "k": 100} },
                    { "ty": "tr", "p": {"a": 0, "k": [0, 0]}, "a": {"a": 0, "k": [0, 0]},
                      "s": {"a": 0, "k": [100, 100]}, "r": {"a": 0, "k": 0}, "o": {"a": 0, "k": 100} },
                ] },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        // Three nodes: the layer's group, the `gr` group inside it, and the
        // ellipse the `gr` painted - parentage said once at each level.
        let layer_group = &import.comp.nodes[0];
        let NodeKind::Group { children } = &layer_group.kind else {
            panic!("a shape layer is a group, found {:?}", layer_group.kind);
        };
        assert_eq!(children.len(), 1, "the layer owns the `gr` group alone");
        let inner = import
            .comp
            .nodes
            .iter()
            .find(|node| node.id == children[0])
            .expect("the shape group converted");
        assert_eq!(inner.parent.as_deref(), Some(layer_group.id.as_str()));
        let NodeKind::Group {
            children: inner_children,
        } = &inner.kind
        else {
            panic!("the `gr` is a group, found {:?}", inner.kind);
        };
        assert_eq!(inner_children.len(), 1);
        let shape = import
            .comp
            .nodes
            .iter()
            .find(|node| node.id == inner_children[0])
            .expect("the shape converted");
        assert_eq!(shape.parent.as_deref(), Some(inner.id.as_str()));
        let NodeKind::Ellipse {
            width,
            height,
            fill,
        } = &shape.kind
        else {
            panic!("the shape is an ellipse, found {:?}", shape.kind);
        };
        assert_eq!(width, &Length::Px(120.0));
        assert_eq!(height, &Length::Px(120.0));
        let Some(Paint::Solid { color }) = fill else {
            panic!("the ellipse is painted");
        };
        assert_eq!(color, &Rgba::new(1.0, 0.2, 0.4, 1.0));
        assert!(import.notes.is_empty(), "{:?}", import.notes);
    }

    #[test]
    fn a_rect_shape_keeps_its_roundness() {
        let layer = json!({
            "ind": 1, "ty": 4, "nm": "card", "ip": 0, "op": 90, "st": 0,
            "ks": {
                "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                "s": {"a": 0, "k": [100, 100, 100]},
            },
            "shapes": [
                { "ty": "rc", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [200, 80]}, "r": {"a": 0, "k": 12} },
                { "ty": "fl", "c": {"a": 0, "k": [0, 0, 0, 1]}, "o": {"a": 0, "k": 100} },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        let NodeKind::Rect { radius, .. } = &import.comp.nodes[1].kind else {
            panic!("the shape is a rect, found {:?}", import.comp.nodes[1].kind);
        };
        assert_eq!(*radius, 12.0);
    }

    #[test]
    fn a_paint_over_two_shapes_is_split_and_said() {
        let layer = json!({
            "ind": 1, "ty": 4, "nm": "pair", "ip": 0, "op": 90, "st": 0,
            "ks": {
                "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                "s": {"a": 0, "k": [100, 100, 100]},
            },
            "shapes": [
                { "ty": "el", "p": {"a": 0, "k": [-40, 0]}, "s": {"a": 0, "k": [80, 80]} },
                { "ty": "el", "p": {"a": 0, "k": [40, 0]}, "s": {"a": 0, "k": [80, 80]} },
                { "ty": "fl", "c": {"a": 0, "k": [1, 1, 1, 1]}, "o": {"a": 0, "k": 50} },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        assert_eq!(import.comp.nodes.len(), 3, "the group and both ellipses");
        for node in &import.comp.nodes[1..] {
            let NodeKind::Ellipse { fill, .. } = &node.kind else {
                panic!("both shapes are ellipses");
            };
            let Some(Paint::Solid { color }) = fill else {
                panic!("both are painted");
            };
            assert!(
                (color.a - 0.5).abs() < 1e-9,
                "the paint's opacity folds into the colour"
            );
        }
        assert!(
            import
                .notes
                .iter()
                .any(|note| note.contains("split per shape")),
            "{:?}",
            import.notes
        );
    }

    #[test]
    fn a_path_is_rebased_to_its_own_box() {
        let layer = json!({
            "ind": 1, "ty": 4, "nm": "mark", "ip": 0, "op": 90, "st": 0,
            "ks": {
                "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                "s": {"a": 0, "k": [100, 100, 100]},
            },
            "shapes": [
                { "ty": "sh", "ks": {"a": 0, "k": {
                    "v": [[100, 100], [200, 100], [200, 160]],
                    "i": [[0, 0], [0, 0], [0, 0]],
                    "o": [[0, 0], [0, 0], [0, 0]],
                    "c": true,
                } } },
                { "ty": "fl", "c": {"a": 0, "k": [0, 0, 0, 1]}, "o": {"a": 0, "k": 100} },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        let NodeKind::Path { commands, .. } = &import.comp.nodes[1].kind else {
            panic!("the shape is a path, found {:?}", import.comp.nodes[1].kind);
        };
        // The box runs 100..200 across and 100..160 down, so the vertices
        // re-base to a 100×60 box whose centre the node places at (150, 130)
        // in the group's coordinates.
        assert_eq!(
            import.comp.nodes[1].transform.position,
            Some(Point::new(150.0, 130.0))
        );
        let PathCommand::MoveTo { point } = &commands[0] else {
            panic!("the path starts with a move");
        };
        assert_eq!((point.x, point.y), (0.0, 0.0));
        assert!(matches!(commands.last(), Some(PathCommand::Close)));
    }

    #[test]
    fn an_image_layer_registers_its_asset() {
        let document = json!({
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 60, "w": 512, "h": 512,
            "assets": [
                { "id": "img_0", "w": 128, "h": 96, "u": "images/", "p": "img_0.png" },
            ],
            "layers": [
                { "ind": 1, "ty": 2, "nm": "logo", "ip": 0, "op": 60, "st": 0, "refId": "img_0",
                  "ks": {
                      "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                      "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [64, 48, 0]},
                      "s": {"a": 0, "k": [100, 100, 100]},
                  } },
            ],
        });
        let import = import(&document).expect("converts");
        let NodeKind::Image { asset, fit } = &import.comp.nodes[0].kind else {
            panic!(
                "an image layer becomes an image, found {:?}",
                import.comp.nodes[0].kind
            );
        };
        assert_eq!(asset, "lottie.img-0");
        let registered = import.comp.assets.get(asset).expect("the asset registered");
        assert_eq!(registered.src, "images/img_0.png");
        assert!(matches!(fit, Fit::Contain));
    }

    #[test]
    fn a_precomp_nests_under_a_prefixed_group() {
        let document = json!({
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 90, "w": 512, "h": 512,
            "assets": [
                { "id": "comp_1", "layers": [
                    { "ind": 1, "ty": 1, "nm": "inner", "ip": 0, "op": 90, "st": 0,
                      "sw": 50, "sh": 50, "sc": "#00ff00",
                      "ks": {
                          "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                          "p": {"a": 0, "k": [0, 0, 0]}, "a": {"a": 0, "k": [25, 25, 0]},
                          "s": {"a": 0, "k": [100, 100, 100]},
                      } },
                ] },
            ],
            "layers": [
                { "ind": 1, "ty": 0, "nm": "holder", "ip": 0, "op": 90, "st": 0, "refId": "comp_1",
                  "ks": {
                      "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                      "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                      "s": {"a": 0, "k": [100, 100, 100]},
                  } },
            ],
        });
        let import = import(&document).expect("converts");
        let holder = &import.comp.nodes[0];
        let NodeKind::Group { children } = &holder.kind else {
            panic!("a precomp layer is a group");
        };
        assert_eq!(children.len(), 1);
        let inner = import
            .comp
            .nodes
            .iter()
            .find(|node| node.id == children[0])
            .expect("the inner layer converted");
        // The id carries the holder's prefix, so a second use of the same
        // precomp could not collide with this one.
        assert!(inner.id.starts_with("holder/"), "{:?}", inner.id);
        assert_eq!(inner.parent.as_deref(), Some("holder"));
    }

    #[test]
    fn a_precomp_that_nests_forever_is_stopped_at_the_limit() {
        // A precomp whose only layer is itself: the walk would never end
        // without the depth limit, and the limit is what stops it.
        let document = json!({
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 90, "w": 512, "h": 512,
            "assets": [
                { "id": "ouroboros", "layers": [
                    { "ind": 1, "ty": 0, "nm": "tail", "ip": 0, "op": 90, "st": 0,
                      "refId": "ouroboros",
                      "ks": {
                          "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                          "p": {"a": 0, "k": [0, 0, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                          "s": {"a": 0, "k": [100, 100, 100]},
                      } },
                ] },
            ],
            "layers": [
                { "ind": 1, "ty": 0, "nm": "head", "ip": 0, "op": 90, "st": 0, "refId": "ouroboros",
                  "ks": {
                      "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                      "p": {"a": 0, "k": [0, 0, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                      "s": {"a": 0, "k": [100, 100, 100]},
                  } },
            ],
        });
        match import(&document) {
            // The walk stopped at the limit, every layer above it came back
            // empty, and the comp refuses the nothing it drew.
            Err(AdapterError::Invalid { problems }) => assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("draws nothing")),
                "{problems:?}"
            ),
            other => panic!("an ouroboros is refused, got {other:?}"),
        }
    }

    #[test]
    fn a_document_that_is_not_lottie_is_refused() {
        assert!(import(&json!({"fr": 30})).is_err(), "no layers");
        assert!(
            import(&json!({"layers": [], "w": 0, "h": 0, "fr": 30, "ip": 0, "op": 30})).is_err(),
            "no frame"
        );
        assert!(
            import(&json!({"layers": [{"ind": 1}], "w": 10, "h": 10, "fr": 0, "ip": 0, "op": 30}))
                .is_err(),
            "no fps"
        );
        assert!(
            import(
                &json!({"layers": [{"ind": 1}], "w": 10, "h": 10, "fr": 30, "ip": 30, "op": 30})
            )
            .is_err(),
            "no length"
        );
    }

    #[test]
    fn the_output_lints_by_the_librarys_own_rules() {
        // The adapters hold themselves to the library's bar: a conversion
        // with no notes lints clean, and the notes there are name every
        // approximation the lint would otherwise hide.
        let layer = json!({
            "ind": 1, "ty": 4, "nm": "clean", "ip": 0, "op": 90, "st": 0,
            "ks": {
                "o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                "s": {"a": 0, "k": [100, 100, 100]},
            },
            "shapes": [
                { "ty": "rc", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [220, 90]}, "r": {"a": 0, "k": 8} },
                { "ty": "fl", "c": {"a": 0, "k": [0.1, 0.2, 0.3, 1]}, "o": {"a": 0, "k": 100} },
            ],
        });
        let import = import(&lottie(vec![layer])).expect("converts");
        assert!(import.notes.is_empty(), "{:?}", import.notes);
        let findings = concat_motion::lint::audit(&import.comp, false);
        assert!(
            findings.is_empty(),
            "a clean conversion lints clean: {findings:?}"
        );
    }
}
