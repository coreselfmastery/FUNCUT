// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The motion layer's door on the MCP: the five verbs an agent needs to
//! design with it - what presets exist, what is in a comp, one frame to
//! look at, a bake to place, and a word-timed caption reel - each a thin
//! wrapper over `concat-motion` rather than a second implementation.
//!
//! Comps live with the project: a comp a tool makes is written to
//! `<project>/comps/<hash>.json`, its frames bake into
//! `<project>/cache/comps/<hash>-<w>x<h>@<fps>/`, and an encoded piece
//! lands in `<project>/motion/` ready for `media_import` and `clip_add`.
//! The project is only a folder to these tools - no session, no lock -
//! because a comp is a document, not an edit.
//!
//! The engine is Geneva, the browserless markup renderer; the honest
//! refusal when it is not on the machine names the installer. Encoding
//! goes through Geneva's own encoder, which is 4:2:0 without an alpha
//! channel: a comp with a transparent background is opaque in the file,
//! and the note in the bake reply says so. Overlays that must keep their
//! alpha wait for the Motion clip, which bakes PNG frames the compositor
//! samples; until then a plate (a still the captions render over) is the
//! way to a finished caption video in one call.

use std::path::{Path, PathBuf};

use concat_motion::authoring::{self, CompSummary};
use concat_motion::comp::{AssetKind, Comp, NodeKind, Paint, Rgba};
use concat_motion::engine::{BakeRequest, CompEngine, EnginePreference};
use concat_motion::geneva::Geneva;
use concat_motion::presets;
use concat_motion::sequence::{self, SceneLayer};
use concat_motion::storyboard;
use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::tools::Outcome;
use concat_api::{ApiError, ErrorCode};

/// Where in the project the motion layer works: comps, their bake cache,
/// and the encoded pieces ready to import.
fn comps_dir(project: &Path) -> PathBuf {
    project.join("comps")
}

fn cache_dir(project: &Path) -> PathBuf {
    project.join("cache").join("comps")
}

fn motion_dir(project: &Path) -> PathBuf {
    project.join("motion")
}

/// The sidecar, or the refusal that says where it comes from.
fn geneva() -> Result<Geneva, ApiError> {
    concat_motion::geneva::Geneva::discover(EnginePreference::default().geneva.as_deref())
        .ok_or_else(|| {
            ApiError::failed(
                "the Geneva motion sidecar is not on this machine - the app's installer \
                 brings it (install-linux.sh), or set CONCAT_GENEVA_BIN to its path"
                    .to_owned(),
            )
        })
}

/// A project folder argument: present, a folder, and without `..` in it.
/// When the session knows where it writes (the headless backend's roots),
/// the folder must be inside one of them: the motion tools write into the
/// project - comps/, cache/, motion/ - and a project anywhere on the disk
/// was a way to make those writes land anywhere on the disk. An empty
/// roots list is the window backend, whose server holds its own line and
/// refuses there.
fn project_of(fields: &Map<String, Value>, backend: &dyn Backend) -> Result<PathBuf, ApiError> {
    let path = fields
        .get("project")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| ApiError::invalid("invalid arguments: `project` is required"))?;
    if Path::new(path).components().any(|part| {
        matches!(
            part,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return Err(ApiError::invalid(
            "invalid arguments: `project` may not hold `..` or a drive",
        ));
    }
    if !Path::new(path).is_dir() {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("no project folder at {path}"),
        ));
    }
    let roots = backend.roots();
    if !roots.is_empty() {
        // The same resolution the sense layer's write_target does: what
        // exists is canonicalised, and what does not is compared
        // lexically past its deepest existing ancestor, so a symlink
        // cannot quietly move the folder out of a root.
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
        if !roots.iter().any(|root| {
            resolved.starts_with(root)
                || root
                    .canonicalize()
                    .is_ok_and(|real| resolved.starts_with(real))
        }) {
            let list = roots
                .iter()
                .map(|root| root.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApiError::invalid(format!(
                "invalid arguments: {path} is outside where this session works ({list})"
            )));
        }
    }
    Ok(PathBuf::from(path))
}

/// The comp an argument set names: a preset id, or a file under the
/// project's `comps/`, optionally edited by one batch of operations first.
fn resolve_comp(fields: &Map<String, Value>) -> Result<(Comp, Option<PathBuf>), ApiError> {
    let preset = fields.get("preset").and_then(Value::as_str);
    let comp = fields.get("comp").and_then(Value::as_str);
    let project = fields
        .get("project")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let (mut comp, file) = match (preset, comp) {
        (Some(id), _) => (
            presets::build(id).ok_or_else(|| {
                ApiError::new(ErrorCode::NotFound, format!("no preset called {id:?}"))
            })?,
            None,
        ),
        (None, Some(name)) => {
            if name.contains("..") || name.starts_with('/') {
                return Err(ApiError::invalid(
                    "invalid arguments: `comp` is a file name under the project's comps/, \
                     not a path out of it",
                ));
            }
            let project = project.ok_or_else(|| {
                ApiError::invalid("invalid arguments: reading a comp needs `project`")
            })?;
            let file = comps_dir(&project).join(name);
            if !file.is_file() {
                return Err(ApiError::new(
                    ErrorCode::NotFound,
                    format!("no comp at {}", file.display()),
                ));
            }
            let text = std::fs::read_to_string(&file)
                .map_err(|error| ApiError::failed(format!("{}: {error}", file.display())))?;
            let comp: Comp = serde_json::from_str(&text)
                .map_err(|error| ApiError::failed(format!("{}: {error}", file.display())))?;
            (comp, Some(file))
        }
        (None, None) => {
            return Err(ApiError::invalid(
                "invalid arguments: name a `preset` or a `comp` file",
            ));
        }
    };
    if let Some(ops) = fields.get("ops").filter(|value| !value.is_null()) {
        comp = authoring::apply_json(&comp, ops)
            .map_err(|error| ApiError::failed(error.to_string()))?;
    }
    Ok((comp, file))
}

/// The summary an agent plans with: what a comp holds, moves and can be
/// told, and whether it needs the markup engine.
fn summary_of(comp: &Comp) -> Value {
    let summary = CompSummary::of(comp);
    let mut value = serde_json::to_value(&summary).unwrap_or_else(|_| json!({}));
    let problems = comp.validate();
    if !problems.is_empty() {
        value["problems"] = json!(
            problems
                .iter()
                .map(|problem| {
                    let mut said = format!("{}: {}", problem.path, problem.message);
                    if let Some(help) = &problem.help {
                        said.push_str(&format!(" - {help}"));
                    }
                    said
                })
                .collect::<Vec<String>>()
        );
    }
    value
}

/// `motion_presets`: the library - every preset with its category, size,
/// duration, slots and whether the sidecar is there to bake it.
pub fn presets_list() -> Result<Outcome, ApiError> {
    let engine = Geneva::discover(EnginePreference::default().geneva.as_deref());
    let mut list = Vec::with_capacity(presets::ALL.len());
    for preset in presets::ALL {
        let Some(comp) = presets::build(preset.id) else {
            continue;
        };
        let summary = CompSummary::of(&comp);
        list.push(json!({
            "id": preset.id,
            "name": preset.name,
            "category": preset.category.as_str(),
            "engine": preset.engine.as_str(),
            "width": summary.width,
            "height": summary.height,
            "duration": summary.duration,
            "slots": summary.slots.iter().map(|(label, node, field)| json!({
                "label": label, "node": node, "field": field,
            })).collect::<Vec<_>>(),
            "nodes": summary.nodes.iter().map(|node| json!({
                "id": node.id, "kind": node.kind, "start": node.start,
                "duration": node.duration, "tracks": node.tracks,
            })).collect::<Vec<_>>(),
        }));
    }
    Ok(Outcome::json(json!({
        "presets": list,
        "geneva": match &engine {
            Some(geneva) => {
                let version = geneva.version().unwrap_or_default();
                json!({ "available": true, "version": version })
            }
            None => json!({ "available": false }),
        },
    })))
}

