// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The editor's senses. Before this module the only tool that returned an
//! MCP `type: image` block was `preview_frame`; everything else that made
//! pixels handed the agent a path it could not see. These tools decode real
//! frames, measure them, and return a labelled contact sheet as an image,
//! together with a structured craft reading and concrete revisions.
//!
//! The heavy lifting is two standard-library Python files embedded with
//! `include_str!` (see sense/README.md). They are written to a per-process
//! temp directory once. The timeline path needs no ffmpeg CLI at all: it
//! asks `preview.frame` for PNGs and decodes them in Python.

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use concat_api::ApiError;
use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::summary;
use crate::tools::Outcome;

const SENSE_PY: &str = include_str!("../sense/sense.py");
const VLM_PY: &str = include_str!("../sense/vlm.py");
static RUNTIME: OnceLock<Result<PathBuf, String>> = OnceLock::new();
static NONCE: AtomicU64 = AtomicU64::new(0);

/// Base64, for tools that have a PNG on disk and need it inline. Keeping it
/// here avoids a new dependency for twenty lines of alphabet.
pub fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn invalid(why: &str) -> ApiError {
    ApiError::invalid(format!("invalid arguments: {why}"))
}

fn failed(why: impl std::fmt::Display) -> ApiError {
    ApiError::failed(why.to_string())
}

/// Once per process: unpack the embedded Python runtime.
fn runtime() -> Result<&'static PathBuf, ApiError> {
    let result = RUNTIME.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("concat-sense-{}", std::process::id()));
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("sense.py"), SENSE_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("vlm.py"), VLM_PY).map_err(|e| e.to_string())?;
        Ok(dir)
    });
    result.as_ref().map_err(failed)
}

