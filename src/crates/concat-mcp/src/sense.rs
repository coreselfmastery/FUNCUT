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
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Once per process: unpack the embedded Python runtime. The directory
/// name carries the process id and a clock reading, so a neighbour on a
/// shared machine cannot pre-create it as a symlink and have the
/// sidecar's writes land somewhere they chose.
fn runtime() -> Result<&'static PathBuf, ApiError> {
    let result = RUNTIME.get_or_init(|| {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("concat-sense-{}-{stamp}", std::process::id()));
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

/// Take down every process in `leader`'s group. std cannot signal a
/// group, and this stays out of libc's dependency tree by asking the
/// shell's own `kill` instead - POSIX, no shell involved, the same binary
/// every Unix has in PATH since long before `kill` was a builtin.
#[cfg(unix)]
fn kill_group(leader: u32) {
    let _ = Command::new("kill")
        .arg("-9")
        .arg(format!("-{leader}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Run one sense command. stdout and stderr go to files: a contact sheet is
/// hundreds of KB of base64, which would deadlock a piped `wait_with_output`.
/// The spec travels by pipe for the same reason, and one more: Linux caps a
/// single argv string at 128 KiB, and a spec carrying real frames - or a
/// contact sheet on its way to the critic - is megabytes, so on the very
/// content the tool exists for, argv was `E2BIG` and the call simply failed.
fn run(cmd: &str, spec: &Value, timeout: Duration) -> Result<Value, ApiError> {
    let dir = runtime()?;
    let n = NONCE.fetch_add(1, Ordering::Relaxed);
    let out_path = dir.join(format!("out-{cmd}-{n}.json"));
    let err_path = dir.join(format!("err-{cmd}-{n}.log"));
    let out_file = File::create(&out_path).map_err(failed)?;
    let err_file = File::create(&err_path).map_err(failed)?;
    let spec = serde_json::to_string(spec).map_err(failed)?;
    let mut command = Command::new(python());
    command
        .arg(dir.join("sense.py"))
        .arg(cmd)
        .arg("--spec")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file));
    // A process group of its own, so a timeout can take the sidecar's
    // ffmpeg grandchildren down with it: an overlay encode left alive
    // would burn CPU for minutes after the tool had already given up.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        failed(format!(
            "could not run {}: {e}. Set FUNCUT_PYTHON to a Python 3 interpreter.",
            python()
        ))
    })?;
    // The writer owns the pipe from here and nothing else does: a child
    // that dies before reading the spec breaks the pipe instead of
    // deadlocking us, and a child that hangs is still covered by the
    // timeout below.
    let stdin = child.stdin.take();
    let bytes = spec.into_bytes();
    let writer = std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(&bytes);
        }
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    #[cfg(unix)]
                    kill_group(child.id());
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
    // The child has exited, so the pipe is closed and the writer ends on
    // its own; joining keeps its buffer alive until then.
    let _ = writer.join();
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
        // The tail of stderr, not its reversal: the last 300 characters,
        // read in order, are what names a Python traceback's actual error.
        let count = stderr.chars().count();
        let tail: String = stderr.chars().skip(count.saturating_sub(300)).collect();
        map.insert("stderrTail".to_owned(), json!(tail));
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