/// `motion_inspect`: one comp - a preset, or a file under the project's
/// `comps/` - with an optional batch of operations applied first, so the
/// edit a bake would carry can be checked before it runs.
pub fn inspect(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    let known = ["preset", "comp", "project", "ops"];
    for key in fields.keys() {
        if !known.contains(&key.as_str()) {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let (comp, file) = resolve_comp(&fields)?;
    let mut reply = json!({ "comp": summary_of(&comp) });
    if let Some(file) = file {
        reply["file"] = json!(file.display().to_string());
    }
    Ok(Outcome::json(reply))
}

/// `scene_analysis`: the comp read as an engineer reads it, before a
/// frame is drawn - `motion_inspect` says *what* a comp holds; this says
/// *how well it is built*. The census (nodes by kind, animated against
/// static, keyframes and the curves they ride, nesting depth), the timing
/// (first start, last end, the fraction of the comp's span anything is
/// on screen, the gaps where nothing is), the text (sizes against the
/// frame, the fonts, karaoke, plates, outlines, shadows, gradient ink),
/// per-text legibility (contrast of the ink against the plate or the
/// background it actually draws over, WCAG-shaped and honestly null when
/// the ink is a gradient over video), the assets against the disk, and
/// the lint's own findings with their stable `L###` codes. No engine, no
/// render, no write: everything here is read off the document, so it
/// costs nothing and refuses nothing - it is the critique that happens
/// before the pixels do.
pub fn scene_analysis(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    let known = ["preset", "comp", "project", "ops"];
    for key in fields.keys() {
        if !known.contains(&key.as_str()) {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let (comp, file) = resolve_comp(&fields)?;
    // The asset family reads the disk, and the disk it reads is the
    // folder the comp file lives in - the same place a bake stages from.
    // A preset has no folder, so its assets are reported as unverified
    // rather than checked against a directory that is not there.
    let source = file
        .as_ref()
        .and_then(|file| file.parent().map(Path::to_path_buf));
    let mut reply = json!({ "analysis": analyse(&comp, source.as_deref()) });
    if let Some(file) = file {
        reply["file"] = json!(file.display().to_string());
    }
    Ok(Outcome::json(reply))
}

/// The analysis itself: every number `scene_analysis` answers, walked off
/// the document. Kept private because it is the tool's own shape, not a
/// contract another crate reads.
fn analyse(comp: &Comp, source: Option<&Path>) -> Value {
    let duration = comp.duration.max(0.0);

    // ── The census ────────────────────────────────────────────────────
    let mut by_kind: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut curves: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut keys = 0usize;
    let mut animated = 0usize;
    let mut effects = 0usize;
    let mut parents: std::collections::BTreeMap<&str, Option<&str>> =
        std::collections::BTreeMap::new();
    let mut depth = 0usize;
    for node in &comp.nodes {
        let kind = match &node.kind {
            NodeKind::Group { .. } => "group",
            NodeKind::Rect { .. } => "rect",
            NodeKind::Ellipse { .. } => "ellipse",
            NodeKind::Path { .. } => "path",
            NodeKind::Adjustment => "adjustment",
            NodeKind::Text { .. } => "text",
            NodeKind::Html { .. } => "html",
            NodeKind::Image { .. } => "image",
            NodeKind::Lottie { .. } => "lottie",
            NodeKind::Composition { .. } => "composition",
        };
        *by_kind.entry(kind).or_default() += 1;
        parents.insert(node.id.as_str(), node.parent.as_deref());
        effects += node.effects.len();
        if node.tracks.is_empty() {
            continue;
        }
        animated += 1;
        for track in node.tracks.values() {
            keys += track.keys.len();
            for key in &track.keys {
                if let Some(curve) = &key.curve {
                    *curves.entry(curve.css()).or_default() += 1;
                }
            }
        }
    }
    // Nesting depth, the honest way: walk each node's parent chain, with
    // the engine's own cap (32) so a document that is a loop is a
    // finding, not a hang here too.
    for node in &comp.nodes {
        let mut walked = 0usize;
        let mut parent = node.parent.as_deref();
        while let Some(id) = parent {
            walked += 1;
            if walked > 32 {
                break;
            }
            parent = parents.get(id).copied().flatten();
        }
        depth = depth.max(walked);
    }

    // ── The timing ────────────────────────────────────────────────────
    // The union of the node windows, exactly: sort the spans, merge what
    // overlaps, and the gaps fall out as what is left of [0, duration].
    let mut spans: Vec<(f64, f64)> = comp
        .nodes
        .iter()
        .map(|node| (node.start, node.end(duration)))
        .filter(|(from, to)| to > from)
        .collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut covered: Vec<(f64, f64)> = Vec::new();
    for (from, to) in spans {
        match covered.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => covered.push((from, to)),
        }
    }
    let on_screen: f64 = covered.iter().map(|(from, to)| to - from).sum();
    let mut gaps: Vec<Value> = Vec::new();
    let mut at = 0.0;
    for (from, to) in &covered {
        if from - at >= 0.05 {
            gaps.push(json!({ "from": at, "to": *from }));
        }
        at = at.max(*to);
    }
    if duration - at >= 0.05 {
        gaps.push(json!({ "from": at, "to": duration }));
    }
    let first_start = comp
        .nodes
        .iter()
        .map(|node| node.start)
        .fold(f64::INFINITY, f64::min);
    let last_end = comp
        .nodes
        .iter()
        .map(|node| node.end(duration))
        .fold(f64::NEG_INFINITY, f64::max);

    // ── The text, and its legibility ──────────────────────────────────
    let mut sizes: Vec<f64> = Vec::new();
    let mut fonts: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut karaoke = 0usize;
    let mut plates = 0usize;
    let mut outlines = 0usize;
    let mut shadows = 0usize;
    let mut gradient_ink = 0usize;
    let mut readability: Vec<Value> = Vec::new();
    for node in &comp.nodes {
        let NodeKind::Text(text) = &node.kind else {
            continue;
        };
        if !text.words.is_empty() {
            karaoke += 1;
        }
        if text.background.a > 0.0 {
            plates += 1;
        }
        if text.outline.is_some() {
            outlines += 1;
        }
        if text.shadow.is_some() {
            shadows += 1;
        }
        if text.fill.is_some() {
            gradient_ink += 1;
        }
        if !text.font.is_empty() {
            fonts.insert(text.font.clone());
        }
        sizes.push(text.size);
        // The backdrop the words actually draw over: their own plate when
        // it is opaque enough to matter, else the comp's background when
        // it is, else video nobody can read from the document.
        let backdrop = if text.background.a > 0.5 {
            Some(text.background)
        } else if comp.background.a > 0.5 {
            Some(comp.background)
        } else {
            None
        };
        // The ink: a flat colour answers with one ratio, a gradient with
        // its worst stop (the ramp is only as legible as its quietest
        // moment), and no backdrop leaves the ratio honestly null.
        let inks: Vec<&Rgba> = match &text.fill {
            Some(Paint::Solid { color }) => vec![color],
            Some(Paint::Gradient { stops, .. }) => stops.iter().map(|(_, color)| color).collect(),
            None => vec![&text.color],
        };
        let contrast = backdrop.and_then(|backdrop| {
            inks.iter().map(|ink| contrast_ratio(ink, &backdrop)).fold(
                None::<f64>,
                |worst, ratio| match worst {
                    None => Some(ratio),
                    Some(worst) => Some(worst.min(ratio)),
                },
            )
        });
        let as_percent = if comp.height > 0 {
            text.size / f64::from(comp.height) * 100.0
        } else {
            0.0
        };
        let mut verdict = "ok";
        if as_percent < 3.0 {
            verdict = "small";
        }
        if let Some(ratio) = contrast
            && ratio < 3.0
        {
            verdict = if as_percent < 3.0 {
                "small-and-low-contrast"
            } else {
                "low-contrast"
            };
        }
        readability.push(json!({
            "node": node.id,
            "size": text.size,
            "asPercentOfHeight": (as_percent * 100.0).round() / 100.0,
            "alpha": text.color.a,
            "contrast": contrast.map(|ratio| (ratio * 100.0).round() / 100.0),
            "backdrop": backdrop.is_some(),
            "verdict": verdict,
        }));
    }
    let min_size = sizes.iter().copied().fold(f64::INFINITY, f64::min);
    let max_size = sizes.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    // ── The assets, against the disk ─────────────────────────────────
    let mut assets: Vec<Value> = Vec::new();
    for (id, asset) in &comp.assets {
        let kind = asset
            .kind
            .as_ref()
            .map(|kind| match kind {
                AssetKind::Image => "image",
                AssetKind::Video => "video",
                AssetKind::Audio => "audio",
                AssetKind::Font => "font",
                AssetKind::Lottie => "lottie",
                AssetKind::Html => "html",
            })
            .map(str::to_owned);
        let off_machine = asset.src.starts_with("http:")
            || asset.src.starts_with("https:")
            || asset.src.starts_with("//");
        let present = if off_machine {
            None
        } else {
            source.map(|source| source.join(&asset.src).is_file())
        };
        assets.push(json!({
            "id": id,
            "kind": kind,
            "src": asset.src,
            "present": present,
        }));
    }

    // ── The lint, with its stable codes ───────────────────────────────
    let mut findings: Vec<Value> = comp_lint(comp, source).iter().map(finding_json).collect();
    if findings.is_empty() {
        findings = vec![json!({
            "code": "clean",
            "family": "none",
            "path": "/",
            "message": "The lint has nothing to say: no finding in any of its five families.",
        })];
    }

    json!({
        "comp": {
            "id": comp.id,
            "name": comp.name,
            "width": comp.width,
            "height": comp.height,
            "fps": comp.fps,
            "duration": comp.duration,
            "needsMarkup": comp.needs_markup(),
            "slots": comp.slots.len(),
        },
        "census": {
            "nodes": comp.nodes.len(),
            "byKind": by_kind,
            "animated": animated,
            "static": comp.nodes.len() - animated,
            "keys": keys,
            "curves": curves,
            "depth": depth,
            "effects": effects,
        },
        "timing": {
            "firstStart": if first_start.is_finite() { json!(first_start) } else { Value::Null },
            "lastEnd": if last_end.is_finite() { json!(last_end) } else { Value::Null },
            "coverage": if duration > 0.0 {
                json!((on_screen / duration * 100.0).round() / 100.0)
            } else {
                Value::Null
            },
            "gaps": gaps,
        },
        "text": {
            "nodes": sizes.len(),
            "minSize": if min_size.is_finite() { json!(min_size) } else { Value::Null },
            "maxSize": if max_size.is_finite() { json!(max_size) } else { Value::Null },
            "fonts": fonts,
            "karaoke": karaoke,
            "plates": plates,
            "outlines": outlines,
            "shadows": shadows,
            "gradientInk": gradient_ink,
        },
        "readability": readability,
        "assets": assets,
        "assetsChecked": source.is_some(),
        "lint": findings,
        "note": "Read this before a frame is drawn: the small and the low-contrast are the \
                 retakes a render would have to tell you, the gaps are the dead air, and the \
                 lint's codes are stable - L1xx structure, L2xx timing, L3xx assets, L4xx \
                 motion, L5xx determinism. Nothing here refused anything; everything here is \
                 advice about a document you can still change for free.",
    })
}

/// The lint's findings for one comp: the four families that read the
/// document, plus the asset family when there is a folder to read. The
/// `playful` voice is off - an agent wants the sentence, not the wink.
fn comp_lint(comp: &Comp, source: Option<&Path>) -> Vec<concat_motion::lint::Finding> {
    let mut found = concat_motion::lint::audit(comp, false);
    if let Some(source) = source {
        found.extend(concat_motion::lint::audit_assets(comp, source));
    }
    found
}

/// One finding, as the tool answers it: the code, the family the code's
/// prefix names, the place, the sentence, and the help when there is one.
fn finding_json(finding: &concat_motion::lint::Finding) -> Value {
    let family = match finding.code.as_bytes().first() {
        Some(b'L') => match finding.code.as_bytes().get(1) {
            Some(b'1') => "structure",
            Some(b'2') => "timing",
            Some(b'3') => "assets",
            Some(b'4') => "motion",
            Some(b'5') => "determinism",
            _ => "unknown",
        },
        _ => "unknown",
    };
    let mut value = json!({
        "code": finding.code,
        "family": family,
        "path": finding.path,
        "message": finding.message,
    });
    if let Some(help) = &finding.help {
        value["help"] = json!(help);
    }
    value
}

/// The WCAG-shaped contrast ratio of an ink over a backdrop: relative
/// luminance of each, the lighter over the darker, both plus 0.05 - the
/// 1.0 a grey draws on its own grey, the 21.0 black on white. A shape
/// video borrows for want of its own: a caption's legibility against a
/// plate is the same arithmetic a page's is against a background.
fn contrast_ratio(ink: &Rgba, backdrop: &Rgba) -> f64 {
    let lighter = luminance(ink).max(luminance(backdrop));
    let darker = luminance(ink).min(luminance(backdrop));
    (lighter + 0.05) / (darker + 0.05)
}

/// Relative luminance, the WCAG shape: linearised channels, weighted,
/// alpha ignored (the caller already chose an opaque-enough backdrop).
fn luminance(color: &Rgba) -> f64 {
    let channel = |c: f64| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

/// `motion_frame`: one frame of a comp at `at` seconds, drawn in tens of
/// milliseconds, written under the project's `motion/` and returned as a
/// path - the look-before-you-bake of the verification loop.
pub fn frame(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        // No `height` here, on purpose: the frame draws at the comp's own
        // aspect, and a height the engine cannot honour is a promise the
        // tool must not make - `motion_bake` takes one because it renders
        // a whole clip at the project's frame; this draws one frame.
        if !["preset", "comp", "project", "ops", "at", "width"].contains(&key.as_str()) {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let project = project_of(&fields, backend)?;
    let at = fields
        .get("at")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| {
            ApiError::invalid("invalid arguments: `at` is a time in seconds, 0 or more")
        })?;
    let (comp, _) = resolve_comp(&fields)?;
    let engine = geneva()?;
    let width = fields
        .get("width")
        .and_then(Value::as_u64)
        .map(|w| w as u32);
    std::fs::create_dir_all(motion_dir(&project))
        .map_err(|error| ApiError::failed(format!("could not create motion/: {error}")))?;
    let drawn = engine
        .frame(&comp, at, width, &comps_dir(&project))
        .map_err(|error| ApiError::failed(error.to_string()))?;
    let out = motion_dir(&project).join(format!("frame-{:016x}-{at:.2}.png", comp.hash(),));
    // Read the drawn PNG before it is removed: an agent cannot open a path,
    // so the frame has to travel back as MCP `type: image` content. This is
    // what makes the look-before-you-bake design loop actually work.
    let bytes = std::fs::read(&drawn)
        .map_err(|error| ApiError::failed(format!("could not read the drawn frame: {error}")))?;
    std::fs::copy(&drawn, &out)
        .map_err(|error| ApiError::failed(format!("could not keep the frame: {error}")))?;
    let _ = std::fs::remove_file(&drawn);
    let inline = crate::sense::base64(&bytes);
    let reading = json!({
        "at": at,
        "frame": out.display().to_string(),
        "duration": comp.duration,
        "width": width.unwrap_or(comp.width),
        "height": comp.height,
        "note": "Look at this frame. Check legibility, contrast, safe margins and whether the motion is mid-entrance or settled before baking.",
    });
    Ok(Outcome {
        content: vec![
            json!({ "type": "image", "data": inline, "mimeType": "image/png" }),
            json!({
                "type": "text",
                "text": serde_json::to_string_pretty(&reading).unwrap_or_default(),
            }),
        ],
        structured: Some(reading),
        is_error: false,
    })
}

/// The project's frame, from its document: the `video` a `concat.json`
/// manifest holds, when one can be read. None keeps the comp's own size
/// - the 1920x1080 canvas every comp is authored on.
fn frame_of(project: &Path) -> Option<(u32, u32)> {
    let manifest = std::fs::read_to_string(project.join("concat.json")).ok()?;
    let document: Value = serde_json::from_str(&manifest).ok()?;
    let video = document.get("video")?;
    Some((
        video.get("width")?.as_u64()? as u32,
        video.get("height")?.as_u64()? as u32,
    ))
}

/// `motion_preview`: the comp's motion, watched - `frames` stills spread
/// evenly across the comp's duration (never the dead last instant, where
/// an exit has usually already gone), drawn small and answered as one
/// document of inline images in time order. `motion_frame` is one exact
/// moment; this is the whole span at a glance - the entrance, the hold,
/// the exit - for the cost of a handful of frames instead of a bake.
pub fn preview(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if !["preset", "comp", "project", "ops", "frames", "width"].contains(&key.as_str()) {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let project = project_of(&fields, backend)?;
    // The spread: how many stills, and how wide each draws. The count is
    // bounded because the point is a glance, not a flipbook; the width is
    // bounded because a preview that costs what a bake costs has missed
    // its own name.
    let frames = fields.get("frames").and_then(Value::as_u64).unwrap_or(6);
    if !(1..=12).contains(&frames) {
        return Err(ApiError::invalid(
            "invalid arguments: `frames` is 1 to 12 - six is the glance, twelve the strip",
        ));
    }
    let width = fields.get("width").and_then(Value::as_u64).unwrap_or(480);
    if !(16..=640).contains(&width) {
        return Err(ApiError::invalid(
            "invalid arguments: `width` is 16 to 640 - a preview, not a bake",
        ));
    }
    let (comp, _) = resolve_comp(&fields)?;
    if comp.duration <= 0.0 || !comp.duration.is_finite() {
        return Err(ApiError::invalid(
            "invalid arguments: the comp has no duration to spread frames across",
        ));
    }
    let engine = geneva()?;
    std::fs::create_dir_all(motion_dir(&project))
        .map_err(|error| ApiError::failed(format!("could not create motion/: {error}")))?;

    // One sixth of the way in is the first sample, five sixths the last:
    // the samples walk [0, duration) without ever landing on the frame
    // after everything has left.
    let mut content = Vec::with_capacity(frames as usize + 1);
    let mut said = Vec::with_capacity(frames as usize);
    for index in 0..frames {
        let at = comp.duration * f64::from(index as u32) / f64::from(frames as u32);
        let drawn = engine
            .frame(&comp, at, Some(width as u32), &comps_dir(&project))
            .map_err(|error| ApiError::failed(error.to_string()))?;
        // Read the drawn PNG before it is removed: an agent cannot open a
        // path, so each sample travels back as MCP `type: image` content,
        // in time order - the flipbook the reply is.
        let bytes = std::fs::read(&drawn).map_err(|error| {
            ApiError::failed(format!("could not read the drawn frame: {error}"))
        })?;
        let out = motion_dir(&project).join(format!(
            "preview-{:016x}-{}-{at:.2}.png",
            comp.hash(),
            index + 1
        ));
        std::fs::copy(&drawn, &out)
            .map_err(|error| ApiError::failed(format!("could not keep the frame: {error}")))?;
        let _ = std::fs::remove_file(&drawn);
        content.push(json!({
            "type": "image",
            "data": crate::sense::base64(&bytes),
            "mimeType": "image/png",
        }));
        said.push(json!({
            "index": index + 1,
            "at": (at * 100.0).round() / 100.0,
            "frame": out.display().to_string(),
        }));
    }
    let reading = json!({
        "frames": said,
        "count": frames,
        "duration": comp.duration,
        "width": width,
        "height": comp.height,
        "note": "Watch these in order: the entrance should land on its curve, the hold \
                 should breathe, the exit should leave. Compare the first against the \
                 last before motion_bake pays for the whole clip; motion_frame draws one \
                 exact time, scene_analysis reads the document with no engine at all.",
    });
    content.push(json!({
        "type": "text",
        "text": serde_json::to_string_pretty(&reading).unwrap_or_default(),
    }));
    Ok(Outcome {
        content,
        structured: Some(reading),
        is_error: false,
    })
}

/// `motion_import`: motion made elsewhere, read here. Two doors, one
/// rule - nothing converts silently.
///
/// `action: "lottie"` turns a bodymovin JSON (what After Effects exports,
/// what LottieFiles hands over) into a comp under the project's `comps/`:
/// solids, images, shape layers, grouped and nested transforms,
/// keyframed properties with their eases, parenting and in/out points.
/// `action: "gsap"` applies a GSAP-style timeline to a comp the project
/// already holds (or a preset), writing the animated result as a new
/// comp: tweens on x, y, scale, rotation, opacity and blur, with delay,
/// stagger, repeat and yoyo.
///
/// Both answer with the comp's summary (the same `motion_inspect` read)
/// and the adapter's notes: everything the conversion skipped,
/// approximated or clamped, in the adapter's own words. A conversion
/// with no notes lost nothing the source asked for; one with `skipped:`
/// notes draws less than the source did, and the caller decides whether
/// that is acceptable. The comp validates and lints by the same rules a
/// hand-written one does before it is written, so an import that cannot
/// bake says so here rather than at the oven.
pub fn import(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    let known = [
        "action", "project", "file", "name", "preset", "comp", "timeline",
    ];
    for key in fields.keys() {
        if !known.contains(&key.as_str()) {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let action = fields
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::invalid("invalid arguments: `action` is `lottie` or `gsap`"))?;
    let project = fields
        .get("project")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| {
            ApiError::invalid("invalid arguments: `project` names the folder the comp lands in")
        })?;
    if !project.is_dir() {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("no project at {}", project.display()),
        ));
    }
    // The name the comp is written under: a file name inside comps/,
    // never a path out of it. Defaulted from the source, and `.json` is
    // added when the caller left it off - a name is a name, not a
    // format.
    let clean_name = |name: &str| -> Result<String, ApiError> {
        if name.contains("..") || name.starts_with('/') || name.is_empty() {
            return Err(ApiError::invalid(
                "invalid arguments: `name` is a file name under the project's comps/, not a path out of it",
            ));
        }
        Ok(if name.ends_with(".json") {
            name.to_owned()
        } else {
            format!("{name}.json")
        })
    };
    let (comp, notes, name, from) = match action {
        "lottie" => {
            let file = fields
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ApiError::invalid(
                        "invalid arguments: `file` is the Lottie JSON to read (a .json bodymovin export)",
                    )
                })?;
            let text = std::fs::read_to_string(file).map_err(|error| {
                ApiError::new(
                    ErrorCode::NotFound,
                    format!("could not read {file}: {error}"),
                )
            })?;
            let document: Value = serde_json::from_str(&text)
                .map_err(|error| ApiError::invalid(format!("{file} is not JSON: {error}")))?;
            // A JSON file that is not a Lottie document refuses here
            // rather than converting into an empty comp: the adapter's
            // own refusal words carry the detail.
            let imported = geneva_adapter::lottie::import(&document).map_err(|error| {
                ApiError::invalid(format!("the Lottie document does not convert: {error}"))
            })?;
            let name = clean_name(fields.get("name").and_then(Value::as_str).unwrap_or_else(
                || {
                    Path::new(file)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("imported")
                },
            ))?;
            (imported.comp, imported.notes, name, file.to_owned())
        }
        "gsap" => {
            let timeline = fields
                .get("timeline")
                .filter(|value| !value.is_null())
                .ok_or_else(|| {
                    ApiError::invalid(
                        "invalid arguments: `timeline` is the GSAP-style timeline JSON to apply",
                    )
                })?;
            // The comp the timeline animates: a preset, or a file under
            // the project's comps/ - the same resolution every other
            // comp tool makes, `ops` included. resolve_comp refuses any
            // field but its own four, so it is handed exactly those.
            let mut naming = Map::new();
            for key in ["preset", "comp", "project", "ops"] {
                if let Some(value) = fields.get(key) {
                    naming.insert(key.to_owned(), value.clone());
                }
            }
            let (base, _source) = resolve_comp(&naming)?;
            let (comp, applied) =
                geneva_adapter::gsap::apply(&base, timeline).map_err(|error| {
                    ApiError::invalid(format!("the timeline does not apply: {error}"))
                })?;
            let default = match (fields.get("comp"), fields.get("preset")) {
                (Some(named), _) => format!(
                    "{}-animated",
                    Path::new(named.as_str().unwrap_or_default())
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("comp")
                ),
                (_, Some(preset)) => format!("{}-animated", preset.as_str().unwrap_or("comp")),
                _ => "animated".to_owned(),
            };
            let name = clean_name(
                fields
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&default),
            )?;
            let from = fields
                .get("comp")
                .and_then(Value::as_str)
                .or_else(|| fields.get("preset").and_then(Value::as_str))
                .unwrap_or("a comp")
                .to_owned();
            (comp, applied, name, from)
        }
        _ => {
            return Err(ApiError::invalid(
                "invalid arguments: `action` is `lottie` or `gsap`",
            ));
        }
    };
    // The library's own bar, before anything is written: a comp that
    // does not validate is refused with its problems named, so a broken
    // import never reaches the bake.
    let problems = comp.validate();
    if !problems.is_empty() {
        let said: Vec<String> = problems
            .iter()
            .map(|problem| {
                format!(
                    "{} at {}{}",
                    problem.message,
                    problem.path,
                    problem
                        .help
                        .as_ref()
                        .map(|help| format!(" ({help})"))
                        .unwrap_or_default()
                )
            })
            .collect();
        return Err(ApiError::failed(format!(
            "the converted comp does not validate: {}",
            said.join("; ")
        )));
    }
    std::fs::create_dir_all(comps_dir(&project))
        .map_err(|error| ApiError::failed(format!("could not create comps/: {error}")))?;
    let target = comps_dir(&project).join(&name);
    let text = serde_json::to_string_pretty(&comp)
        .map_err(|error| ApiError::failed(format!("could not serialise the comp: {error}")))?;
    std::fs::write(&target, text)
        .map_err(|error| ApiError::failed(format!("{}: {error}", target.display())))?;
    let skipped = notes
        .iter()
        .filter(|note| note.starts_with("skipped: "))
        .count();
    let reply = json!({
        "ok": true,
        "action": action,
        "from": from,
        "file": target.display().to_string(),
        "name": name,
        "comp": summary_of(&comp),
        "notes": notes,
        "skipped": skipped,
        "next": "motion_frame to see it, motion_bake to render it, ops to edit it",
    });
    Ok(Outcome::json(reply))
}

