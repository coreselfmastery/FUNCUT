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
use concat_motion::comp::Comp;
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

/// `motion_frame`: one frame of a comp at `at` seconds, drawn in tens of
/// milliseconds, written under the project's `motion/` and returned as a
/// path - the look-before-you-bake of the verification loop.
pub fn frame(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("invalid arguments: the arguments are an object"))?;
    for key in fields.keys() {
        if !["preset", "comp", "project", "ops", "at", "width", "height"].contains(&key.as_str()) {
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
    let width = fields
        .get("width")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .unwrap_or(1080);
    let height = fields
        .get("height")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
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