/// A path an agent named as somewhere to write, held to the same line the
/// API draws for its own writes: no `..`, and under one of the backend's
/// roots when it has any. A relative name lands under `base`. The sense
/// layer writes files of its own - contact sheets, encoded bakes, the
/// craft journal - and before this those writes went wherever an agent
/// (or a prompt-injected agent) pointed them, which the setup table
/// promises they cannot.
pub(crate) fn write_target(
    backend: &dyn Backend,
    base: Option<&Path>,
    given: &str,
) -> Result<PathBuf, ApiError> {
    let trimmed = given.trim();
    if trimmed.is_empty() {
        return Err(invalid("the write path is empty"));
    }
    let given = Path::new(trimmed);
    if given
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(invalid("the write path may not climb with `..`"));
    }
    let target = if given.is_absolute() {
        given.to_path_buf()
    } else {
        match base {
            Some(base) => base.join(given),
            None => std::env::current_dir().map_err(failed)?.join(given),
        }
    };
    let roots = backend.roots();
    if roots.is_empty() {
        return Ok(target);
    }
    // Resolve what exists and compare lexically past that, the way the
    // API's own check does, so a path that leaves a root only through a
    // symlink still has to say so.
    let resolved = if let Ok(real) = target.canonicalize() {
        real
    } else {
        // Not there yet (the usual case: the file is the point of the
        // write). Resolve the deepest ancestor that exists and rejoin the
        // rest, which a created directory is then checked against.
        let mut existing = target.clone();
        let mut tail = PathBuf::new();
        while let Some(parent) = existing.parent().map(Path::to_path_buf) {
            if existing.canonicalize().is_ok() || parent.as_os_str().is_empty() {
                break;
            }
            if let Some(name) = existing.file_name() {
                tail = Path::new(name).join(&tail);
            }
            existing = parent;
        }
        existing
            .canonicalize()
            .map(|real| real.join(tail))
            .unwrap_or_else(|_| target.clone())
    };
    if roots.iter().any(|root| resolved.starts_with(root)) {
        Ok(target)
    } else {
        let list = roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Err(invalid(&format!(
            "{} is outside where this session writes ({list})",
            target.display()
        )))
    }
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
    let project = fields.get("project").and_then(Value::as_str);
    let mut spec = json!({
        "samples": samples,
        "cols": cols,
        "tileWidth": tile_width,
        "tileHeight": tile_height,
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    for key in ["targetLufs", "targetTruePeakDbtp"] {
        if let Some(value) = fields.get(key) {
            spec[key] = value.clone();
        }
    }

    let caption;
    // Set when the agent pinned exact frames, which replaces the sampling
    // the audio measurement rides on; the reply says so instead of the
    // missing loudness reading having to.
    let mut pinned_frames = false;
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
                pinned_frames = true;
            }
        }
        caption =
            format!("{samples} frames sampled from the rendered file {file}, in timeline order.");
    } else {
        // No file: ask the editor itself for the true composited frames.
        let Some(project) = project else {
            return Err(invalid(
                "give `path` (a rendered movie) or `project` (an open project)",
            ));
        };
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
    if let Some(out) = fields.get("out").and_then(Value::as_str) {
        spec["out"] = json!(
            write_target(backend, project.map(Path::new), out)?
                .display()
                .to_string()
        );
    } else if let Some(project) = project {
        // Keep the sheet on disk where a later craft_critique can read it.
        // The agent sees the image inline but cannot hand those bytes back
        // in a tool call, so the sheet's `pngPath` is how the critic looks
        // at the same frames - without it the documented critique loop
        // could never see a picture at all.
        let n = NONCE.fetch_add(1, Ordering::Relaxed);
        let sheet = Path::new(project)
            .join("cache")
            .join("sense")
            .join(format!("sheet-{n}.png"));
        spec["out"] = json!(
            write_target(
                backend,
                Some(Path::new(project)),
                &sheet.display().to_string()
            )?
            .display()
            .to_string()
        );
    }
    let mut result = run("sheet", &spec, Duration::from_secs(180))?;
    if pinned_frames {
        if let Some(notes) = result.get_mut("notes").and_then(Value::as_array_mut) {
            notes.push(json!(
                "`times` pinned the exact frames, so the audio envelope and loudness were not measured; call measure_loudness on the file for the mix."
            ));
        }
    }
    Ok(image_outcome(&result, &caption))
}