/// `motion_bake`: a comp - a preset or a file, edited by `ops` - baked to
/// PNG frames in the project's cache and, when `encode` is set, written as
/// one video file under `motion/` ready for `media_import`.
pub fn bake(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if ![
            "project", "preset", "comp", "ops", "width", "height", "fps", "crf", "encode", "name",
        ]
        .contains(&key.as_str())
        {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let project = project_of(&fields, backend)?;
    let (comp, _) = resolve_comp(&fields)?;
    let width = fields
        .get("width")
        .and_then(Value::as_u64)
        .map(|w| w as u32);
    let height = fields
        .get("height")
        .and_then(Value::as_u64)
        .map(|h| h as u32);
    let fps = fields
        .get("fps")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 240.0);
    let crf = fields
        .get("crf")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(0, 51) as u8)
        .unwrap_or(18);
    // A bake that names no size lands at the project's own frame, read
    // from its document: the comps are authored on a 1920x1080 canvas,
    // which used to be the fallback, and a vertical project baked
    // horizontal. A document that cannot be read keeps the comp's size.
    let frame = frame_of(&project);
    let width = width.or(frame.map(|(width, _)| width));
    let height = height.or(frame.map(|(_, height)| height));
    // The tool description has promised "never above 4K for safety" since
    // the beginning, and a 4-6 GB laptop has no business rendering 8K PNG
    // frames - now the clamp actually exists, and the schema agrees at
    // 4096. A clamped bake says so in the reply instead of silently
    // rendering smaller than asked.
    let asked = (width, height);
    let width = width.map(|value| value.min(4096));
    let height = height.map(|value| value.min(4096));

    // The comp lands in the project, content-addressed: two bakes of the
    // same comp share one file, and an edit is a new hash by itself.
    let hash = format!("{:016x}", comp.hash());
    std::fs::create_dir_all(comps_dir(&project))
        .map_err(|error| ApiError::failed(format!("could not create comps/: {error}")))?;
    let comp_file = comps_dir(&project).join(format!("{hash}.json"));
    let text = serde_json::to_string_pretty(&comp)
        .map_err(|error| ApiError::failed(format!("the comp would not serialise: {error}")))?;
    std::fs::write(&comp_file, text)
        .map_err(|error| ApiError::failed(format!("{}: {error}", comp_file.display())))?;

    let request = BakeRequest {
        width,
        height,
        fps,
        alpha: true,
        crf,
        frames: None,
        source_dir: comps_dir(&project),
    };
    let dir = cache_dir(&project).join(format!(
        "{hash}-{}x{}@{}",
        request.width(&comp),
        request.height(&comp),
        request.fps(&comp),
    ));
    let engine = geneva()?;
    let baked = engine
        .bake(&comp, &request, &dir)
        .map_err(|error| ApiError::failed(error.to_string()))?;

    // The bake cache is bounded: every bake sweeps its own root under the
    // machine-sized budget, the bake just written kept but counted, so
    // `cache/comps` is an accelerator and not a disk-eater that grows a
    // directory per comp forever. A hit is swept the same as a write -
    // the budget holds either way - and the manifest the hit touched is
    // the clock the sweep sorts by.
    let swept = concat_motion::cache::sweep_comps(
        &cache_dir(&project),
        concat_motion::cache::comps_budget(),
        Some(&dir),
    );

    let mut reply = json!({
        "frames": baked.frames,
        "width": baked.width,
        "height": baked.height,
        "fps": baked.fps,
        "hash": format!("{:016x}", baked.hash),
        "comp": comp_file.display().to_string(),
        "dir": baked.dir.display().to_string(),
        "engine": baked.engine.as_str(),
        "engine_version": baked.engine_version,
        "cached": baked.cached,
    });
    if asked != (width, height) {
        reply["clampedTo"] =
            json!("4096: the bake was requested above 4K and is rendered at 4K for safety");
    }
    // The sweep is said only when it did something: a sweep that freed
    // nothing is housekeeping, not news.
    if !swept.evicted.is_empty() || !swept.junk.is_empty() {
        reply["cache"] = json!({
            "budgetBytes": swept.budget,
            "keptBytes": swept.kept,
            "freedBytes": swept.freed,
            "evicted": swept.evicted,
            "junk": swept.junk,
        });
    }

    if fields.get("encode").and_then(Value::as_bool) == Some(true) {
        let name = fields
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty() && !name.contains(".."))
            .map(|name| name.trim().to_owned())
            .unwrap_or_else(|| format!("{hash}.mp4"));
        // The export resolver finds a motion clip's bake by the comp hash
        // in the filename, so a custom name carries it too: a bake written
        // under a hashless name encoded fine and then vanished from the
        // render, which is exactly the bug this follows. The confinement
        // is the same line every other write holds.
        let name = if name.starts_with(&hash) {
            name
        } else {
            format!("{hash}-{name}")
        };
        let out = crate::sense::write_target(backend, Some(&motion_dir(&project)), &name)?;
        encode_bake(&engine, &dir, &out, crf)?;
        // The honest note every overlay comp earns: the sidecar's video
        // codecs carry no alpha channel, and only the PNG sequence keeps
        // what a transparent background meant.
        if comp.background.a < 1.0 {
            reply["note"] = json!(
                "this comp has a transparent background; the encoded file is opaque where \
                 the comp was not - the PNG sequence in `dir` keeps the alpha"
            );
        }
        reply["file"] = json!(out.display().to_string());
    }
    Ok(Outcome::json(reply))
}