fn python() -> String {
    std::env::var("FUNCUT_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.to_owned())
}

/// Run one sense command. stdout and stderr go to files: a contact sheet is
/// hundreds of KB of base64, which would deadlock a piped `wait_with_output`.
fn run(cmd: &str, spec: &Value, timeout: Duration) -> Result<Value, ApiError> {
    let dir = runtime()?;
    let n = NONCE.fetch_add(1, Ordering::Relaxed);
    let out_path = dir.join(format!("out-{cmd}-{n}.json"));
    let err_path = dir.join(format!("err-{cmd}-{n}.log"));
    let out_file = File::create(&out_path).map_err(failed)?;
    let err_file = File::create(&err_path).map_err(failed)?;
    let spec = serde_json::to_string(spec).map_err(failed)?;
    let mut child = Command::new(python())
        .arg(dir.join("sense.py"))
        .arg(cmd)
        .arg("--spec")
        .arg(spec)
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn()
        .map_err(|e| {
            failed(format!(
                "could not run {}: {e}. Set FUNCUT_PYTHON to a Python 3 interpreter.",
                python()
            ))
        })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(failed(format!(
                        "sense {cmd} timed out after {}s",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            Err(e) => return Err(failed(e)),
        }
    }
    let text = fs::read_to_string(&out_path).unwrap_or_default();
    let stderr = fs::read_to_string(&err_path).unwrap_or_default();
    let _ = fs::remove_file(&out_path);
    let _ = fs::remove_file(&err_path);
    let value: Value = serde_json::from_str(text.trim()).map_err(|e| {
        failed(format!(
            "sense {cmd} returned no JSON ({e}). {}",
            stderr.chars().take(500).collect::<String>()
        ))
    })?;
    if value.get("ok") == Some(&Value::Bool(false)) {
        let mut message = value["error"]
            .as_str()
            .unwrap_or("unknown sense error")
            .to_owned();
        let errors = value["errors"].as_array().cloned().unwrap_or_default();
        if !errors.is_empty() {
            message.push_str(" | ");
            message.push_str(
                &errors
                    .iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
        }
        if !stderr.is_empty() {
            message.push_str(" | ");
            message.push_str(&stderr.chars().take(400).collect::<String>());
        }
        return Err(failed(message));
    }
    let mut value = value;
    if let Some(map) = value.as_object_mut() {
        map.insert(
            "stderrTail".to_owned(),
            json!(stderr.chars().rev().take(300).collect::<String>()),
        );
    }
    Ok(value)
}

/// An image plus its compact JSON reading. `pngB64` stays in the image block
/// only; structured clients get every measurement without a megabyte of PNG.
fn image_outcome(value: &Value, caption: &str) -> Outcome {
    let png = value
        .get("pngB64")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut structured = value.clone();
    if let Some(map) = structured.as_object_mut() {
        map.remove("pngB64");
        map.remove("stderrTail");
    }
    let text = serde_json::to_string_pretty(&structured).unwrap_or_else(|_| structured.to_string());
    Outcome {
        content: vec![
            json!({ "type": "image", "data": png, "mimeType": "image/png" }),
            json!({ "type": "text", "text": format!("{caption}\n\n{text}") }),
        ],
        structured: Some(structured),
        is_error: false,
    }
}

fn number(fields: &Map<String, Value>, key: &str, default: f64) -> f64 {
    fields.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn request(method: &str, mut params: Value) -> Result<concat_api::Request, ApiError> {
    let fields = params
        .as_object_mut()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    fields.insert("method".to_owned(), json!(method));
    serde_json::from_value(params).map_err(|e| invalid(&format!("{e}")))
}

/// `watch_render`: sample a rendered file or, with no file, the open
/// project's true composited timeline. Either way the agent gets a picture.
pub fn watch_render(arguments: Value, backend: &mut dyn Backend) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let samples = (number(&fields, "samples", 12.0).round() as usize).clamp(2, 24);
    let cols = (number(&fields, "cols", 4.0).round() as usize).clamp(1, 6);
    let tile_width = (number(&fields, "tileWidth", 320.0).round() as u32).clamp(64, 480);
    let tile_height = (number(&fields, "tileHeight", 180.0).round() as u32).clamp(48, 360);
    let mut spec = json!({
        "samples": samples,
        "cols": cols,
        "tileWidth": tile_width,
        "tileHeight": tile_height,
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    if let Some(path) = fields.get("out").and_then(Value::as_str) {
        spec["out"] = json!(path);
    }

    let caption;
    if let Some(file) = fields.get("path").and_then(Value::as_str) {
        // A rendered movie: ffmpeg decodes frames and the audio envelope.
        spec["path"] = json!(file);
        spec["audio"] = json!(fields.get("audio").and_then(Value::as_bool).unwrap_or(true));
        if let Some(times) = fields.get("times").and_then(Value::as_array) {
            let sources: Vec<Value> = times
                .iter()
                .filter_map(|t| t.as_f64())
                .enumerate()
                .map(|(i, at)| json!({ "path": file, "at": at, "label": format!("{i:02}") }))
                .collect();
            if !sources.is_empty() {
                spec["sources"] = json!(sources);
                spec.as_object_mut().map(|m| m.remove("path"));
                spec["audio"] = json!(false);
            }
        }
        caption =
            format!("{samples} frames sampled from the rendered file {file}, in timeline order.");
    } else {
        // No file: ask the editor itself for the true composited frames.
        let project = fields
            .get("project")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                invalid("give `path` (a rendered movie) or `project` (an open project)")
            })?;
        let view = backend.call(request("project.get", json!({ "path": project }))?)?;
        let read = summary::view(&view);
        let duration = fields
            .get("duration")
            .and_then(Value::as_f64)
            .filter(|d| *d > 0.0)
            .or_else(|| read.pointer("/timeline/duration").and_then(Value::as_f64))
            .filter(|d| *d > 0.0)
            .ok_or_else(|| failed("the timeline is empty; add clips before watching a render"))?;
        let width = read
            .pointer("/project/width")
            .and_then(Value::as_f64)
            .unwrap_or(1920.0);
        let height = read
            .pointer("/project/height")
            .and_then(Value::as_f64)
            .unwrap_or(1080.0);
        let scale = (tile_width as f64 / width.max(height)).min(1.0);
        let pw = ((width * scale).round() as u32).clamp(16, 640);
        let ph = ((height * scale).round() as u32).clamp(16, 640);
        let times: Vec<f64> = match fields.get("times").and_then(Value::as_array) {
            Some(list) => list.iter().filter_map(Value::as_f64).collect(),
            None => (0..samples)
                .map(|i| duration * (i as f64 + 0.5) / samples as f64)
                .collect(),
        };
        let mut sources = Vec::with_capacity(times.len());
        for (i, at) in times.iter().enumerate() {
            let frame = backend.call(request(
                "preview.frame",
                json!({ "path": project, "time": at, "width": pw, "height": ph }),
            )?)?;
            let Some(png) = frame.get("png").and_then(Value::as_str) else {
                continue;
            };
            sources.push(json!({
                "pngB64": png,
                "at": (at * 1000.0).round() / 1000.0,
                "label": format!("{i:02} {:.1}s", at),
            }));
        }
        if sources.is_empty() {
            return Err(failed("preview.frame returned no pictures"));
        }
        spec["sources"] = json!(sources);
        spec["duration"] = json!(duration);
        caption = format!(
            "{} true composited frames from the open timeline ({duration:.1}s), in timeline order.",
            sources.len()
        );
    }
    let result = run("sheet", &spec, Duration::from_secs(180))?;
    Ok(image_outcome(&result, &caption))
}

/// `stock_contact_sheet`: actually LOOK at candidate B-roll before choosing
/// it, with an energy-centroid crop hint for each and optional semantic
/// ranking against the creative brief.
pub fn stock_contact_sheet(
    arguments: Value,
    backend: &mut dyn Backend,
) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let limit = (number(&fields, "limit", 8.0).round() as usize).clamp(1, 12);
    let mut clips: Vec<Value> = if let Some(query) = fields.get("query").and_then(Value::as_str) {
        let mut params = json!({ "query": query, "limit": limit });
        for key in ["minAesthetic", "minMotion", "orientation", "source", "sort"] {
            if let Some(v) = fields.get(key) {
                params[key] = v.clone();
            }
        }
        let found = backend.call(request("openvid.search", params)?)?;
        found
            .get("clips")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else if let Some(ids) = fields.get("ids").and_then(Value::as_array) {
        let ids: Vec<i64> = ids.iter().filter_map(|v| v.as_i64()).take(limit).collect();
        let got = backend.call(request("openvid.get", json!({ "ids": ids }))?)?;
        got.as_array().cloned().unwrap_or_else(|| {
            got.get("clips")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
    } else if let Some(paths) = fields.get("paths").and_then(Value::as_array) {
        paths
            .iter()
            .filter_map(Value::as_str)
            .map(|p| json!({ "localPath": p, "name": p }))
            .collect()
    } else {
        return Err(invalid("give `query`, `ids`, or `paths`"));
    };
    clips.truncate(limit);
    if clips.is_empty() {
        return Err(failed(
            "no stock clips matched; broaden the query or lower minAesthetic",
        ));
    }

    let mut sources = Vec::new();
    let mut meta = Vec::new();
    let mut errors = Vec::new();
    for (i, clip) in clips.iter().enumerate() {
        let id = clip.get("id").and_then(Value::as_i64);
        let mut path = clip
            .get("localPath")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let (true, Some(id)) = (path.is_none(), id) {
            match backend.call(request("openvid.fetch", json!({ "id": id }))?) {
                Ok(reply) => {
                    let list = reply.as_array().cloned().unwrap_or_else(|| {
                        reply
                            .get("clips")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default()
                    });
                    path = list
                        .first()
                        .and_then(|c| c.get("localPath"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                Err(e) => errors.push(format!("clip {id}: {e}")),
            }
        }
        let Some(path) = path else {
            errors.push(format!("candidate {i}: no local file"));
            continue;
        };
        let seconds = clip.get("seconds").and_then(Value::as_f64).unwrap_or(2.0);
        let caption = clip.get("caption").and_then(Value::as_str).unwrap_or("");
        sources.push(json!({
            "path": path,
            "at": (seconds * 0.35).min(1.5),
            "label": format!("{} {}", i + 1, caption.chars().take(22).collect::<String>()),
        }));
        meta.push(json!({
            "id": id,
            "name": clip.get("name").cloned().unwrap_or(Value::Null),
            "caption": caption,
            "aesthetic": clip.get("aesthetic").cloned().unwrap_or(Value::Null),
            "motion": clip.get("motion").cloned().unwrap_or(Value::Null),
            "camera": clip.get("camera").cloned().unwrap_or(Value::Null),
            "source": clip.get("source").cloned().unwrap_or(Value::Null),
            "localPath": path,
        }));
    }
    if sources.is_empty() {
        return Err(failed(format!(
            "no candidate clips could be fetched: {}",
            errors.join(" | ")
        )));
    }
    let mut spec = json!({
        "sources": sources,
        "cols": (number(&fields, "cols", 4.0).round() as u32).clamp(1, 6),
        "tileWidth": (number(&fields, "tileWidth", 320.0).round() as u32).clamp(64, 480),
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    if let Some(out) = fields.get("out").and_then(Value::as_str) {
        spec["out"] = json!(out);
    }
    let mut sheet = run("sheet", &spec, Duration::from_secs(240))?;
    // Attach the index metadata to each measured tile; rank() needs it.
    if let Some(tiles) = sheet.get_mut("tiles").and_then(Value::as_array_mut) {
        for (i, tile) in tiles.iter_mut().enumerate() {
            let index = tile
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or(i as u64) as usize;
            if let (Some(m), Some(map)) = (
                meta.get(index).and_then(Value::as_object),
                tile.as_object_mut(),
            ) {
                for (k, v) in m {
                    map.insert(k.clone(), v.clone());
                }
            }
        }
    }
    if !errors.is_empty() {
        sheet["fetchErrors"] = json!(errors);
    }
    if fields.get("rank").and_then(Value::as_bool).unwrap_or(false) {
        let brief = fields
            .get("brief")
            .cloned()
            .unwrap_or_else(|| load_brief(fields.get("project").and_then(Value::as_str)));
        let ranking = run(
            "rank",
            &json!({
                "sheet": sheet,
                "brief": brief,
                "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(false),
                "profile": fields.get("profile").cloned().unwrap_or(json!("mini")),
            }),
            Duration::from_secs(180),
        )?;
        sheet["ranking"] = json!(ranking);
    }
    Ok(image_outcome(
        &sheet,
        &format!(
            "{} B-roll candidates, one real frame each. Use cropHint to reframe off-centre subjects.",
            meta.len()
        ),
    ))
}

/// `craft_critique`: six-axis creative score and a short list of what to
/// change next. Uses the local VLM when one is up, measurements otherwise.
pub fn craft_critique(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let brief = fields
        .get("brief")
        .cloned()
        .unwrap_or_else(|| load_brief(fields.get("project").and_then(Value::as_str)));
    let mut spec = json!({
        "sheet": fields.get("sheet").cloned().unwrap_or(json!({})),
        "brief": brief,
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(false),
        "profile": fields.get("profile").cloned().unwrap_or(json!("mini")),
    });
    if let Some(png) = fields.get("pngB64").and_then(Value::as_str) {
        spec["pngB64"] = json!(png);
    }
    let result = run("critique", &spec, Duration::from_secs(240))?;
    Ok(Outcome::json(result))
}

/// `motion_overlay`: composite a transparent RGBA bake over real footage.
///
/// The motion sidecar's video codecs write opaque frames, and FFmpeg's WebM
/// muxer does not reliably write VP9's AlphaMode - so an "alpha video" would
/// be a lie. This honours the comp's transparency where it matters (at the
/// compositing step) and returns an ordinary H.264 clip the timeline already
/// knows how to blend, crop and cut.
pub fn motion_overlay(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let mut frames = fields
        .get("frames")
        .or_else(|| fields.get("dir"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if frames.is_none() {
        // A bake's hash is enough: find its frames under the project cache.
        if let (Some(project), Some(hash)) = (
            fields.get("project").and_then(Value::as_str),
            fields.get("hash").and_then(Value::as_str),
        ) {
            let cache = std::path::Path::new(project).join("cache").join("comps");
            if let Ok(entries) = fs::read_dir(&cache) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !name.starts_with(hash) {
                        continue;
                    }
                    let candidate = entry.path().join("frames");
                    if candidate.is_dir() {
                        frames = Some(candidate.display().to_string());
                        break;
                    }
                }
            }
        }
    }
    let frames = frames.ok_or_else(|| {
        invalid("give `frames` (the bake's frames/ directory) or `hash` + `project`")
    })?;
    let out = fields
        .get("out")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            fields
                .get("project")
                .and_then(Value::as_str)
                .map(|project| {
                    std::path::Path::new(project)
                        .join("motion")
                        .join("overlay.mp4")
                        .display()
                        .to_string()
                })
        })
        .ok_or_else(|| invalid("give `out` (a .mp4 path) or `project`"))?;
    let mut spec = json!({
        "frames": frames,
        "out": out,
        "fps": number(&fields, "fps", 30.0),
        "at": number(&fields, "at", 0.0),
        "crf": number(&fields, "crf", 18.0),
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    for key in ["plate", "width", "height", "background", "x", "y"] {
        if let Some(value) = fields.get(key) {
            spec[key] = value.clone();
        }
    }
    let mut result = run("overlay", &spec, Duration::from_secs(900))?;
    result["hint"] = json!(
        "Import this file with media_import, then clip_add it on the lane above the plate. \
         The original RGBA PNG sequence is untouched; re-run with a different plate or `at` to reposition."
    );
    Ok(Outcome::json(result))
}

/// `local_vlm`: status or first-use setup of the sub-1B local critic.
pub fn local_vlm(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let action = fields
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("status");
    if !["status", "setup"].contains(&action) {
        return Err(invalid("action is `status` or `setup`"));
    }
    let profile = fields
        .get("profile")
        .and_then(Value::as_str)
        .unwrap_or("mini");
    if !["mini", "image", "minicpm"].contains(&profile) {
        return Err(invalid("profile is `mini`, `image` or `minicpm`"));
    }
    let result = run(
        "vlm",
        &json!({ "action": action, "profile": profile }),
        Duration::from_secs(if action == "setup" { 900 } else { 30 }),
    )?;
    Ok(Outcome::json(result))
}

/// Where a project's creative brief lives. Keeping it in the project folder
/// (not in a chat transcript) means every later turn and every process can
/// read the same intent.
const BRIEF_FILE: &str = "creative-brief.json";
const BRIEF_KEYS: &[&str] = &[
    "audience",
    "platform",
    "goal",
    "thesis",
    "curiosityGap",
    "payoff",
    "emotionalArc",
    "referenceStyle",
    "tone",
    "durationSeconds",
    "brandKit",
    "constraints",
    "avoid",
];

fn load_brief(project: Option<&str>) -> Value {
    let Some(project) = project else {
        return json!({});
    };
    fs::read_to_string(std::path::Path::new(project).join(BRIEF_FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(json!({}))
}

/// Turn a brief into a prompt a director model can actually work from.
fn director_prompt(brief: &Value) -> String {
    let get = |k: &str| {
        brief
            .get(k)
            .map(Value::to_string)
            .unwrap_or_else(|| "unspecified".to_owned())
    };
    format!(
        "You are directing a short video, not assembling clips.\n\
         Audience: {audience}. Platform: {platform}. Goal: {goal}.\n\
         One-sentence thesis: {thesis}.\n\
         Open a curiosity gap: {gap}. Pay it off with: {payoff}.\n\
         Emotional arc: {arc}. Tone: {tone}. Reference style: {reference}.\n\
         Duration target: {duration} seconds. Constraints: {constraints}. Avoid: {avoid}.\n\
         Brand kit: {brand}.\n\n\
         Rules: every shot must earn its place. The first 1.5 seconds must create a \
         question or a feeling. Vary shot size, angle and motion between consecutive beats. \
         Cut on energy changes, not on a fixed grid. Captions add meaning; never restate the \
         voiceover verbatim. Build to the payoff, then stop. After rendering, call watch_render, \
         read the craft score and revise at least once before declaring success.",
        audience = get("audience"),
        platform = get("platform"),
        goal = get("goal"),
        thesis = get("thesis"),
        gap = get("curiosityGap"),
        payoff = get("payoff"),
        arc = get("emotionalArc"),
        tone = get("tone"),
        reference = get("referenceStyle"),
        duration = get("durationSeconds"),
        constraints = get("constraints"),
        avoid = get("avoid"),
        brand = get("brandKit"),
    )
}

/// `creative_brief`: read or write the project's persisted creative intent.
pub fn creative_brief(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let project = fields
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`path` is the project folder"))?;
    let action = fields
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("get");
    let mut brief = load_brief(Some(project));
    if action == "set" {
        let patch = fields
            .get("brief")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("`brief` is an object"))?;
        if !brief.is_object() {
            brief = json!({});
        }
        let map = brief.as_object_mut().expect("an object");
        for (key, value) in patch {
            if !BRIEF_KEYS.contains(&key.as_str()) {
                return Err(invalid(&format!("unknown brief field `{key}`")));
            }
            map.insert(key.clone(), value.clone());
        }
        fs::write(
            std::path::Path::new(project).join(BRIEF_FILE),
            serde_json::to_string_pretty(&brief).map_err(failed)?,
        )
        .map_err(|e| failed(format!("could not write {BRIEF_FILE}: {e}")))?;
    } else if action != "get" {
        return Err(invalid("action is `get` or `set`"));
    }
    let mut missing: Vec<&str> = BRIEF_KEYS[..6]
        .iter()
        .copied()
        .filter(|k| brief.get(*k).is_none())
        .collect();
    missing.sort_unstable();
    Ok(Outcome::json(json!({
        "file": std::path::Path::new(project).join(BRIEF_FILE).display().to_string(),
        "brief": brief,
        "missingCoreFields": missing,
        "directorPrompt": director_prompt(&brief),
        "hint": "Pass directorPrompt to the Forge pane, then call craft_critique after the first render.",
    })))
}