/// The clip_transform offset that brings an off-centre subject into a
/// filled frame. `cropHint` is an energy centroid in -1..1; OpenVid's
/// Pixabay/Pexels/CelebV shelves are square, so filling a frame of aspect
/// `aw/ah` scales the square to max(frameW, frameH) and the overhang sits
/// on ONE axis only: the horizontal one for a portrait frame, the vertical
/// one for a landscape frame, neither for a square. An offset is a
/// fraction of the output width (x) or height (y), and the per-side
/// overhang in exactly those units - half the surplus dimension, as a
/// fraction of the filled axis - is both the gain on the hint and the
/// clamp: a hint of +/-1 spends the whole overhang and can never push
/// past it into a background strip. A square frame has no overhang, so
/// there is nothing to reframe and the offsets are zero whatever the
/// hint says.
fn reframe_offset(aspect: &str, hint_x: f64, hint_y: f64) -> (f64, f64) {
    let (aw, ah) = match aspect {
        "16:9" => (16.0_f64, 9.0),
        "1:1" => (1.0, 1.0),
        "4:5" => (4.0, 5.0),
        _ => (9.0, 16.0),
    };
    let round = |v: f64| (v * 100.0).round() / 100.0;
    if aw > ah {
        // Landscape: the square fills the width, overhang is vertical, and
        // (aw/ah - 1)/2 is half the surplus height in units of frame height.
        let room = (aw / ah - 1.0) / 2.0;
        (0.0, round((-hint_y * room).clamp(-room, room)))
    } else {
        // Portrait: the square fills the height, overhang is horizontal,
        // and (ah/aw - 1)/2 is half the surplus width in units of frame
        // width. A square frame gives room 0 and offsets 0.
        let room = (ah / aw - 1.0) / 2.0;
        (round((-hint_x * room).clamp(-room, room)), 0.0)
    }
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
        spec["out"] = json!(
            write_target(
                backend,
                fields.get("project").and_then(Value::as_str).map(Path::new),
                out
            )?
            .display()
            .to_string()
        );
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
    // cropHint is an energy centroid in -1..1. Turn it into the exact
    // clip_transform offsets a vertical cut needs, so an agent does not
    // have to derive the geometry (and so it stops centre-cropping every
    // square OpenVid clip). Pixabay/Pexels/CelebV sources are 512x512;
    // filling a 9:16 frame scales them to the frame height, so the
    // horizontal overflow is (frameH - frameW)/2 on each side.
    let aspect = fields
        .get("aspect")
        .and_then(Value::as_str)
        .unwrap_or("9:16");
    if let Some(tiles) = sheet.get_mut("tiles").and_then(Value::as_array_mut) {
        for tile in tiles.iter_mut() {
            let hint = tile
                .pointer("/cropHint/x")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let hint_y = tile
                .pointer("/cropHint/y")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let (offset_x, offset_y) = reframe_offset(aspect, hint, hint_y);
            tile["reframeHint"] = json!({
                "aspect": aspect,
                "clipTransform": { "offsetX": offset_x, "offsetY": offset_y },
                "use": "After clip_frame {framing:'fill'}, call clip_transform with these offsets on the clip, or keyframe_set to follow the subject.",
            });
        }
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
                // The tool's own default, not the sub-call's old `false`:
                // a machine where the model is not up yet otherwise got a
                // silent pixel-only ranking with a note naming a tool that
                // does not exist, and never knew what it had missed.
                "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
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
pub fn craft_critique(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
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
    if let Some(path) = fields.get("pngPath").and_then(Value::as_str) {
        spec["pngPath"] = json!(path);
    }
    let mut result = run("critique", &spec, Duration::from_secs(240))?;
    if let Some(project) = fields.get("project").and_then(Value::as_str) {
        record_craft_pass(project, &mut result, backend);
    }
    Ok(Outcome::json(result))
}

/// Append one critique pass to the project's craft journal and annotate the
/// reply with how it compares to the previous pass. Keeping this in the
/// project - not in a chat transcript - is what makes a multi-turn
/// render -> inspect -> revise loop measurable. Every way this can fail is
/// said in the reply: a journal that cannot be read, or a pass that cannot
/// be written, used to pass silently while the annotations claimed history.
fn record_craft_pass(project: &str, result: &mut Value, backend: &dyn Backend) {
    let path = Path::new(project).join("craft-history.json");
    let mut notes: Vec<String> = Vec::new();
    let mut history: Vec<Value> = match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_else(|_| {
            notes.push(
                "the existing craft-history.json could not be parsed; a fresh journal was started"
                    .to_owned(),
            );
            Vec::new()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            notes.push(format!(
                "craft-history.json could not be read ({e}); a fresh journal was started"
            ));
            Vec::new()
        }
    };
    let previous = history
        .last()
        .and_then(|entry| entry.get("overall").and_then(Value::as_f64));
    let overall = result.get("overall").and_then(Value::as_f64);
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    history.push(json!({
        "at": seconds,
        "engine": result.get("engine").cloned().unwrap_or(Value::Null),
        "overall": overall,
        "scores": result.get("scores").cloned().unwrap_or(Value::Null),
        "notes": result.get("notes").cloned().unwrap_or(Value::Null),
        "revisions": result.get("revisions").cloned().unwrap_or(Value::Null),
    }));
    if history.len() > 20 {
        history.drain(..history.len() - 20);
    }
    let journaled =
        write_target(backend, Some(Path::new(project)), "craft-history.json").and_then(|path| {
            let text = serde_json::to_string_pretty(&history).map_err(failed)?;
            fs::write(&path, text).map_err(|e| failed(format!("could not write: {e}")))
        });
    if let Err(why) = &journaled {
        notes.push(format!("the pass could not be journaled ({why})"));
    }
    result["pass"] = json!(history.len());
    result["historyFile"] = json!(path.display().to_string());
    if let (Some(now), Some(before)) = (overall, previous) {
        let delta = ((now - before) * 10.0).round() / 10.0;
        result["previousOverall"] = json!(before);
        result["delta"] = json!(delta);
        result["improved"] = json!(delta > 0.0);
        result["trend"] = json!(if delta > 0.3 {
            "better than the last pass"
        } else if delta < -0.3 {
            "worse than the last pass - undo or rethink this revision"
        } else {
            "no measurable change - the revision did not affect the frames"
        });
    } else {
        result["trend"] = json!("first measured pass; revise and critique again to compare");
    }
    if !notes.is_empty() {
        if let Some(list) = result.get_mut("notes").and_then(Value::as_array_mut) {
            for note in notes {
                list.push(json!(note));
            }
        } else {
            result["notes"] = json!(notes);
        }
    }
}

/// `suggest_cuts`: beat-aligned cut times from the soundtrack, with no
/// Python extras. `analyze_beats` needs librosa; this works with ffmpeg
/// alone, so a headless agent on a small machine can still cut on energy.
pub fn suggest_cuts(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let path = fields
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| invalid("`path` is the media file (music or voiceover) to analyse"))?;
    let mut spec = json!({
        "path": path,
        "cuts": number(&fields, "cuts", 12.0),
        "minGap": number(&fields, "minGap", 0.35),
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    if let Some(bpm) = fields.get("bpm") {
        spec["bpm"] = bpm.clone();
    }
    if let Some(ff) = fields.get("ffmpeg") {
        spec["ffmpeg"] = ff.clone();
    }
    let mut result = run("rhythm", &spec, Duration::from_secs(240))?;
    result["hint"] = json!(
        "These are energy onsets, not a finished edit. Use them for section changes and entrances, keep a few cuts deliberately OFF the pulse, and never place more cuts than the story needs. Feed the times into plan_validate as section boundaries."
    );
    Ok(Outcome::json(result))
}

/// `measure_loudness`: the exported mix's ACTUAL integrated LUFS and true
/// peak, not the plan's target. `export_wait` reports no loudness, so this
/// is the only headless way to prove a platform target was met.
pub fn measure_loudness(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let path = fields
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| invalid("`path` is the media file to measure"))?;
    let mut spec = json!({
        "path": path,
        "targetLufs": number(&fields, "targetLufs", -14.0),
        "targetTruePeakDbtp": number(&fields, "targetTruePeakDbtp", -1.0),
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    if let Some(ff) = fields.get("ffmpeg") {
        spec["ffmpeg"] = ff.clone();
    }
    let mut result = run("loudness", &spec, Duration::from_secs(300))?;
    result["hint"] = json!(
        "Short-form platforms sit near -14 LUFS integrated with a true peak under -1 dBTP; broadcast EBU R128 is -23 LUFS. A plan's target is a wish - this is the measurement. Fix with clip_audio_levels_db and measure again; never claim compliance from a plan."
    );
    Ok(Outcome::json(result))
}

/// `motion_overlay`: composite a transparent RGBA bake over real footage.
///
/// The motion sidecar's video codecs write opaque frames, and FFmpeg's WebM
/// muxer does not reliably write VP9's AlphaMode - so an "alpha video" would
/// be a lie. This honours the comp's transparency where it matters (at the
/// compositing step) and returns an ordinary H.264 clip the timeline already
/// knows how to blend, crop and cut.
pub fn motion_overlay(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
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
    let project = fields.get("project").and_then(Value::as_str);
    let out = fields
        .get("out")
        .and_then(Value::as_str)
        .map(|out| {
            write_target(backend, project.map(Path::new), out)
                .map(|path| path.display().to_string())
        })
        .transpose()?
        .or_else(|| {
            project.map(|project| {
                Path::new(project)
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
pub fn creative_brief(arguments: Value, backend: &dyn Backend) -> Result<Outcome, ApiError> {
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
        // A `set` is a write into the project folder, so it is held to the
        // same line every other write is - an agent pointed at a folder
        // outside the session's roots gets the refusal here, not a file.
        let target = write_target(backend, Some(Path::new(project)), BRIEF_FILE)?;
        fs::write(
            &target,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use concat_api::{ApiError, Request};

    /// A backend that confines nothing, the way the window's does: enough
    /// for the journal test, which is about the file, not the confinement.
    struct NoRoots;

    impl Backend for NoRoots {
        fn call(&mut self, _request: Request) -> Result<Value, ApiError> {
            Err(ApiError::failed("not used"))
        }
        fn describe(&self) -> Value {
            json!({ "mode": "test" })
        }
    }

    #[test]
    fn craft_journal_tracks_improvement_and_regression() {
        let dir = std::env::temp_dir().join(format!("concat-craft-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let project = dir.to_string_lossy().into_owned();

        let mut first = json!({ "engine": "heuristic", "overall": 5.5, "scores": {}, "notes": [], "revisions": [] });
        record_craft_pass(&project, &mut first, &NoRoots);
        assert_eq!(first["pass"], 1);
        assert!(
            first["trend"]
                .as_str()
                .unwrap_or_default()
                .contains("first")
        );

        let mut second = json!({ "engine": "local-vlm", "overall": 7.2, "scores": {}, "notes": [], "revisions": [] });
        record_craft_pass(&project, &mut second, &NoRoots);
        assert_eq!(second["pass"], 2);
        assert_eq!(second["delta"], 1.7);
        assert_eq!(second["improved"], true);

        let mut third = json!({ "engine": "local-vlm", "overall": 6.0, "scores": {}, "notes": [], "revisions": [] });
        record_craft_pass(&project, &mut third, &NoRoots);
        assert_eq!(third["improved"], false);
        assert!(
            third["trend"]
                .as_str()
                .unwrap_or_default()
                .contains("worse")
        );

        let saved: Vec<Value> = serde_json::from_str(
            &fs::read_to_string(dir.join("craft-history.json")).expect("the journal is written"),
        )
        .expect("valid json");
        assert_eq!(saved.len(), 3);

        // A corrupt journal is said, not silently reset: the reply carries
        // what happened, and the pass after it starts from an empty file.
        fs::write(dir.join("craft-history.json"), "{not json").expect("corrupt it");
        let mut fourth = json!({ "engine": "heuristic", "overall": 5.0, "scores": {}, "notes": [], "revisions": [] });
        record_craft_pass(&project, &mut fourth, &NoRoots);
        let notes = serde_json::to_string(&fourth["notes"]).unwrap_or_default();
        assert!(notes.contains("could not be parsed"), "{notes}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reframe_offset_follows_the_subject_within_the_overhang() {
        // A subject left of centre (hint -0.5) needs the image moved RIGHT.
        // A square source in a 9:16 frame overhangs horizontally by
        // (16/9 - 1)/2 = 0.389 of the frame width on each side: the hint
        // spends part of it, and can never be pushed past it.
        let (x, y) = reframe_offset("9:16", -0.5, 0.0);
        assert!((0.19..=0.2).contains(&x), "x = {x}");
        assert_eq!(y, 0.0, "a portrait frame has no vertical overhang");
        let (x2, _) = reframe_offset("9:16", 0.5, 0.0);
        assert!((-0.2..=-0.19).contains(&x2), "x2 = {x2}");
        assert_eq!(reframe_offset("9:16", 0.0, 0.0), (0.0, 0.0));
        // A hint beyond +/-1 is clamped to the overhang itself, not past it.
        assert_eq!(reframe_offset("9:16", 99.0, 0.0).0, -0.39);
        // 16:9: the overhang moves to the vertical axis, in frame heights.
        let (x, y) = reframe_offset("16:9", 0.0, -0.5);
        assert_eq!(x, 0.0, "a landscape frame has no horizontal overhang");
        assert!((0.19..=0.2).contains(&y), "y = {y}");
        assert_eq!(reframe_offset("16:9", 0.0, 99.0).1, 0.39);
        // 4:5: barely portrait, so barely any room - the old code's 0.45
        // clamp here could push a clip mostly off its source.
        let (x, y) = reframe_offset("4:5", 1.0, 1.0);
        assert_eq!(x, -0.13);
        assert_eq!(y, 0.0);
        // 1:1: a square source fills a square frame exactly; there is
        // nothing to reframe and any hint pretending otherwise moves
        // nothing, rather than revealing a background strip.
        assert_eq!(reframe_offset("1:1", 0.8, -0.8), (0.0, 0.0));
    }

    #[test]
    fn base64_round_trips_a_png_header() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x01\xff";
        assert_eq!(base64(png), "iVBORw0KGgoAAf8=");
    }
}