/// Encode a staged bake to one file, the way `concat-motion-agent bake
/// --encode` does: the timeline the bake staged is rendered once more with
/// its encode instruction switched to a video codec. The extension picks
/// it - `.webm` VP9, `.mov` ProRes 4444, anything else the H.264 every
/// player reads.
fn encode_bake(engine: &Geneva, dir: &Path, out: &Path, crf: u8) -> Result<(), ApiError> {
    if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|error| ApiError::failed(format!("{}: {error}", parent.display())))?;
    }
    let timeline = dir.join("timeline.json");
    let text = std::fs::read_to_string(&timeline)
        .map_err(|error| ApiError::failed(format!("{}: {error}", timeline.display())))?;
    let mut document: Value = serde_json::from_str(&text)
        .map_err(|error| ApiError::failed(format!("{}: {error}", timeline.display())))?;
    let (container, codec, profile) = match out.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("webm") => ("webm", "vp9", None),
        Some(ext) if ext.eq_ignore_ascii_case("mov") => ("mov", "prores", Some("4444")),
        _ => ("mp4", "h264", None),
    };
    let mut video = Map::new();
    video.insert("codec".to_owned(), json!(codec));
    match profile {
        Some(profile) => {
            video.insert("profile".to_owned(), json!(profile));
        }
        None => {
            video.insert("crf".to_owned(), json!(crf));
        }
    }
    document["output"]["encode"] = json!({
        "container": container,
        "video": Value::Object(video),
    });
    std::fs::write(
        &timeline,
        serde_json::to_string_pretty(&document)
            .map_err(|error| ApiError::failed(format!("the staged timeline: {error}")))?,
    )
    .map_err(|error| ApiError::failed(format!("{}: {error}", timeline.display())))?;
    let render = [
        "--format".to_owned(),
        "json".to_owned(),
        "render".to_owned(),
        "timeline.json".to_owned(),
        "-o".to_owned(),
        out.display().to_string(),
    ];
    engine
        .run_in(dir, &render)
        .map_err(|error| ApiError::failed(error.to_string()))?;
    if !out.is_file() {
        return Err(ApiError::failed(format!(
            "geneva reported success but {} was not written",
            out.display()
        )));
    }
    Ok(())
}

