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
use concat_motion::sequence;
use serde_json::{Map, Value, json};

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
fn project_of(fields: &Map<String, Value>) -> Result<PathBuf, ApiError> {
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
pub fn frame(arguments: Value) -> Result<Outcome, ApiError> {
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
    let project = project_of(&fields)?;
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
pub fn bake(arguments: Value) -> Result<Outcome, ApiError> {
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
    let project = project_of(&fields)?;
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
    });

    if fields.get("encode").and_then(Value::as_bool) == Some(true) {
        let name = fields
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty() && !name.contains(".."))
            .map(|name| name.trim().to_owned())
            .unwrap_or_else(|| format!("{hash}.mp4"));
        let out = motion_dir(&project).join(&name);
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
pub fn captions_animate(arguments: Value) -> Result<Outcome, ApiError> {
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
        ]
        .contains(&key.as_str())
        {
            return Err(ApiError::invalid(format!(
                "invalid arguments: unknown field `{key}`"
            )));
        }
    }
    let project = project_of(&fields)?;
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
            scene.plate = Some(sequence::Plate {
                asset: "plate".to_owned(),
                zoom: 0.04,
            });
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
    let out = motion_dir(&project).join(name);
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
