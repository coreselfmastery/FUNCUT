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

use std::collections::HashMap;
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
const EMOTION_PY: &str = include_str!("../sense/emotion.py");
const VOICE_SIGNALS_PY: &str = include_str!("../sense/voice_signals.py");
const DISCOURSE_PY: &str = include_str!("../sense/discourse.py");
const VOICE_QUALITY_PY: &str = include_str!("../sense/voice_quality.py");
const PROSODY_STYLE_PY: &str = include_str!("../sense/prosody_style.py");
const AUDIO_REPAIR_PY: &str = include_str!("../sense/audio_repair.py");
const CAPTIONS_PY: &str = include_str!("../sense/captions.py");
const FARM_SYNC_PY: &str = include_str!("../sense/farm-sync.py");
/// The semantic search step, at the repo's `analysis/rerank.py`: the CLIP
/// rerank `openvid_search` runs behind BM25. It lands in the same runtime
/// directory as the other scripts, so it can import their ffmpeg helpers.
const RERANK_PY: &str = include_str!("../../../../analysis/rerank.py");
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
        fs::write(dir.join("emotion.py"), EMOTION_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("voice_signals.py"), VOICE_SIGNALS_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("discourse.py"), DISCOURSE_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("voice_quality.py"), VOICE_QUALITY_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("prosody_style.py"), PROSODY_STYLE_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("audio_repair.py"), AUDIO_REPAIR_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("captions.py"), CAPTIONS_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("farm-sync.py"), FARM_SYNC_PY).map_err(|e| e.to_string())?;
        fs::write(dir.join("rerank.py"), RERANK_PY).map_err(|e| e.to_string())?;
        Ok(dir)
    });
    result.as_ref().map_err(failed)
}