/// `captions_animate`: a word-timed transcript in, a baked caption video
/// out - the words broken into lines by the caption rules, each line one
/// kinetic comp, the whole thing one render. The file lands under the
/// project's `motion/`, ready for `media_import` and `clip_add` at the
/// voiceover's own start.
pub fn captions_animate(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if ![
            "project",
            "words",
            "preset",
            "wordsPerLine",
            "maxLineSeconds",
            "gapSeconds",
            "leadSeconds",
            "tailSeconds",
            "width",
            "height",
            "fps",
            "plate",
            "name",
            "curtain",
            "kinetic",
        ]
        .contains(&key.as_str())
        {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let project = project_of(&fields, backend)?;
    let words = read_words(
        fields
            .get("words")
            .ok_or_else(|| ApiError::invalid("invalid arguments: `words` is required"))?,
    )?;
    if words.is_empty() {
        return Err(ApiError::invalid(
            "invalid arguments: `words` holds at least one word",
        ));
    }
    // The caption canvas follows the project the way a bake's does: a
    // caller that names a `project` gets its own frame, read from its
    // document, so a 16:9 edit's captions are not a vertical reel's
    // default. A caller that names neither keeps the reel default -
    // caption video is short-form, and the library's reels are portrait
    // unless the caller says otherwise.
    let frame = fields
        .get("project")
        .and_then(Value::as_str)
        .map(Path::new)
        .and_then(frame_of);
    let width = fields
        .get("width")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .or(frame.map(|(width, _)| width))
        .unwrap_or(1080);
    let height = fields
        .get("height")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .or(frame.map(|(_, height)| height))
        .unwrap_or(1920);
    let fps = fields
        .get("fps")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 240.0)
        .unwrap_or(30.0);
    // A vertical default: caption video is short-form, and the library's
    // reels are portrait unless the caller says otherwise.
    let mut caps = concat_motion::captions::Captions {
        comp: fields
            .get("preset")
            .and_then(Value::as_str)
            .unwrap_or("funcut.caption.kinetic")
            .to_owned(),
        ..concat_motion::captions::Captions::default()
    };
    if let Some(count) = fields.get("wordsPerLine").and_then(Value::as_u64) {
        caps.words_per_line = (count.clamp(1, 12)) as usize;
    }
    if let Some(line) = fields.get("maxLineSeconds").and_then(Value::as_f64) {
        caps.max_line = line.clamp(0.5, 12.0);
    }
    if let Some(gap) = fields.get("gapSeconds").and_then(Value::as_f64) {
        caps.gap = gap.clamp(0.0, 5.0);
    }
    if let Some(lead) = fields.get("leadSeconds").and_then(Value::as_f64) {
        caps.lead = lead.clamp(0.0, 2.0);
    }
    if let Some(tail) = fields.get("tailSeconds").and_then(Value::as_f64) {
        caps.tail = tail.clamp(0.0, 5.0);
    }
    // The kinetic hand: `kinetic: true` takes the library's word-staggered
    // default, and an object spells it out - `stagger` ("word" or "char"),
    // `delay` seconds between units, `entrance` ("blur-in", "slide-up" or
    // "scale-in"), `seconds` for the entrance, `curve` as any comp curve,
    // `flexSeconds`, and `weight: { "from": 100, "to": 900 }` for the
    // variable-font flex. The caption node is rebuilt as markup whose
    // per-word spans appear, light and settle on the recogniser's own
    // word times.
    match fields.get("kinetic") {
        Some(Value::Bool(true)) => {
            caps.kinetic = Some(concat_motion::typography::Kinetic::words());
        }
        Some(setting @ Value::Object(_)) => {
            caps.kinetic = Some(serde_json::from_value(setting.clone()).map_err(|error| {
                ApiError::invalid(format!("invalid arguments: `kinetic`: {error}"))
            })?);
        }
        Some(Value::Bool(false)) | None => {}
        Some(other) => {
            return Err(ApiError::invalid(format!(
                "invalid arguments: `kinetic` is true, false, or an object of its settings - not {}",
                serde_json::to_string(other).unwrap_or_default()
            )));
        }
    }

    let lines = concat_motion::captions::lay_out(&words, &caps);
    let line_count = lines.len();
    let mut reel = concat_motion::captions::sequence(&words, &caps, width, height, fps);
    // The curtain dips between caption lines read as flashes when the reel
    // is burned over a voiceover - hard cuts are usually what that cut
    // wants. The default keeps the cinematic dip; `curtain: false`
    // removes the whole curtain layer.
    if fields.get("curtain").and_then(Value::as_bool) == Some(false) {
        reel.curtain = None;
    }
    if let Some(plate) = fields.get("plate").and_then(Value::as_str) {
        if plate.contains("..") || plate.starts_with('/') {
            return Err(ApiError::invalid(
                "invalid arguments: `plate` is a file name, not a path out of the project",
            ));
        }
        let file = project.join(plate);
        if !file.is_file() {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no plate at {}", file.display()),
            ));
        }
        reel.asset(
            "plate".to_owned(),
            concat_motion::comp::Asset {
                src: file.display().to_string(),
                kind: None,
            },
        );
        for scene in &mut reel.scenes {
            scene.layers.push(SceneLayer::new("plate").zooming(0.04));
        }
    }

    let engine = geneva()?;
    let planned =
        sequence::plan(&engine, &reel).map_err(|error| ApiError::failed(error.to_string()))?;
    let cache = cache_dir(&project).join(format!("captions-{}x{}@{}", width, height, fps,));
    std::fs::create_dir_all(motion_dir(&project))
        .map_err(|error| ApiError::failed(format!("could not create motion/: {error}")))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let name = fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty() && !name.contains(".."))
        .map(|name| name.trim().to_owned())
        .unwrap_or_else(|| format!("captions-{stamp}.mp4"));
    // The name lands under motion/, and an agent-named write is held to
    // the same roots every other write is.
    let out = crate::sense::write_target(backend, Some(&motion_dir(&project)), &name)?;
    planned
        .render(&engine, &cache, &out)
        .map_err(|error| ApiError::failed(error.to_string()))?;

    let mut reply = json!({
        "file": out.display().to_string(),
        "duration": planned.duration,
        "lines": line_count,
        "words": words.len(),
        "width": width,
        "height": height,
        "fps": fps,
        "preset": caps.comp,
        "kinetic": caps.kinetic.is_some(),
        "warnings": planned.warnings,
    });
    if let Some(scene_count) = planned.document["layers"].as_array() {
        reply["scenes"] = json!(scene_count.len());
    }
    Ok(Outcome::json(reply))
}

/// Words from the shapes a recogniser or a person gives: triples
/// `[text, start, end]`, objects `{"text", "start", "end"}`, or a
/// `{"words": [...]}` wrapper around either.
fn read_words(document: &Value) -> Result<Vec<concat_motion::captions::Word>, ApiError> {
    use concat_motion::captions::Word;
    let list = match document {
        Value::Array(list) => list,
        Value::Object(map) => match map.get("words") {
            Some(Value::Array(list)) => list,
            _ => {
                return Err(ApiError::invalid(
                    "invalid arguments: `words` is an array of [text, start, end] triples",
                ));
            }
        },
        _ => {
            return Err(ApiError::invalid(
                "invalid arguments: `words` is an array of [text, start, end] triples",
            ));
        }
    };
    let mut words = Vec::with_capacity(list.len());
    for (index, entry) in list.iter().enumerate() {
        let word = match entry {
            Value::Array(parts) => {
                let text = parts
                    .first()
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .ok_or_else(|| {
                        ApiError::invalid(format!(
                            "invalid arguments: word {index} starts with its text"
                        ))
                    })?;
                let start = parts.get(1).and_then(Value::as_f64).ok_or_else(|| {
                    ApiError::invalid(format!(
                        "invalid arguments: word {index} carries its start time second"
                    ))
                })?;
                let end = parts.get(2).and_then(Value::as_f64).unwrap_or(start + 0.25);
                Word::new(text, start, end)
            }
            Value::Object(fields) => {
                let text = fields
                    .get("text")
                    .or_else(|| fields.get("word"))
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .ok_or_else(|| {
                        ApiError::invalid(format!("invalid arguments: word {index} needs a `text`"))
                    })?;
                let start = fields.get("start").and_then(Value::as_f64).ok_or_else(|| {
                    ApiError::invalid(format!("invalid arguments: word {index} needs a `start`"))
                })?;
                let end = fields
                    .get("end")
                    .and_then(Value::as_f64)
                    .unwrap_or(start + 0.25);
                Word::new(text, start, end)
            }
            _ => {
                return Err(ApiError::invalid(format!(
                    "invalid arguments: word {index} is a triple or an object"
                )));
            }
        };
        if !word.start.is_finite() || word.start < 0.0 || !word.end.is_finite() {
            return Err(ApiError::invalid(format!(
                "invalid arguments: word {index} has times that are not finite"
            )));
        }
        words.push(word);
    }
    Ok(words)
}