fn python() -> String {
    std::env::var("FUNCUT_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.to_owned())
}

/// The Rust ears' binary beside an executable, when it is there: the
/// same place every bundle stages it and the Geneva sidecar. Pure, so a
/// test can drive it with a made-up folder.
pub(crate) fn voice_signals_beside(exe: &Path) -> Option<std::path::PathBuf> {
    let beside = exe.parent()?.join("voice-signals");
    beside.is_file().then_some(beside)
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
    run_py(&dir.join("sense.py"), cmd, spec, timeout)
}

/// A named script's path in the runtime directory the sidecars unpacked
/// into: the door a tool in another module calls one through - the motion
/// tools' cloud farm, for one, reaches farm-sync.py this way.
pub(crate) fn runtime_script(name: &str) -> Result<PathBuf, ApiError> {
    Ok(runtime()?.join(name))
}

/// [`run`], naming the script: sense.py for the pixel and sound commands,
/// rerank.py for the semantic search step - both unpacked into the same
/// runtime directory, which is what lets one import the other's helpers.
/// farm-sync.py, the cloud farm's legs, is unpacked beside them and called
/// the same way by the motion tools.
pub(crate) fn run_py(
    script: &Path,
    cmd: &str,
    spec: &Value,
    timeout: Duration,
) -> Result<Value, ApiError> {
    let dir = runtime()?;
    let n = NONCE.fetch_add(1, Ordering::Relaxed);
    let out_path = dir.join(format!("out-{cmd}-{n}.json"));
    let err_path = dir.join(format!("err-{cmd}-{n}.log"));
    let out_file = File::create(&out_path).map_err(failed)?;
    let err_file = File::create(&err_path).map_err(failed)?;
    let spec = serde_json::to_string(spec).map_err(failed)?;
    let mut command = Command::new(python());
    command
        .arg(script)
        .arg(cmd)
        .arg("--spec")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file));
    // The Rust ears' binary ships beside the executable in every bundle
    // (AppImage, .deb, .rpm, Arch), the way the Geneva sidecar does. The
    // Python bridge looks for it through FUNCUT_VOICE_SIGNALS_BIN first,
    // so the pin is handed over when the binary is there and the caller
    // has not pinned one of their own - and the prosody, forensic,
    // speaker and NER readings answer instead of noting their absence.
    if std::env::var_os("FUNCUT_VOICE_SIGNALS_BIN").is_none()
        && let Ok(exe) = std::env::current_exe()
        && let Some(beside) = voice_signals_beside(&exe)
    {
        command.env("FUNCUT_VOICE_SIGNALS_BIN", beside);
    }
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
                    // The group first, while the leader still exists and its
                    // pid is certainly its pgid: a kill-and-reap can land on
                    // a recycled id, and the ffmpeg grandchildren are the
                    // expensive part anyway. The call's temp files leave
                    // with the error too - a timed-out call used to leak
                    // both into the runtime dir.
                    #[cfg(unix)]
                    kill_group(child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(&out_path);
                    let _ = fs::remove_file(&err_path);
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
        // The tail, not the head: argparse noise opens stderr, and the line
        // that names a Python traceback's actual error closes it.
        let count = stderr.chars().count();
        let tail: String = stderr.chars().skip(count.saturating_sub(500)).collect();
        failed(format!("sense {cmd} returned no JSON ({e}). {tail}"))
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

/// `openvid_search`'s second half: the BM25 page, re-read by a CLIP.
///
/// The index ranks the captions that carry the words; a query like "the
/// moment a decision goes wrong" deserves the clips that SHOW it. The
/// rerank runs only when it can: a textless query, a page sorted by
/// something other than relevance, a page past the first, and a page of
/// ten or fewer all keep the BM25 order untouched. The shortlist is the
/// page BM25 gave, capped at fifty; the clips whose files are already
/// local are scored straight away and the rest are fetched under a time
/// budget, so a first search pays for a few downloads and every later
/// one is the ~200ms the model costs. Whatever the Python half answers -
/// or fails with - lands in the reply as `rerank`, never as an error.
pub fn semantic_rerank(arguments: &Value, mut found: Value, backend: &mut dyn Backend) -> Value {
    /// The shortlist the rerank sees: wide enough that the meaning has
    /// room to be on the page, small enough that scoring it stays cheap.
    const POOL: usize = 50;
    /// A page this short is already the answer; ranking it cannot help.
    const TINY: usize = 10;
    /// What fetching may cost before the rest keep their BM25 places.
    const FETCH_BUDGET: Duration = Duration::from_secs(20);

    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty());
    let sort = arguments
        .get("sort")
        .and_then(Value::as_str)
        .unwrap_or("relevance");
    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0);
    if query.is_none()
        || !sort.eq_ignore_ascii_case("relevance")
        || offset > 0
        || found
            .get("clips")
            .and_then(Value::as_array)
            .is_none_or(|clips| clips.len() <= TINY)
    {
        return found;
    }
    let Some(query) = query else {
        return found;
    };
    // The rerank cannot run without its model, and a model that is not
    // installed must not be paid for: fetching fifty candidate videos
    // for a second pass that can only report its own absence is minutes
    // of network for a page that keeps its BM25 order anyway.
    // `openvid.status` names whether the two ONNX files are in place -
    // the same directories rerank.py looks in, so what this asks is what
    // the script itself would find - and the page answers with the BM25
    // order and the reason before a single byte moves. A status that
    // cannot be asked is not a verdict: the optimistic path keeps the
    // old fetch-then-find-out behaviour for backends that do not speak
    // for the model.
    let rerank_ready = request("openvid.status", json!({}))
        .and_then(|status| backend.call(status))
        .ok()
        .and_then(|reply| {
            reply
                .get("rerank")
                .and_then(|rerank| rerank.get("available"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(true);
    if !rerank_ready {
        if let Some(slot) = found.as_object_mut() {
            slot.insert(
                "rerank".into(),
                json!({
                    "applied": false,
                    "reason": "the vision rerank model is not installed - openvid_status names \
                               where its two files go; the BM25 order stands, and \
                               stock_contact_sheet is the way to look at the page",
                }),
            );
        }
        return found;
    }
    let page: Vec<Value> = found
        .get("clips")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut page = page.into_iter();
    let shortlist: Vec<Value> = page.by_ref().take(POOL).collect();
    let rest: Vec<Value> = page.collect();

    // Two looks per clip, early and middle, the sheet's own timing; a
    // clip too short for two gets one. Files already local are read as
    // they are; the rest are fetched until the budget runs out, and the
    // clips it left behind keep their BM25 places.
    let deadline = Instant::now() + FETCH_BUDGET;
    let mut frames: Vec<Value> = Vec::new();
    let mut fetched = 0_usize;
    for clip in &shortlist {
        let Some(id) = clip.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let seconds = clip.get("seconds").and_then(Value::as_f64).unwrap_or(2.0);
        let mut path = clip
            .get("localPath")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if path.is_none() && Instant::now() < deadline {
            // The fetch call, like the rerank around it, never fails the
            // reply: a request that cannot even be built - and with a
            // literal object it cannot - leaves the clip its BM25 place.
            let Ok(fetch) = request("openvid.fetch", json!({ "id": id })) else {
                continue;
            };
            match backend.call(fetch) {
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
                        .and_then(|clip| clip.get("localPath"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    if path.is_some() {
                        fetched += 1;
                    }
                }
                Err(_) => continue,
            }
        }
        let Some(path) = path else {
            continue;
        };
        frames.push(json!({
            "id": id,
            "path": path.clone(),
            "at": (seconds * 0.35).clamp(0.0, 1.5),
        }));
        if seconds > 1.2 {
            frames.push(json!({
                "id": id,
                "path": path,
                "at": (seconds * 0.65).clamp(0.0, 3.0),
            }));
        }
    }
    if frames.is_empty() {
        if let Some(slot) = found.as_object_mut() {
            slot.insert(
                "rerank".into(),
                json!({ "applied": false, "reason": "no candidate frames were reachable" }),
            );
        }
        return found;
    }

    let script = match runtime() {
        Ok(dir) => dir.join("rerank.py"),
        Err(error) => {
            if let Some(slot) = found.as_object_mut() {
                slot.insert(
                    "rerank".into(),
                    json!({ "applied": false, "reason": error.to_string() }),
                );
            }
            return found;
        }
    };
    // The shortlist's ids in the order BM25 gave them: whatever the reply
    // never named goes back into that order, not a hash map's.
    let ids_in_order: Vec<i64> = shortlist
        .iter()
        .filter_map(|clip| clip.get("id").and_then(Value::as_i64))
        .collect();
    let mut by_id: HashMap<i64, Value> = HashMap::new();
    for clip in &shortlist {
        if let Some(id) = clip.get("id").and_then(Value::as_i64) {
            by_id.insert(id, clip.clone());
        }
    }
    let spec = json!({
        "query": query,
        "frames": frames,
        "candidates": shortlist,
        "autoSetup": true,
    });
    match run_py(&script, "rerank", &spec, Duration::from_secs(240)) {
        Ok(reply) => {
            let order: Vec<i64> = reply
                .get("order")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            let mut ordered: Vec<Value> = Vec::with_capacity(order.len() + rest.len());
            for id in &order {
                if let Some(clip) = by_id.remove(id) {
                    ordered.push(clip);
                }
            }
            for id in &ids_in_order {
                if let Some(clip) = by_id.remove(id) {
                    ordered.push(clip);
                }
            }
            ordered.extend(by_id.into_values());
            ordered.extend(rest);
            let scored = reply.get("scored").and_then(Value::as_u64).unwrap_or(0);
            let ms = reply.get("ms").and_then(Value::as_u64).unwrap_or(0);
            if let Some(slot) = found.as_object_mut() {
                slot.insert("clips".into(), Value::Array(ordered));
                slot.insert(
                    "rerank".into(),
                    json!({
                        "applied": true,
                        "engine": "mobileclip2-s0",
                        "scored": scored,
                        "fetched": fetched,
                        "ms": ms,
                    }),
                );
            }
        }
        Err(error) => {
            if let Some(slot) = found.as_object_mut() {
                slot.insert(
                    "rerank".into(),
                    json!({ "applied": false, "reason": error.to_string() }),
                );
            }
        }
    }
    found
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
    let mut content = Vec::with_capacity(2);
    // An empty image block - a reply whose picture never materialised - is a
    // base64 decoding error on a strict client, for a sheet that is
    // otherwise fine: send the measurements and let the caption say what
    // happened, instead of an image block no renderer can read.
    if !png.is_empty() {
        content.push(json!({ "type": "image", "data": png, "mimeType": "image/png" }));
    }
    content.push(json!({ "type": "text", "text": format!("{caption}\n\n{text}") }));
    Outcome {
        content,
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
///
/// A backend with no roots - the window's - is not a licence for every
/// path: the writes this layer makes locally are names under the project
/// the tool was pointed at, never absolute routes, and an absolute path
/// is refused here rather than clobbering whatever it named. The window's
/// own refusal never sees these writes - they never leave this process.
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
    let roots = backend.roots();
    if roots.is_empty() && given.is_absolute() {
        return Err(invalid(
            "the write path is a name under the project, not an absolute route out of it",
        ));
    }
    let target = if given.is_absolute() {
        given.to_path_buf()
    } else {
        match base {
            Some(base) => base.join(given),
            None => std::env::current_dir().map_err(failed)?.join(given),
        }
    };
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

/// The sheet's own tile ceiling (sense.py caps its sources at 24): an
/// agent-supplied `times` list longer than it is a render paid for and
/// never drawn, so both `watch_render` paths cut it down to this before
/// doing any work.
const MAX_SHEET_TILES: usize = 24;

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
                .take(MAX_SHEET_TILES)
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
            // MAX_SHEET_TILES is the sheet's own tile ceiling: every entry
            // past it is a synchronous preview.frame render whose tile is
            // dropped before it is ever drawn - paid for, then thrown away.
            Some(list) => list
                .iter()
                .filter_map(Value::as_f64)
                .take(MAX_SHEET_TILES)
                .collect(),
            None => (0..samples)
                .map(|i| duration * (i as f64 + 0.5) / samples as f64)
                .collect(),
        };
        let mut sources = Vec::with_capacity(times.len());
        let mut dropped = 0u32;
        for (i, at) in times.iter().enumerate() {
            // One flaky frame render used to fail the whole sheet: skip it
            // and keep watching. A dropped tile costs one sample; an error
            // here cost the agent its only look at the cut.
            let frame = match backend.call(request(
                "preview.frame",
                json!({ "path": project, "time": at, "width": pw, "height": ph }),
            )?) {
                Ok(frame) => frame,
                Err(_) => {
                    dropped += 1;
                    continue;
                }
            };
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
            "{} true composited frames from the open timeline ({duration:.1}s), in timeline order.{}",
            sources.len(),
            if dropped > 0 {
                format!(" {dropped} sampled frames failed to render and were skipped.")
            } else {
                String::new()
            }
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
        //
        // A relative name, held to the project by the base: an absolute
        // path is refused outright on the window backend, which serves
        // no write roots, and `--mcp auto` picks that backend whenever
        // the window is open - the default deployment. The siblings
        // (craft-history.json, the brief) pass relative names for
        // exactly this reason.
        let n = NONCE.fetch_add(1, Ordering::Relaxed);
        let sheet = Path::new("cache")
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
    if pinned_frames && let Some(notes) = result.get_mut("notes").and_then(Value::as_array_mut) {
        notes.push(json!(
            "`times` pinned the exact frames, so the audio envelope and loudness were not measured; call measure_loudness on the file for the mix."
        ));
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
        // The same degradation contract `semantic_rerank` keeps: the sheet
        // and its measured pixels are the point of the tool, and a rank
        // that cannot run (the model not downloaded yet, a slow first
        // setup) must not throw the whole sheet away after it was built.
        let ranking = match run(
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
        ) {
            Ok(ranking) => ranking,
            Err(error) => json!({
                "applied": false,
                "reason": error.to_string(),
                "use": "The sheet and its measured pixels are intact; rank again after local_vlm setup.",
            }),
        };
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
    // The sheet arrives two ways: the whole reading object a render
    // returned, or the bare pngPath string the reel contact sheet's reply
    // names - the documented loop says the string, and the schema used to
    // refuse it while the docs said to send it.
    let sheet = match fields.get("sheet") {
        Some(Value::String(path)) => json!({ "pngPath": path }),
        other => other.cloned().unwrap_or(json!({})),
    };
    let mut spec = json!({
        "sheet": sheet,
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
        "fixes": result.get("fixes").cloned().unwrap_or(Value::Null),
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
    // No `ffmpeg` here, by design: the executable that runs is the
    // operator's (FUNCUT_FFMPEG, the PATH, or the pinned bootstrap) and
    // never the agent's to name - a tool that took a binary path from
    // its caller was arbitrary execution wearing a media tool's name.
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
    let spec = json!({
        "path": path,
        "targetLufs": number(&fields, "targetLufs", -14.0),
        "targetTruePeakDbtp": number(&fields, "targetTruePeakDbtp", -1.0),
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    // As `suggest_cuts`: the binary is never the caller's to choose.
    let mut result = run("loudness", &spec, Duration::from_secs(300))?;
    result["hint"] = json!(
        "Short-form platforms sit near -14 LUFS integrated with a true peak under -1 dBTP; broadcast EBU R128 is -23 LUFS. A plan's target is a wish - this is the measurement. Fix with clip_audio_levels_db and measure again; never claim compliance from a plan."
    );
    Ok(Outcome::json(result))
}

/// `track_subject`: the multi-frame answer to the single-frame `cropHint`.
///
/// `stock_contact_sheet` says where the subject is in ONE frame; a vertical
/// edit is seconds long and the subject moves. This samples frames across
/// the clip, finds the subject in each (a weighted centroid of what stands
/// out from its own frame AND from the temporal median of the same tile
/// across every sampled frame, so a static background cancels), smooths
/// the path - holding the last known position through occlusion and
/// exits, entering from the centre when the subject arrives late - and
/// returns the keyframes at the path's turns, each with the exact
/// `clip_transform` offsets for the target aspect, ready for
/// `keyframe_set` on the clip's offsetX/offsetY. Read-only: it decodes
/// frames and answers; it changes nothing.
pub fn track_subject(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let path = fields
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| invalid("`path` is the media file to track the subject of"))?;
    let aspect = fields
        .get("aspect")
        .and_then(Value::as_str)
        .unwrap_or("9:16");
    if !["16:9", "9:16", "1:1", "4:5"].contains(&aspect) {
        return Err(invalid("aspect is 16:9, 9:16, 1:1, or 4:5"));
    }
    let mut spec = json!({
        "path": path,
        "samples": (number(&fields, "samples", 12.0).round() as i64).clamp(3, 24),
        "minConfidence": number(&fields, "minConfidence", 0.18),
        "autoSetup": fields.get("autoSetup").and_then(Value::as_bool).unwrap_or(true),
    });
    for key in ["from", "to"] {
        if let Some(at) = fields.get(key).and_then(Value::as_f64) {
            spec[key] = json!(at);
        }
    }
    if let Some(times) = fields.get("times").and_then(Value::as_array) {
        spec["times"] = json!(times);
    }
    // As `measure_loudness`: the binary is never the caller's to choose.
    let mut result = run("track", &spec, Duration::from_secs(240))?;
    // The keyframes carry the subject's position as a cropHint (-1..1);
    // each becomes the exact clip_transform offsets for `aspect`, the same
    // geometry the single-frame reframeHint uses, so the two agree.
    if let Some(keys) = result.get_mut("keyframes").and_then(Value::as_array_mut) {
        for key in keys.iter_mut() {
            let hint_x = key.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let hint_y = key.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            let (offset_x, offset_y) = reframe_offset(aspect, hint_x, hint_y);
            key["offsetX"] = json!(offset_x);
            key["offsetY"] = json!(offset_y);
        }
    }
    result["aspect"] = json!(aspect);
    result["hint"] = json!(
        "Keyframes are the crop path's turns, `at` seconds from the clip's start: keyframe_set each one on the clip's offsetX/offsetY (and the first at the clip's start) so the vertical crop follows the subject instead of centre-cropping it. `path` is the smoothed position every sample, `held` marking the holds; a `moved: false` subject needs no keys - the single reframeHint is enough."
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
    for key in ["plate", "width", "height", "background", "x", "y", "gpu"] {
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

/// `local_vlm`: status, first-use setup, or provider configuration of the
/// vision critic - local 0.5B by default, or a free-tier hosted API.
pub fn local_vlm(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let action = fields
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("status");
    if !["status", "setup", "configure", "providers", "clear"].contains(&action) {
        return Err(invalid(
            "action is `status`, `setup`, `configure`, `providers` or `clear`",
        ));
    }
    let profile = fields
        .get("profile")
        .and_then(Value::as_str)
        .unwrap_or("mini");
    if !["mini", "image", "minicpm"].contains(&profile) {
        return Err(invalid("profile is `mini`, `image` or `minicpm`"));
    }
    let mut spec = json!({ "action": action, "profile": profile });
    // `configure` is the only action that carries provider fields; the key
    // travels the existing stdin pipe (never argv), and the Python side
    // replies with a masked key only - the real one stays in vlm.json
    // inside the app's own data dir.
    if action == "configure" {
        for key in ["provider", "apiKey", "endpoint", "model"] {
            if let Some(value) = fields.get(key).and_then(Value::as_str) {
                spec[key] = json!(value);
            }
        }
    }
    let result = run(
        "vlm",
        &spec,
        Duration::from_secs(if action == "setup" { 900 } else { 30 }),
    )?;
    Ok(Outcome::json(result))
}

/// `emotion`: the emotional read of the voiceover - per line: the emotion,
/// its act, the easing curve it demands, colour temperature, duck depth and
/// stock vocabulary - plus the brand gate and the export verify.
pub fn emotion(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let action = fields
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("extract");
    if !["extract", "brand", "gate", "verify", "template", "selftest"].contains(&action) {
        return Err(invalid(
            "action is `extract`, `brand`, `gate`, `verify`, `template` or `selftest`",
        ));
    }
    let mut spec = json!({ "action": action });
    // `project` loads the brief the way every other creative tool does: the
    // brief's emotionalArc checks the lines, and its brandKit (when it is a
    // full brand guide) is applied as the brand.
    if let Some(project) = fields.get("project").and_then(Value::as_str) {
        let brief = load_brief(Some(project));
        let has_brief = brief.as_object().is_some_and(|map| !map.is_empty());
        if has_brief && !fields.contains_key("brief") {
            spec["brief"] = brief.clone();
        }
        let kit = brief.get("brandKit").filter(|kit| kit.is_object());
        if kit.is_some() && !fields.contains_key("brand") {
            spec["brand"] = kit.cloned().unwrap_or(Value::Null);
        }
    }
    // Only the fields the action reads travel on: the spec crosses a
    // process boundary, and a transcript with word timings is large enough
    // without carrying fields the command would ignore.
    let passthrough: &[&str] = match action {
        "extract" => &[
            "transcript",
            "lines",
            "script",
            "brief",
            "sections",
            "brand",
            "apiKey",
            "model",
        ],
        "brand" => &["brand", "lines"],
        "gate" => &["segment", "candidates", "brand"],
        "verify" => &["expected", "actual", "toleranceMs"],
        _ => &[],
    };
    for key in passthrough {
        if let Some(value) = fields.get(*key) {
            spec[*key] = value.clone();
        }
    }
    // `extract` is the only action that may wait on a hosted model: the
    // lines travel in batches of 64 with per-endpoint retries inside the
    // budget. A fixed 90 s covered a short script; a real one - hundreds of
    // lines against a slow free tier - was killed mid-batch with no partial
    // result and no fallback note. Scale the budget with the work, capped
    // under the harness's own 900 s call timeout.
    let budget = if action == "extract" {
        let scripted = fields
            .get("script")
            .and_then(Value::as_str)
            .map_or(0, |text| text.lines().count());
        let lined = fields
            .get("lines")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        // A transcript arrives two ways: a bare array, or the object the
        // transcribe job returns with its segments inside - both count
        // their segments, or a dict-shaped transcript of hundreds of
        // lines was budgeted like an empty one and killed mid-batch.
        let segments = match fields.get("transcript") {
            Some(Value::Array(list)) => list.len(),
            Some(Value::Object(map)) => map
                .get("segments")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            _ => 0,
        };
        90 + 8 * scripted.max(lined).max(segments) as u64
    } else {
        30
    };
    let result = run("emotion", &spec, Duration::from_secs(budget.min(480)))?;
    Ok(Outcome::json(result))
}

/// The voiceover read as a score: the `voice` tool's extract. One spec in
/// (audio + transcript), one reading out - per-line loudness, pause
/// classes, speech rate, word confidence, sentence types, motifs and
/// fillers from the Python side; prosody, forensics, speakers and
/// entities from the Rust `voice-signals` binary when it is built; the
/// emotion read's arc passed through when the caller gives it. The budget
/// scales with the lines and the audio length: the Rust ears analyse
/// every line and window, and a WhisperX alignment (when installed) is
/// real work on top.
pub fn voice_signals(arguments: Value) -> Result<Outcome, ApiError> {
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let action = fields
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("extract");
    if !["extract", "selftest"].contains(&action) {
        return Err(invalid("action is `extract` or `selftest`"));
    }
    let mut spec = json!({ "action": action });
    // Only the fields the action reads travel on: the spec crosses a
    // process boundary, and a transcript with word timings is large
    // enough already.
    for key in [
        "audio",
        "transcript",
        "lines",
        "script",
        "language",
        "emotion",
        "thresholdDb",
        "minSilenceMs",
        "maxSpeakers",
        "speakerThreshold",
        "whisperx",
    ] {
        if let Some(value) = fields.get(key) {
            spec[key] = value.clone();
        }
    }
    let budget = if action == "extract" {
        let scripted = fields
            .get("script")
            .and_then(Value::as_str)
            .map_or(0, |text| text.lines().count());
        let lined = fields
            .get("lines")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let segments = match fields.get("transcript") {
            Some(Value::Array(list)) => list.len(),
            Some(Value::Object(map)) => map
                .get("segments")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            _ => 0,
        };
        120 + 8 * scripted.max(lined).max(segments) as u64
    } else {
        60
    };
    let result = run("voice", &spec, Duration::from_secs(budget.min(480)))?;
    Ok(Outcome::json(result))
}

/// One tool served by the Python sense families: which command sense.py
/// dispatches on, and which argument fields travel into the spec. The
/// tool's own name rides along as `tool`, and the module inside the
/// sidecar does the rest - the same one-spec-one-JSON contract every
/// other sense script keeps.
struct SenseFamilyTool {
    /// The tool an agent names.
    tool: &'static str,
    /// The cmd sense.py takes: one per family module.
    cmd: &'static str,
    /// The argument names forwarded into the spec verbatim.
    fields: &'static [&'static str],
    /// The budget's floor: the families that run ffmpeg passes or write
    /// files need more room than the pure-text readers.
    base: u64,
}

/// The thirty-three consumption-layer tools, five families: discourse
/// (the script as language), voiceq (the take's health), prosody (the
/// delivery), audiorepair (the clean and the music) and captions (the
/// words made to agree with the take).
const SENSE_FAMILY_TOOLS: &[SenseFamilyTool] = &[
    // discourse.py - the script read as language
    SenseFamilyTool {
        tool: "analyze_discourse",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "segment_topics",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script", "maxSegments"],
    },
    SenseFamilyTool {
        tool: "detect_speech_acts",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "classify_speech_acts",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "score_readability",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "extract_claim_graph",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "score_script_quality",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script", "targetSeconds"],
    },
    SenseFamilyTool {
        tool: "detect_sarcasm",
        cmd: "discourse",
        base: 150,
        fields: &["audio", "transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "detect_code_switching",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script", "language"],
    },
    SenseFamilyTool {
        tool: "detect_cross_lingual_emotion",
        cmd: "discourse",
        base: 90,
        fields: &["transcript", "lines", "script", "language"],
    },
    // voice_quality.py - the voice itself, measured
    SenseFamilyTool {
        tool: "detect_vocal_events",
        cmd: "voiceq",
        base: 150,
        fields: &["audio", "transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "classify_voice_quality",
        cmd: "voiceq",
        base: 150,
        fields: &["audio"],
    },
    SenseFamilyTool {
        tool: "classify_recording_quality",
        cmd: "voiceq",
        base: 150,
        fields: &["audio"],
    },
    SenseFamilyTool {
        tool: "measure_reverb",
        cmd: "voiceq",
        base: 150,
        fields: &["audio"],
    },
    SenseFamilyTool {
        tool: "detect_restarts",
        cmd: "voiceq",
        base: 90,
        fields: &["audio", "transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "classify_filler_type",
        cmd: "voiceq",
        base: 90,
        fields: &["audio", "transcript", "lines", "script"],
    },
    // prosody_style.py - how the voice moves
    SenseFamilyTool {
        tool: "score_emphasis_type",
        cmd: "prosody",
        base: 150,
        fields: &["audio", "transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "detect_speaking_style",
        cmd: "prosody",
        base: 150,
        fields: &["audio", "transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "measure_rhythm_meter",
        cmd: "prosody",
        base: 150,
        fields: &["audio", "transcript", "lines", "script", "bpm"],
    },
    SenseFamilyTool {
        tool: "detect_tempo_changes",
        cmd: "prosody",
        base: 150,
        fields: &["audio", "transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "detect_accent",
        cmd: "prosody",
        base: 150,
        fields: &["audio", "transcript", "lines", "script"],
    },
    SenseFamilyTool {
        tool: "measure_vocal_tract",
        cmd: "prosody",
        base: 150,
        fields: &["audio", "transcript", "lines", "script"],
    },
    // audio_repair.py - the clean and the music
    SenseFamilyTool {
        tool: "enhance_speech",
        cmd: "audiorepair",
        base: 240,
        fields: &[
            "audio",
            "out",
            "overwrite",
            "targetLufs",
            "targetTruePeakDbtp",
        ],
    },
    SenseFamilyTool {
        tool: "remove_hiss_hum_rumble",
        cmd: "audiorepair",
        base: 240,
        fields: &["audio", "out", "overwrite", "force"],
    },
    SenseFamilyTool {
        tool: "normalize_loudness",
        cmd: "audiorepair",
        base: 240,
        fields: &[
            "audio",
            "out",
            "overwrite",
            "platform",
            "targetLufs",
            "targetTruePeakDbtp",
        ],
    },
    SenseFamilyTool {
        tool: "detect_music_segments",
        cmd: "audiorepair",
        base: 180,
        fields: &["audio"],
    },
    SenseFamilyTool {
        tool: "identify_music",
        cmd: "audiorepair",
        base: 180,
        fields: &["audio"],
    },
    SenseFamilyTool {
        tool: "analyze_music_structure",
        cmd: "audiorepair",
        base: 180,
        fields: &["audio"],
    },
    // captions.py - the words and the take, made to agree
    SenseFamilyTool {
        tool: "evaluate_tts_quality",
        cmd: "captions",
        base: 150,
        fields: &["audio", "transcript", "lines"],
    },
    SenseFamilyTool {
        tool: "detect_ai_voice",
        cmd: "captions",
        base: 150,
        fields: &["audio", "transcript", "lines"],
    },
    SenseFamilyTool {
        tool: "seal_voice",
        cmd: "captions",
        base: 150,
        fields: &["audio", "out", "overwrite", "policy"],
    },
    SenseFamilyTool {
        tool: "align_captions",
        cmd: "captions",
        base: 150,
        fields: &[
            "audio",
            "transcript",
            "lines",
            "script",
            "maxCharsPerSecond",
            "maxDurationSeconds",
            "minDurationSeconds",
            "maxCharsPerLine",
        ],
    },
    SenseFamilyTool {
        tool: "clean_subtitles",
        cmd: "captions",
        base: 90,
        fields: &[
            "srt",
            "transcript",
            "lines",
            "script",
            "stripFillers",
            "maxCharsPerLine",
            "maxCharsPerSecond",
            "format",
        ],
    },
];

/// The five family doors and the members each one serves: the catalogue
/// names the family, the `action` argument names the member, and this
/// table is the boundary between them - a member named through the wrong
/// family is refused here, with the family that carries it named.
const SENSE_FAMILIES: &[(&str, &[&str])] = &[
    (
        "discourse",
        &[
            "analyze_discourse",
            "segment_topics",
            "detect_speech_acts",
            "classify_speech_acts",
            "score_readability",
            "extract_claim_graph",
            "score_script_quality",
            "detect_sarcasm",
            "detect_code_switching",
            "detect_cross_lingual_emotion",
        ],
    ),
    (
        "voiceq",
        &[
            "detect_vocal_events",
            "classify_voice_quality",
            "classify_recording_quality",
            "measure_reverb",
            "detect_restarts",
            "classify_filler_type",
        ],
    ),
    (
        "prosody",
        &[
            "score_emphasis_type",
            "detect_speaking_style",
            "measure_rhythm_meter",
            "detect_tempo_changes",
            "detect_accent",
            "measure_vocal_tract",
        ],
    ),
    (
        "audiorepair",
        &[
            "enhance_speech",
            "remove_hiss_hum_rumble",
            "normalize_loudness",
            "detect_music_segments",
            "identify_music",
            "analyze_music_structure",
        ],
    ),
    (
        "captions",
        &[
            "evaluate_tts_quality",
            "detect_ai_voice",
            "seal_voice",
            "align_captions",
            "clean_subtitles",
        ],
    ),
];

/// One family tool's input schema: the `action` enum is the family's
/// members, `selftest` runs the member's own proof, and the fields are
/// the union of what the family's members take - each named only where a
/// member reads it, so the schema stays one screen tall.
pub(crate) fn sense_family_schema(name: &str) -> Value {
    let Some((_, members)) = SENSE_FAMILIES
        .iter()
        .find(|(family, _)| *family == family_key(name))
    else {
        return json!({ "type": "object" });
    };
    let string = |description: &str| json!({ "type": "string", "description": description });
    let transcript = || json!({ "type": "object", "description": "A transcribe job's output: {segments: [{text, start, end, words: [{text,start,end}]}]} - the word timings the timing reads need." });
    let lines = || json!({ "type": "array", "description": "Lines as [{text, start, end, words}] (or bare strings) when there is no transcript.", "items": { "type": ["object", "string"] } });
    let script = || json!({ "type": "string", "description": "Raw narration text when there is no transcript or audio; the text-only signals still answer." });
    let mut props = Map::new();
    props.insert(
        "action".to_owned(),
        json!({
            "type": "string",
            "enum": members,
            "description": "The reading to run, one of this family's.",
        }),
    );
    props.insert(
        "selftest".to_owned(),
        json!({
            "type": "boolean",
            "default": false,
            "description": "Run the reading's built-in proof instead of an extract.",
        }),
    );
    let takes_text = matches!(
        name,
        "sense_discourse" | "sense_voice_quality" | "sense_prosody_style"
    );
    let takes_audio = name != "sense_discourse";
    if takes_audio {
        props.insert(
            "audio".to_owned(),
            string(
                "The voiceover file (any format ffmpeg reads) the reading needs; never modified.",
            ),
        );
    }
    if takes_text {
        props.insert("transcript".to_owned(), transcript());
        props.insert("lines".to_owned(), lines());
        props.insert("script".to_owned(), script());
    }
    let member = |name: &str| members.contains(&name);
    if member("segment_topics") {
        props.insert(
            "maxSegments".to_owned(),
            json!({ "type": "integer", "minimum": 2, "maximum": 24, "default": 12, "description": "segment_topics: the most topic boundaries to cut at." }),
        );
    }
    if member("score_script_quality") {
        props.insert(
            "targetSeconds".to_owned(),
            json!({ "type": "number", "minimum": 3, "description": "score_script_quality: the duration the script must fit; the 150-wpm estimate names the word count to cut." }),
        );
    }
    if member("detect_code_switching") || member("detect_cross_lingual_emotion") {
        props.insert(
            "language".to_owned(),
            string("detect_code_switching / detect_cross_lingual_emotion: the language hint the transcribe call used (ISO code)."),
        );
    }
    if member("measure_rhythm_meter") {
        props.insert(
            "bpm".to_owned(),
            json!({ "type": "number", "minimum": 30, "maximum": 300, "description": "measure_rhythm_meter: a BPM hint - which syllable nuclei land on the beat grid." }),
        );
    }
    if member("enhance_speech") || member("normalize_loudness") {
        props.insert(
            "out".to_owned(),
            string("enhance_speech / remove_hiss_hum_rumble / normalize_loudness: the output file; defaults beside the input."),
        );
        props.insert(
            "overwrite".to_owned(),
            json!({ "type": "boolean", "default": false, "description": "Replace an existing output file." }),
        );
    }
    if member("enhance_speech") {
        props.insert(
            "targetLufs".to_owned(),
            json!({ "type": "number", "default": -16, "minimum": -40, "maximum": 0, "description": "enhance_speech: the loudness the chain lands on." }),
        );
        props.insert(
            "targetTruePeakDbtp".to_owned(),
            json!({ "type": "number", "default": -1.5, "minimum": -9, "maximum": 0, "description": "enhance_speech: the true-peak ceiling." }),
        );
    }
    if member("remove_hiss_hum_rumble") {
        props.insert(
            "force".to_owned(),
            json!({ "type": "boolean", "default": false, "description": "remove_hiss_hum_rumble: clean even when the take is cleaner than the cure." }),
        );
    }
    if member("normalize_loudness") {
        props.insert(
            "platform".to_owned(),
            json!({ "type": "string", "enum": ["youtube", "spotify", "reels-tiktok", "streaming", "podcast", "apple-podcast", "broadcast-eu", "broadcast-atsc", "quiet"], "description": "normalize_loudness: the platform whose loudness target to land on (-14 streaming, -16 podcast, -23 EBU, -24 ATSC)." }),
        );
        props.insert(
            "targetLufs".to_owned(),
            json!({ "type": "number", "default": -14, "minimum": -40, "maximum": 0, "description": "normalize_loudness: the loudness to land on; the platform default when absent." }),
        );
        props.insert(
            "targetTruePeakDbtp".to_owned(),
            json!({ "type": "number", "default": -1, "minimum": -9, "maximum": 0, "description": "normalize_loudness: the true-peak ceiling." }),
        );
    }
    if member("seal_voice") {
        props.insert(
            "policy".to_owned(),
            json!({ "type": "object", "description": "seal_voice: the usage record - {allowedUses: [string], expiry: ISO date, attribution, license, syntheticVoiceDisclosure}.", "properties": {
                "allowedUses": { "type": "array", "items": { "type": "string" } },
                "expiry": { "type": "string" },
                "attribution": { "type": "string" },
                "license": { "type": "string" },
                "syntheticVoiceDisclosure": { "type": ["string", "boolean"] },
            }, "additionalProperties": false }),
        );
    }
    if member("evaluate_tts_quality") || member("align_captions") {
        // The captions family's text inputs ride on the members that read
        // them: transcript/lines for the TTS and alignment reads, script
        // for alignment and subtitle hygiene.
        props.insert("transcript".to_owned(), transcript());
        props.insert("lines".to_owned(), lines());
    }
    if member("align_captions") || member("clean_subtitles") {
        props.insert("script".to_owned(), script());
    }
    if member("align_captions") {
        props.insert(
            "maxCharsPerSecond".to_owned(),
            json!({ "type": "number", "default": 17, "minimum": 5, "maximum": 40, "description": "align_captions: the caption pace ceiling." }),
        );
        props.insert(
            "maxDurationSeconds".to_owned(),
            json!({ "type": "number", "default": 5, "minimum": 1, "maximum": 15, "description": "align_captions: the longest a cue may hold." }),
        );
        props.insert(
            "minDurationSeconds".to_owned(),
            json!({ "type": "number", "default": 0.9, "minimum": 0.3, "maximum": 3, "description": "align_captions: the shortest a cue may flash." }),
        );
        props.insert(
            "maxCharsPerLine".to_owned(),
            json!({ "type": "integer", "default": 42, "minimum": 16, "maximum": 84, "description": "align_captions / clean_subtitles: the line length before wrapping." }),
        );
    }
    if member("clean_subtitles") {
        props.insert(
            "srt".to_owned(),
            string("clean_subtitles: raw SRT text to parse and clean."),
        );
        props.insert(
            "stripFillers".to_owned(),
            json!({ "type": "boolean", "default": true, "description": "clean_subtitles: take the um/uh out of the cues." }),
        );
        props.insert(
            "format".to_owned(),
            json!({ "type": "string", "enum": ["cues", "srt"], "default": "cues", "description": "clean_subtitles: the shape of the cleaned reply." }),
        );
    }
    json!({
        "type": "object",
        "properties": props,
        "required": ["action"],
        "additionalProperties": false,
    })
}

/// The catalogue's family tool name to the family key the sidecar's cmd
/// uses: sense_audio_repair -> audiorepair, sense_voice_quality -> voiceq.
fn family_key(tool: &str) -> &str {
    match tool {
        "sense_discourse" => "discourse",
        "sense_voice_quality" => "voiceq",
        "sense_prosody_style" => "prosody",
        "sense_audio_repair" => "audiorepair",
        _ => "captions",
    }
}

/// The door the thirty-three consumption-layer readings come through
/// from their five family tools: `action` names the member, the member's
/// row names the sidecar command and the fields that travel, and the
/// budget scales with the lines the way `voice_signals`'s does - a
/// transcript-heavy call is real work, a script-only call is not.
pub fn sense_family_action(family: &str, arguments: Value) -> Result<Outcome, ApiError> {
    let members = SENSE_FAMILIES
        .iter()
        .find(|(key, _)| *key == family)
        .map(|(_, members)| *members)
        .ok_or_else(|| invalid(&format!("unknown sense family `{family}`")))?;
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    // `action` is the member: the family tool's whole shape. A missing or
    // unknown one is refused with the family's real members named - the
    // agent reads one refusal instead of a Python traceback.
    let member = fields
        .get("action")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(&format!("name `action`: one of {}", members.join(", "))))?;
    if !members.contains(&member.as_str()) {
        return Err(invalid(&format!(
            "`{member}` is not one of this family's actions: {}",
            members.join(", ")
        )));
    }
    let selftest = fields.get("selftest").and_then(Value::as_bool) == Some(true);
    // The selftest boolean becomes the member's own action; without it
    // the sidecar's contract defaults to "extract".
    let mut fields = fields;
    fields.remove("action");
    fields.remove("selftest");
    if selftest {
        fields.insert("action".to_owned(), json!("selftest"));
    }
    sense_family(&member, Value::Object(fields)).map(|mut outcome| {
        // The reply names the member that answered, so a family call's
        // result reads the same as the reading it ran.
        if let Some(structured) = outcome.structured.as_mut() {
            structured["action"] = json!(member);
        }
        outcome
    })
}

/// The door the thirty-three family members come through when named
/// directly (the internal dispatch `sense_family_action` lands on): the
/// name selects the row, the row names the sidecar command and the
/// fields that travel, and the budget scales with the lines the way
/// `voice_signals`'s does.
pub fn sense_family(name: &str, arguments: Value) -> Result<Outcome, ApiError> {
    let row = SENSE_FAMILY_TOOLS
        .iter()
        .find(|row| row.tool == name)
        .ok_or_else(|| invalid(&format!("unknown sense tool `{name}`")))?;
    let fields = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let mut spec = json!({ "tool": name });
    if let Some(action) = fields.get("action") {
        spec["action"] = action.clone();
    }
    for key in row.fields {
        if let Some(value) = fields.get(*key) {
            spec[*key] = value.clone();
        }
    }
    // The budget: the family's floor plus the per-line work a transcript
    // carries, capped where the outer table takes over.
    let transcript_lines = fields.get("transcript").map_or(0, |v| match v {
        Value::Array(list) => list.len(),
        Value::Object(map) => map
            .get("segments")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        _ => 0,
    });
    let lined = fields
        .get("lines")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let scripted = fields
        .get("script")
        .and_then(Value::as_str)
        .map_or(0, |text| text.lines().count());
    let lines = transcript_lines.max(lined).max(scripted);
    let budget = row.base + 8 * lines as u64;
    let result = run(row.cmd, &spec, Duration::from_secs(budget.min(480)))?;
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

    /// A backend that answers `openvid.status` with the rerank model
    /// missing, and refuses every fetch: the availability gate must keep
    /// a single byte from moving when the second pass cannot run.
    struct NoRerankModel {
        fetches: usize,
    }

    impl Backend for NoRerankModel {
        fn call(&mut self, request: Request) -> Result<Value, ApiError> {
            match request {
                Request::OpenvidStatus => Ok(json!({
                    "state": "ready",
                    "percent": 100,
                    "rows": 937_957,
                    "measured": 0,
                    "rerank": { "available": false, "dir": "/models/mobileclip2-s0", "files": [] }
                })),
                Request::OpenvidFetch { .. } => {
                    self.fetches += 1;
                    Err(ApiError::failed(
                        "a fetch must not happen when the rerank model is missing",
                    ))
                }
                other => Err(ApiError::failed(format!("unexpected {other:?}"))),
            }
        }
        fn describe(&self) -> Value {
            json!({ "mode": "test" })
        }
    }

    #[test]
    fn a_missing_rerank_model_is_found_before_any_fetch() {
        // A relevance page long enough to reach the rerank (the tiny-page
        // early-out keeps ten or fewer untouched), against a backend whose
        // model is absent: the gate must answer before the fetch loop.
        let arguments = json!({ "query": "a face lit by cold screen glow", "sort": "relevance" });
        let clips: Vec<Value> = (0..12)
            .map(|id| json!({ "id": id, "caption": format!("clip {id}"), "seconds": 3.0 }))
            .collect();
        let found = json!({ "total": 12, "clips": clips });
        let mut backend = NoRerankModel { fetches: 0 };
        let ranked = super::semantic_rerank(&arguments, found.clone(), &mut backend);
        assert_eq!(
            backend.fetches, 0,
            "no candidate video is fetched when the model is missing"
        );
        assert_eq!(ranked["rerank"]["applied"], false);
        assert!(
            ranked["rerank"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("not installed"),
            "the reason names the missing model: {ranked}"
        );
        // The BM25 page is the answer, unchanged.
        assert_eq!(ranked["clips"], found["clips"]);
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
        assert_eq!(reframe_offset("16:9", 0.0, 99.0).1, -0.39);
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

    /// The five family doors: `action` names a member, a member named
    /// through the wrong family is refused with both families' real
    /// members, and the schemas carry every member in their enum. None
    /// of this runs Python - the refusals answer before the sidecar is
    /// ever asked.
    #[test]
    fn the_family_doors_name_their_members_and_refuse_the_wrong_family() {
        // Every family's schema carries exactly its members as the
        // action enum, and `action` is required.
        let doors = [
            ("sense_discourse", "discourse"),
            ("sense_voice_quality", "voiceq"),
            ("sense_prosody_style", "prosody"),
            ("sense_audio_repair", "audiorepair"),
            ("sense_captions", "captions"),
        ];
        let mut members = 0;
        for (tool, family) in doors {
            let schema = sense_family_schema(tool);
            assert_eq!(schema["type"], "object", "{tool}");
            assert_eq!(
                schema["required"],
                json!(["action"]),
                "{tool}: action is required"
            );
            let choices = schema["properties"]["action"]["enum"]
                .as_array()
                .unwrap_or_else(|| panic!("{tool}: the action enum"));
            let (_, expected) = SENSE_FAMILIES
                .iter()
                .find(|(key, _)| *key == family)
                .expect("the family");
            assert_eq!(
                choices.len(),
                expected.len(),
                "{tool}: the enum and the family table agree"
            );
            for name in *expected {
                assert!(
                    choices.contains(&json!(name)),
                    "{tool}: {name} missing from the enum"
                );
            }
            members += expected.len();
        }
        assert_eq!(members, 33, "thirty-three readings behind five doors");
        // A member named through the wrong family is refused by name,
        // with the family's real members listed - no Python involved.
        let wrong = match sense_family_action(
            "discourse",
            json!({ "action": "enhance_speech", "audio": "/tmp/x.wav" }),
        ) {
            Err(wrong) => wrong,
            Ok(_) => panic!("enhance_speech is not a discourse reading"),
        };
        assert!(
            wrong.to_string().contains("enhance_speech"),
            "the refusal names the member: {wrong}"
        );
        assert!(
            wrong.to_string().contains("analyze_discourse"),
            "the refusal names the family's members: {wrong}"
        );
        // A missing action is refused the same way.
        let missing = match sense_family_action("voiceq", json!({ "audio": "/tmp/x.wav" })) {
            Err(missing) => missing,
            Ok(_) => panic!("action is required"),
        };
        assert!(
            missing.to_string().contains("detect_vocal_events"),
            "the refusal names the members: {missing}"
        );
        // An unknown family is refused too.
        assert!(sense_family_action("nope", json!({ "action": "x" })).is_err());
    }

    /// The Rust ears' pin: a `voice-signals` beside the executable is
    /// found, and nothing else is - the bundle stages it exactly there,
    /// beside the Geneva sidecar, and the bridge honours the pin first.
    #[test]
    fn the_rust_ears_are_pinned_beside_the_executable() {
        let dir = std::env::temp_dir().join(format!(
            "concat-ears-pin-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("the folder is made");
        let exe = dir.join("concat");
        std::fs::write(&exe, b"binary").expect("the executable is written");
        assert!(
            super::voice_signals_beside(&exe).is_none(),
            "no ears beside an executable that has none"
        );
        std::fs::write(dir.join("voice-signals"), b"binary").expect("the ears are written");
        let found = super::voice_signals_beside(&exe).expect("the ears are found");
        assert_eq!(found, dir.join("voice-signals"));
        // A pathless executable is not a crash.
        assert!(super::voice_signals_beside(Path::new("concat")).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