/// `motion_reel`: a whole storyboard - several scenes, layered plates and
/// overlays, a curtain, a bed of sound - rendered as one video under the
/// project's `motion/`, ready for `media_import` and `clip_add`.
///
/// The storyboard is the same document `concat-motion-agent reel` eats, so
/// a reel that renders one way renders the other way too. A scene's
/// `layers` stack bottom first: plates under the comp, and with `above:
/// true` grain, vignettes and light leaks over it, each with its own
/// drift (`pan`, `zoom`), blend, opacity, fades and timing - which is
/// what makes a stack of stills read as depth. Every asset a storyboard
/// names lives under the project; a path that climbs out of it is
/// refused.
///
/// `plan: true` lays the reel out and returns the timeline without
/// rendering; `frame: seconds` renders one still and returns it as an
/// INLINE IMAGE - the look-before-you-render of the design loop. The
/// render lands under `<project>/motion/<name>.mp4` (`.mp4` H.264,
/// `.webm` VP9, `.mov` ProRes 4444).
pub fn reel(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if ![
            "project",
            "storyboard",
            "name",
            "frame",
            "plan",
            "crf",
            "genevaTimeoutSecs",
            "sheet",
        ]
        .contains(&key.as_str())
        {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let project = project_of(&fields, backend)?;
    let document = fields
        .get("storyboard")
        .ok_or_else(|| {
            ApiError::invalid("invalid arguments: `storyboard` is the storyboard document")
        })?
        .clone();
    // The project is the storyboard's world: every path it names resolves
    // inside it, and a `source_dir` that would move that world is ignored
    // rather than honoured - an agent's storyboard cannot reach past the
    // folder it was given.
    let mut document = document;
    if let Value::Object(map) = &mut document {
        map.remove("source_dir");
    }
    confine_storyboard(&document, &project)?;
    let board = storyboard::Storyboard::from_value(&document).map_err(ApiError::invalid)?;
    let reel = board.sequence(&project).map_err(ApiError::invalid)?;

    let engine = geneva()?;
    // The tool's own per-call cap, when the caller set one: a render
    // that outlives it is killed and named, and the quick calls keep
    // their 60 s unless the cap is tighter than that.
    let engine = match fields.get("genevaTimeoutSecs").and_then(Value::as_f64) {
        Some(secs) if (5.0..=3600.0).contains(&secs) => engine.with_limits(
            concat_motion::CallLimits::capped_at(std::time::Duration::from_secs_f64(secs)),
        ),
        Some(secs) => {
            return Err(ApiError::invalid(format!(
                "invalid arguments: `genevaTimeoutSecs` is seconds between 5 and 3600, not {secs}"
            )));
        }
        None => engine,
    };
    let mut planned =
        sequence::plan(&engine, &reel).map_err(|error| ApiError::failed(error.to_string()))?;
    let summary = json!({
        "scenes": planned.scenes,
        "duration": planned.duration,
        "width": reel.width,
        "height": reel.height,
        "fps": reel.fps,
        "layers": planned.document["layers"].as_array().map(Vec::len).unwrap_or(0),
        "assets": planned.files.len(),
        "warnings": planned.warnings,
    });
    if fields.get("plan").and_then(Value::as_bool) == Some(true) {
        let mut reply = summary;
        reply["timeline"] = planned.document.clone();
        return Ok(Outcome::json(reply));
    }

    // The cache is content-addressed by the storyboard's own hash: the
    // same reel re-renders nothing, and an edit misses immediately.
    let hash = format!("{:016x}", storyboard::hash(&document));
    let cache = cache_dir(&project).join(format!("reel-{hash}"));
    std::fs::create_dir_all(motion_dir(&project))
        .map_err(|error| ApiError::failed(format!("could not create motion/: {error}")))?;
    if let Some(crf) = fields.get("crf").and_then(Value::as_u64) {
        if crf > 51 {
            return Err(ApiError::invalid(format!(
                "invalid arguments: `crf` is 0 to 51, not {crf}"
            )));
        }
        sequence_crf(&mut planned, crf as u8);
    }

    if let Some(at) = fields.get("frame").and_then(Value::as_f64) {
        if !at.is_finite() || at < 0.0 {
            return Err(ApiError::invalid(
                "invalid arguments: `frame` is a time in seconds, 0 or more",
            ));
        }
        let out = motion_dir(&project).join(format!("reel-frame-{hash}-{at:.2}.jpg"));
        planned
            .frame(&engine, &cache, at, &out)
            .map_err(|error| ApiError::failed(error.to_string()))?;
        let bytes = std::fs::read(&out)
            .map_err(|error| ApiError::failed(format!("could not read the frame: {error}")))?;
        let inline = crate::sense::base64(&bytes);
        let reading = json!({
            "at": at,
            "frame": out.display().to_string(),
            "duration": planned.duration,
            "note": "One frame of the whole reel. Check the layer stack reads as one scene - depth, not a collage - before rendering it all.",
            "plan": summary,
        });
        return Ok(Outcome {
            content: vec![
                json!({ "type": "image", "data": inline, "mimeType": "image/jpeg" }),
                json!({
                    "type": "text",
                    "text": serde_json::to_string_pretty(&reading).unwrap_or_default(),
                }),
            ],
            structured: Some(reading),
            is_error: false,
        });
    }

    // The contact sheet: four stills of the reel, tiled into one image
    // the critic can look at. This is the creative loop's look half over
    // the reel's own output - render it, see it, revise it - and the
    // sheet lands where craft_critique reads from.
    if fields.get("sheet").and_then(Value::as_bool) == Some(true) {
        let at = [
            (planned.duration * 0.25).max(0.0),
            (planned.duration * 0.5).max(0.0),
            (planned.duration * 0.75).max(0.0),
            (planned.duration * 0.95).max(0.0),
        ];
        let mut frames = Vec::new();
        for (nth, seconds) in at.iter().enumerate() {
            let out = motion_dir(&project).join(format!("reel-sheet-{hash}-{nth}.png"));
            planned
                .frame(&engine, &cache, *seconds, &out)
                .map_err(|error| ApiError::failed(error.to_string()))?;
            frames.push(out);
        }
        let sense = project.join("cache").join("sense");
        std::fs::create_dir_all(&sense)
            .map_err(|error| ApiError::failed(format!("could not create cache/sense/: {error}")))?;
        let sheet_path = sense.join(format!("reel-sheet-{hash}.png"));
        let bytes = crate::sheet::tile(
            [&frames[0], &frames[1], &frames[2], &frames[3]],
            reel.width,
            reel.height,
        );
        std::fs::write(&sheet_path, &bytes)
            .map_err(|error| ApiError::failed(format!("could not write the sheet: {error}")))?;
        // The four stills were scaffolding for the tile, and the loop this
        // serves runs again on every revision: a hash apiece, four
        // full-resolution pictures left behind each time, is not a cache
        // anyone asked for.
        for frame in &frames {
            let _ = std::fs::remove_file(frame);
        }
        let inline = crate::sense::base64(&bytes);
        let reading = crate::sheet::reading(&sheet_path, planned.duration, &at);
        return Ok(Outcome {
            content: vec![
                json!({ "type": "image", "data": inline, "mimeType": "image/png" }),
                json!({
                    "type": "text",
                    "text": serde_json::to_string_pretty(&reading).unwrap_or_default(),
                }),
            ],
            structured: Some(reading),
            is_error: false,
        });
    }

    let name = fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty() && !name.contains(".."))
        .map(|name| name.trim().to_owned())
        .unwrap_or_else(|| format!("reel-{hash}.mp4"));
    let out = crate::sense::write_target(backend, Some(&motion_dir(&project)), &name)?;
    // The scene-aware cache: one scene, one entry. The reel renders
    // scene by scene, each cached by what actually draws it (its own
    // document, its assets' bytes, the geometry, the engine, the curtain
    // at its boundaries), so an edit to one scene re-renders that scene
    // and serves the rest - and an unchanged reel serves the join whole,
    // byte-identical, rendering nothing.
    let crf = match fields.get("crf").and_then(Value::as_u64) {
        Some(crf) if crf <= 51 => crf as u8,
        Some(crf) => {
            return Err(ApiError::invalid(format!(
                "invalid arguments: `crf` is 0 to 51, not {crf}"
            )));
        }
        None => 18,
    };
    // The scene cache is shared across reels: a scene identical in two
    // reels is one entry, drawn once, wherever it appears.
    let scene_cache = cache_dir(&project).join("scenes");
    let report =
        concat_motion::reel::render_reel(&engine, &board, &project, &out, &scene_cache, crf)
            .map_err(|error| ApiError::failed(error.to_string()))?;
    if !out.is_file() {
        return Err(ApiError::failed(format!(
            "geneva reported success but {} was not written",
            out.display()
        )));
    }
    let mut reply = summary;
    reply["file"] = json!(out.display().to_string());
    // The reel's own length and its own warnings, not the whole-board
    // plan's: a board whose scenes overlap is joined flush by the cache,
    // and the file is as long as the segments made it - the plan's number
    // would describe a different reel than the one on the disk.
    reply["duration"] = json!(report.duration);
    let mut warnings = planned.warnings.clone();
    warnings.extend(report.warnings.iter().cloned());
    reply["warnings"] = json!(warnings);
    reply["cache"] = json!({
        "scenesDrawn": report.renders,
        "scenesServed": report.scenes.len() - report.renders,
        "joinedFromCache": report.joined_from_cache,
        "audioRendered": report.audio_rendered,
        "seconds": report.seconds,
        "sceneKeys": report.scenes.iter().map(|s| s.key.clone()).collect::<Vec<String>>(),
    });
    Ok(Outcome::json(reply))
}

/// A storyboard's paths are names under the project, not routes out of it:
/// every asset and every comp is checked before anything is read, so a
/// storyboard cannot be a way to walk the disk.
fn confine_storyboard(document: &Value, project: &Path) -> Result<(), ApiError> {
    let refuse = |what: &str, value: &str| {
        ApiError::invalid(format!(
            "invalid arguments: the storyboard's {what} {value:?} is a name under the project, \
             not a path out of it"
        ))
    };
    let mut paths: Vec<(String, &str)> = Vec::new();
    if let Some(assets) = document.get("assets").and_then(Value::as_object) {
        for (id, src) in assets {
            if let Some(src) = src.as_str() {
                paths.push((id.clone(), src));
            }
        }
    }
    if let Some(scenes) = document.get("scenes").and_then(Value::as_array) {
        for (index, scene) in scenes.iter().enumerate() {
            if let Some(comp) = scene.get("comp").and_then(Value::as_str) {
                paths.push((format!("scene {index} comp"), comp));
            }
        }
    }
    let root = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    for (what, path) in paths {
        if path.starts_with('/')
            || path.contains('\\')
            || path.split(['/']).any(|part| part == "..")
        {
            return Err(refuse(&what, path));
        }
        // A path that exists must resolve inside the project: a symlink
        // cannot quietly move an asset out of it. A path that does not
        // exist is left for the plan to name, which it does by asset id.
        let joined = project.join(path);
        if joined.exists() {
            let resolved = std::fs::canonicalize(&joined).unwrap_or(joined);
            if !resolved.starts_with(&root) {
                return Err(refuse(&what, path));
            }
        }
    }
    Ok(())
}

/// The crf a storyboard's render runs at, held to the encoder's own range.
fn sequence_crf(plan: &mut sequence::Plan, crf: u8) {
    plan.document["output"]["encode"]["video"]["crf"] = json!(crf.min(51));
}

/// `motion_probe`: what a media file is, as the engine's own reader sees
/// it - the streams, the frame, the duration, the diagnostics a broken
/// file earns. The look-before-you-build over the file itself: the same
/// reader the renderer will use, asked first.
pub fn media_probe(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if !["path", "project"].contains(&key.as_str()) {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let path = fields
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| ApiError::invalid("invalid arguments: `path` is required"))?;
    // An absolute path is taken as it stands; a relative one resolves
    // against the project, and may not climb out of it - the same wall
    // every other project-relative name is held to.
    let file = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        let project = fields
            .get("project")
            .and_then(Value::as_str)
            .filter(|project| !project.trim().is_empty())
            .ok_or_else(|| {
                ApiError::invalid(
                    "invalid arguments: a relative `path` needs the `project` it resolves against",
                )
            })?;
        if path.contains("..") || path.contains('\\') {
            return Err(ApiError::invalid(
                "invalid arguments: `path` is a name under the project, not a path out of it",
            ));
        }
        Path::new(project).join(path)
    };
    if !file.is_file() {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("no media file at {}", file.display()),
        ));
    }
    let engine = geneva()?;
    let report = engine
        .probe_media(&file)
        .map_err(|error| ApiError::failed(error.to_string()))?;
    Ok(Outcome::json(json!({
        "file": file.display().to_string(),
        "probe": report,
    })))
}

/// `scene_timeline`: the look-before-you-render of a storyboard - the
/// same plan `motion_reel` lays, staged, handed to the engine's own
/// validator (`geneva validate --probe`, which opens every asset), and
/// returned with the timeline the engine would take and the engine's
/// verdict on it. Nothing renders and nothing is written.
pub fn scene_timeline(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if !["project", "storyboard"].contains(&key.as_str()) {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let project = project_of(&fields, backend)?;
    let document = fields
        .get("storyboard")
        .ok_or_else(|| {
            ApiError::invalid("invalid arguments: `storyboard` is the storyboard document")
        })?
        .clone();
    // The project is the storyboard's world, exactly as the reel tool
    // holds it: no source_dir, no path out of the folder.
    let mut document = document;
    if let Value::Object(map) = &mut document {
        map.remove("source_dir");
    }
    confine_storyboard(&document, &project)?;
    let board = storyboard::Storyboard::from_value(&document).map_err(ApiError::invalid)?;
    let reel = board.sequence(&project).map_err(ApiError::invalid)?;

    let engine = geneva()?;
    let planned =
        sequence::plan(&engine, &reel).map_err(|error| ApiError::failed(error.to_string()))?;
    // The plan is staged into a scratch of its own - the engine's
    // validator runs against the timeline.json the render would take,
    // and the scratch leaves nothing behind in the project.
    let hash = format!("{:016x}", storyboard::hash(&document));
    let scratch = cache_dir(&project).join(format!("timeline-{hash}"));
    planned
        .write(&scratch)
        .map_err(|error| ApiError::failed(error.to_string()))?;
    // Whatever the engine says about it - the validation report on the
    // ok side, the diagnostics and their explanations on the refusal -
    // is the reply, because what the engine reads is what the caller
    // asked for.
    let reply = match engine.validate_timeline(&scratch) {
        Ok(shown) => json!({
            "scenes": planned.scenes,
            "duration": planned.duration,
            "width": reel.width,
            "height": reel.height,
            "fps": reel.fps,
            "assets": planned.files.len(),
            "warnings": planned.warnings,
            "timeline": planned.document,
            "asTheEngineReadsIt": shown,
        }),
        Err(error) => {
            // The refusal carries its own diagnostics; `geneva explain`
            // has already been consulted for the first code, because the
            // engine's words about its own codes are the fix's first
            // line.
            json!({
                "ok": false,
                "scenes": planned.scenes,
                "duration": planned.duration,
                "warnings": planned.warnings,
                "timeline": planned.document,
                "error": error.to_string(),
            })
        }
    };
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(Outcome::json(reply))
}

/// `geneva_explain`: what a diagnostic code means, in the engine's own
/// words - `geneva explain E302` at the MCP surface, so an agent that
/// meets a refusal can read any code it carries without shelling out.
/// The refusals the motion tools return have already consulted this for
/// their first diagnostic; this is the door for the second and third
/// codes, and for a code the engine printed anywhere else.
pub fn geneva_explain(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if key != "code" {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let text = fields
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .ok_or_else(|| ApiError::invalid("invalid arguments: `code` is required"))?;
    // The code: the whole argument when it is one (`E302`), or the
    // first code-shaped token of a diagnostic pasted whole - the shape
    // is an uppercase letter and the digits after it, which is how the
    // sidecar numbers its own refusals.
    let shaped = |token: &str| {
        let mut characters = token.chars();
        characters
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
            && characters.all(|c| c.is_ascii_digit())
    };
    let code = if shaped(text) {
        text.to_owned()
    } else {
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .find(|token| token.len() > 1 && shaped(token))
            .map(str::to_owned)
            .ok_or_else(|| {
                ApiError::invalid(
                    "invalid arguments: `code` is a diagnostic code (E302) or a \
                     refusal that carries one",
                )
            })?
    };
    let engine = geneva()?;
    let Some(meaning) = engine.explain(&code) else {
        return Err(ApiError::failed(format!(
            "the engine had nothing to say about {code:?} - a code the sidecar \
             knows is one of its own refusals",
        )));
    };
    Ok(Outcome::json(json!({
        "code": code,
        "meaning": meaning,
    })))
}

/// The farm's progress stream, read live: what a `concat-motion-agent
/// render --farm` run is doing right now, without waiting for it.
///
/// The farm cuts a long render into parts that survive their own failure,
/// and every state it passes through is a line in the farm directory's
/// `progress.jsonl` - read here mid-render, from another process, and
/// answered as the number an agent actually wants: the parts done against
/// the parts planned, the frames finished, and the estimate the pace of
/// the parts drawn so far gives for the rest. A farm that has said nothing
/// is a whole farm pending; a farm whose last event is `done` says where
/// the file landed.
pub fn farm_progress(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if key != "farm" && key != "project" {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let farm = fields
        .get("farm")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|farm| !farm.is_empty())
        .ok_or_else(|| ApiError::invalid("invalid arguments: `farm` is required"))?;
    let path = if Path::new(farm).is_absolute() {
        PathBuf::from(farm)
    } else {
        let project = fields
            .get("project")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| {
                ApiError::invalid(
                    "invalid arguments: a relative `farm` needs the `project` it is under",
                )
            })?;
        project.join(farm)
    };
    if path.components().any(|part| {
        matches!(
            part,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return Err(ApiError::invalid(
            "invalid arguments: `farm` may not hold `..` or a drive",
        ));
    }
    let read = concat_motion::progress::progress(&path)
        .map_err(|error| ApiError::failed(error.to_string()))?;
    Ok(Outcome::json(read.to_json()))
}

/// `motion_farm_cloud`: the cloud farm's legs - a farm directory and its
/// staged render tree, pushed to and pulled from object storage, and a
/// worker's whole day (`work`: pull, render one missing part through the
/// sidecar with the farm's own atomic shapes and keys, push it back).
///
/// Everything the storage needs comes from the environment the operator
/// set - `FUNCUT_FARM_BUCKET`, `FUNCUT_FARM_PREFIX`,
/// `FUNCUT_FARM_ENDPOINT`, `FUNCUT_FARM_REGION`, `AWS_ACCESS_KEY_ID`,
/// `AWS_SECRET_ACCESS_KEY` - and never from these arguments, the same
/// posture the VLM endpoint allow-list takes: a prompted agent cannot
/// aim a render, or the credentials beside it, at a host the operator
/// did not choose. The tool's own budget is the deep one (1200 s); a
/// push that outlives it is resumed cheaply, because a re-push skips
/// every file whose stored sha256 already matches.
pub fn farm_cloud(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if ![
            "action",
            "farm",
            "staged",
            "project",
            "part",
            "geneva",
            "prefix",
            "parts",
            "timeoutSeconds",
        ]
        .contains(&key.as_str())
        {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let action = fields
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or_else(|| ApiError::invalid("invalid arguments: `action` is required"))?;
    if !["push", "pull", "status", "work"].contains(&action) {
        return Err(ApiError::invalid(
            "invalid arguments: `action` is push, pull, status or work",
        ));
    }
    let farm = fields
        .get("farm")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|farm| !farm.is_empty())
        .ok_or_else(|| ApiError::invalid("invalid arguments: `farm` is required"))?;
    // The same confinement `motion_farm_progress` holds: a farm is a
    // directory under the operator's own sky, not a path out of it.
    let path = if Path::new(farm).is_absolute() {
        PathBuf::from(farm)
    } else {
        let project = fields
            .get("project")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| {
                ApiError::invalid(
                    "invalid arguments: a relative `farm` needs the `project` it is under",
                )
            })?;
        project.join(farm)
    };
    if path.components().any(|part| {
        matches!(
            part,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return Err(ApiError::invalid(
            "invalid arguments: `farm` may not hold `..` or a drive",
        ));
    }
    let staged = fields.get("staged").and_then(Value::as_str).map(str::trim);
    if staged.is_some_and(|staged| staged.contains("..")) {
        return Err(ApiError::invalid(
            "invalid arguments: `staged` is a directory path, not a climb out of one",
        ));
    }
    // The spec the script reads: the action, the resolved farm, and the
    // worker's own knobs. The bucket, the endpoint and the credentials
    // are deliberately absent - the script reads those from the
    // environment the operator set, and nothing here can name them.
    let mut spec = json!({ "action": action, "farm": path.display().to_string() });
    if let Some(staged) = staged.filter(|staged| !staged.is_empty()) {
        spec["staged"] = json!(staged);
    }
    if let Some(part) = fields.get("part").and_then(Value::as_u64) {
        spec["part"] = json!(part);
    }
    if let Some(parts) = fields.get("parts").and_then(Value::as_u64) {
        spec["parts"] = json!(parts);
    }
    if let Some(geneva) = fields.get("geneva").and_then(Value::as_str) {
        spec["geneva"] = json!(geneva);
    }
    if let Some(prefix) = fields.get("prefix").and_then(Value::as_str) {
        spec["prefix"] = json!(prefix);
    }
    if let Some(timeout) = fields.get("timeoutSeconds").and_then(Value::as_f64) {
        spec["timeoutSeconds"] = json!(timeout);
    }
    let reply = crate::sense::run_py(
        &crate::sense::runtime_script("farm-sync.py")?,
        action,
        &spec,
        std::time::Duration::from_secs(1140),
    )?;
    Ok(Outcome::json(reply))
}

/// Whether the motion tools can reach an engine at all, for the catalogue
/// to say so in its own words.
pub fn engine_available() -> bool {
    Geneva::discover(EnginePreference::default().geneva.as_deref()).is_some()
}

/// The error a caller meets when the sidecar is missing, for the catalogue
/// to carry verbatim rather than as a surprise.
pub fn engine_missing() -> ApiError {
    ApiError::new(
        ErrorCode::Refused,
        "the Geneva motion sidecar is not on this machine",
    )
}
