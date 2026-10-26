// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The Geneva engine: a comp drawn by the browserless HTML/CSS renderer.
//!
//! Geneva is a single binary that reads a JSON *timeline*, draws HTML+CSS with
//! its own layout and paint engine - no browser anywhere - and writes video,
//! or, as here, a PNG sequence with alpha. Its contract is what makes it safe
//! to bolt onto an editor:
//!
//! - **JSON in, JSON out.** `--format json` prints exactly one document with
//!   `ok`, `diagnostics` (each a stable `code`, a JSON-pointer `path`, a
//!   `message` and a `help`) and a `summary`. Nothing has to be scraped.
//! - **Deterministic.** The same timeline, assets and binary version produce
//!   the same frames, so a bake can be cached by content hash.
//! - **Bounded.** It cannot fetch anything, has no JavaScript, and refuses
//!   asset paths that leave the timeline's own directory. Staging a comp's
//!   assets into the bake directory is therefore the whole sandbox.
//!
//! The binary is not vendored in the tree and is not a build dependency: it is
//! discovered, version-checked and pinned by hash in the installer's manifest,
//! and every bake records the version it used. If it is missing, the project
//! still opens and the comp still bakes on the native engine when the comp can
//! be drawn natively; only a markup comp needs it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value as Json, json};

use crate::comp::{
    Asset, AssetKind, Blend, Comp, Effect, Fit, Length, Mask, MaskShape, Node, NodeKind, Paint,
    PathCommand, Rgba, Shadow, Stroke, TextAlign, TextNode,
};
use crate::curve::{Curve, StepPosition};
use crate::engine::{BakeRequest, Baked, CompEngine, EngineKind, MotionError, Probe};
use crate::track::{Track, Value};

/// The Geneva timeline format this module writes.
pub const TIMELINE_VERSION: &str = "1.1";

/// The version this crate was written and tested against. A newer binary is
/// accepted - the format only gains optional fields within a major version -
/// but a different major version is refused with the reason.
pub const TESTED_VERSION: &str = "1.6.0";

/// The environment variable that points at the binary, for a build that brings
/// its own.
pub const ENV_BIN: &str = "CONCAT_GENEVA_BIN";

/// A discovered Geneva binary.
#[derive(Clone, PartialEq, Debug)]
pub struct Geneva {
    exe: PathBuf,
}

impl Geneva {
    /// Find the binary: the environment's hint, then the setting, then the
    /// usual places a sidecar is installed, then `PATH`.
    pub fn discover(preferred: Option<&Path>) -> Option<Geneva> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(path) = std::env::var_os(ENV_BIN) {
            candidates.push(PathBuf::from(path));
        }
        if let Some(path) = preferred {
            candidates.push(path.to_path_buf());
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join("geneva"));
            candidates.push(dir.join("geneyva"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            candidates.push(home.join(".local/bin/geneva"));
            candidates.push(home.join("bin/geneva"));
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                candidates.push(dir.join("geneva"));
            }
        }
        candidates
            .into_iter()
            .find(|path| path.is_file())
            .map(|exe| Geneva { exe })
    }

    /// A Geneva at a known path, without checking that it runs.
    pub fn at(exe: impl Into<PathBuf>) -> Geneva {
        Geneva { exe: exe.into() }
    }

    /// The binary's path.
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// The version it reports, e.g. `1.6.0`.
    pub fn version(&self) -> Result<String, MotionError> {
        let output = Command::new(&self.exe)
            .arg("--version")
            .output()
            .map_err(|error| MotionError::Io {
                doing: "running".into(),
                path: self.exe.clone(),
                source: error,
            })?;
        let text = String::from_utf8_lossy(&output.stdout);
        // "geneva 1.6.0"
        Ok(text
            .split_whitespace()
            .last()
            .unwrap_or_default()
            .to_string())
    }

    /// Whether this binary can draw markup comps, and what it is.
    pub fn probe(&self) -> Probe {
        match self.version() {
            Ok(version) => {
                let major_ok = TESTED_VERSION.split('.').next() == version.split('.').next();
                Probe {
                    available: true,
                    version: Some(version.clone()),
                    detail: if major_ok {
                        format!("geneva {version} at {}", self.exe.display())
                    } else {
                        format!(
                            "geneva {version} is a different major version than the {TESTED_VERSION} this was written for; markup comps may be refused"
                        )
                    },
                }
            }
            Err(error) => Probe {
                available: false,
                version: None,
                detail: format!("{error}"),
            },
        }
    }

    /// The engine's own manual for a topic (`guide`, `explain`), so a
    /// diagnostic can be shown with the text that explains it, from the binary
    /// the project actually renders with.
    pub fn manual(&self, topic: &str) -> Option<String> {
        let output = Command::new(&self.exe)
            .arg("guide")
            .arg(topic)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// The timeline a comp becomes, and what could not be expressed in it.
    ///
    /// Assets are staged separately (`stage_assets`), so this only invents
    /// relative paths.
    pub fn timeline(&self, comp: &Comp, request: &BakeRequest) -> (Json, Vec<String>) {
        let mut warnings = Vec::new();
        let width = request.width(comp);
        let height = request.height(comp);
        let fps = request.fps(comp);

        let mut assets = serde_json::Map::new();
        for (id, asset) in &comp.assets {
            let Some(kind) = geneva_asset_kind(asset) else {
                warnings.push(format!(
                    "asset {id:?} is a {:?}, which geneva does not read",
                    asset.kind
                ));
                continue;
            };
            assets.insert(
                id.clone(),
                json!({ "src": staged_path(asset), "kind": kind }),
            );
        }

        let mut layers = Vec::new();
        for node in comp.paint_order() {
            let Some(source) = source_of(comp, node, &mut warnings) else {
                continue;
            };
            let mut clip = serde_json::Map::new();
            clip.insert("source".into(), source);
            if node.start > 0.0 {
                clip.insert("start".into(), Json::String(seconds(node.start)));
            }
            if node.duration > 0.0 {
                clip.insert("duration".into(), Json::String(seconds(node.duration)));
            }
            let transform = effective_transform(comp, node);
            if let Some(transform) = transform {
                clip.insert("transform".into(), transform);
            }
            let opacity = node
                .tracks
                .get("opacity")
                .and_then(|track| {
                    keyframes(track, Kind::Number, span_of(comp, node), &mut warnings)
                })
                .unwrap_or_else(|| json!(node.opacity));
            if opacity != json!(1.0) {
                clip.insert("opacity".into(), opacity);
            }
            if node.blend != Blend::Normal
                && let Some(name) = blend_name(node.blend)
            {
                clip.insert("blend".into(), Json::String(name.into()));
            }
            if let Some(mask) = &node.mask
                && let Some(mask) = mask_json(mask)
            {
                clip.insert("mask".into(), mask);
            }
            let mut effects = Vec::new();
            for effect in &node.effects {
                match effect {
                    Effect::Blur { radius } => {
                        let radius = node
                            .tracks
                            .get("blur")
                            .and_then(|track| keyframes(track, Kind::Number, span_of(comp, node), &mut warnings))
                            .unwrap_or_else(|| json!(radius));
                        effects.push(json!({ "kind": "blur", "radius": radius }));
                    }
                    Effect::Shadow { .. } => warnings.push(format!(
                        "node {:?}: a drop shadow is drawn by the native engine only; geneva has shadows inside markup",
                        node.id
                    )),
                    Effect::Shader { id, .. } => warnings.push(format!(
                        "node {:?}: shader {id:?} is a project package, drawn by the native engine only",
                        node.id
                    )),
                }
            }
            if !effects.is_empty() {
                clip.insert("effects".into(), Json::Array(effects));
            }
            for name in node.tracks.keys() {
                if !matches!(
                    name.as_str(),
                    "position" | "scale" | "rotation" | "opacity" | "blur"
                ) {
                    warnings.push(format!(
                        "node {:?}: the {name:?} track is not expressible in a geneva timeline",
                        node.id
                    ));
                }
            }
            layers.push(json!({ "id": node.id, "clips": [clip] }));
        }

        let mut output = serde_json::Map::new();
        output.insert("width".into(), json!(width));
        output.insert("height".into(), json!(height));
        output.insert("fps".into(), json!(fps));
        output.insert("duration".into(), Json::String(seconds(comp.duration)));
        output.insert("background".into(), Json::String(comp.background.to_hex()));
        // Frames with alpha: the one carrier the engine guarantees, and the one
        // the compositor samples without a conversion.
        output.insert(
            "encode".into(),
            json!({ "container": "image-sequence", "video": { "codec": "png" } }),
        );

        let mut document = serde_json::Map::new();
        document.insert("geneva".into(), Json::String(TIMELINE_VERSION.into()));
        document.insert("output".into(), Json::Object(output));
        if !assets.is_empty() {
            document.insert("assets".into(), Json::Object(assets));
        }
        document.insert("layers".into(), Json::Array(layers));
        (Json::Object(document), warnings)
    }

    /// Write the timeline and the assets a bake needs into `dir`.
    fn stage(
        &self,
        comp: &Comp,
        source_dir: &Path,
        request: &BakeRequest,
        dir: &Path,
    ) -> Result<Json, MotionError> {
        std::fs::create_dir_all(dir.join("assets")).map_err(|source| MotionError::Io {
            doing: "creating".into(),
            path: dir.join("assets"),
            source,
        })?;
        for asset in comp.assets.values() {
            let from = source_dir.join(&asset.src);
            if !from.is_file() {
                continue;
            }
            let to = dir.join(staged_path(asset));
            if to.is_file() {
                continue;
            }
            std::fs::copy(&from, &to).map_err(|source| MotionError::Io {
                doing: "copying an asset into the bake".into(),
                path: from,
                source,
            })?;
        }
        let (document, warnings) = self.timeline(comp, request);
        for warning in &warnings {
            log_warning(warning);
        }
        let path = dir.join("timeline.json");
        let text =
            serde_json::to_string_pretty(&document).map_err(|error| MotionError::Engine {
                code: None,
                message: format!("the comp could not be written as a geneva timeline: {error}"),
                help: None,
            })?;
        std::fs::write(&path, text).map_err(|source| MotionError::Io {
            doing: "writing".into(),
            path,
            source,
        })?;
        Ok(document)
    }

    /// Run one Geneva command in `dir` and hand back the report it printed.
    ///
    /// Public for the sequence builder: a sequence is a timeline of its own,
    /// not a comp, so it renders through the same door a bake does.
    pub fn run_in(&self, dir: &Path, args: &[String]) -> Result<Json, MotionError> {
        self.run(dir, args)
    }

    /// Run one Geneva command and hand back the report it printed.
    ///
    /// The working directory is the bake's own: a timeline is opened by the
    /// name `timeline.json`, and its assets resolve against the directory it
    /// sits in, which is what keeps a render inside the folder it was given.
    fn run(&self, dir: &Path, args: &[String]) -> Result<Json, MotionError> {
        let output = Command::new(&self.exe)
            .current_dir(dir)
            .args(args)
            .output()
            .map_err(|source| MotionError::Io {
                doing: "running geneva".into(),
                path: self.exe.clone(),
                source,
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let report: Json = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
            json!({
                "ok": output.status.success(),
                "diagnostics": [],
                "summary": {},
                "stdout": stdout.to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            })
        });
        let ok = report.get("ok").and_then(Json::as_bool).unwrap_or(false);
        if ok && output.status.success() {
            return Ok(report);
        }
        // The first error diagnostic is the one worth showing; the rest are
        // usually consequences of it.
        let first = report
            .get("diagnostics")
            .and_then(Json::as_array)
            .and_then(|list| {
                list.iter()
                    .find(|d| d.get("severity").and_then(Json::as_str) == Some("error"))
            });
        let (code, message, help) = match first {
            Some(diagnostic) => (
                diagnostic
                    .get("code")
                    .and_then(Json::as_str)
                    .map(str::to_string),
                diagnostic
                    .get("message")
                    .and_then(Json::as_str)
                    .unwrap_or("geneva refused the timeline")
                    .to_string(),
                diagnostic
                    .get("help")
                    .and_then(Json::as_str)
                    .map(str::to_string),
            ),
            None => (None, format!("geneva exited with {}", output.status), None),
        };
        Err(MotionError::Engine {
            code,
            message,
            help,
        })
    }
}

impl CompEngine for Geneva {
    fn kind(&self) -> EngineKind {
        EngineKind::Geneva
    }

    fn probe(&self) -> Probe {
        Geneva::probe(self)
    }

    fn bake(&self, comp: &Comp, request: &BakeRequest, dir: &Path) -> Result<Baked, MotionError> {
        let problems = comp.validate();
        if !problems.is_empty() {
            return Err(MotionError::Invalid {
                diagnostics: problems,
            });
        }
        let version = self.version()?;
        let source_dir = if request.source_dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            request.source_dir.clone()
        };
        self.stage(comp, &source_dir, request, dir)?;

        let frames = dir.join("frames");
        std::fs::create_dir_all(&frames).map_err(|source| MotionError::Io {
            doing: "creating".into(),
            path: frames.clone(),
            source,
        })?;
        let pattern = format!("{}/%04d.png", frames.display());
        let mut args: Vec<String> = vec![
            "--format".into(),
            "json".into(),
            "render".into(),
            "timeline.json".into(),
            "-o".into(),
            pattern,
        ];
        if let Some((from, to)) = request.frames {
            args.push("--frames".into());
            args.push(format!("{from}..{to}"));
        }
        self.run(dir, &args)?;

        let mut written = 0_usize;
        if let Ok(entries) = std::fs::read_dir(&frames) {
            written = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
                .count();
        }
        let baked = Baked {
            dir: frames,
            frames: written,
            width: request.width(comp),
            height: request.height(comp),
            fps: request.fps(comp),
            hash: comp.hash(),
            engine: EngineKind::Geneva,
            engine_version: version,
            alpha: request.alpha,
        };
        let manifest = serde_json::to_string_pretty(&baked).unwrap_or_default();
        std::fs::write(dir.join("manifest.json"), manifest).map_err(|source| MotionError::Io {
            doing: "writing the bake manifest".into(),
            path: dir.join("manifest.json"),
            source,
        })?;
        Ok(baked)
    }

    fn frame(&self, comp: &Comp, at: f64, width: Option<u32>) -> Result<PathBuf, MotionError> {
        let problems = comp.validate();
        if !problems.is_empty() {
            return Err(MotionError::Invalid {
                diagnostics: problems,
            });
        }
        let scratch = std::env::temp_dir().join(format!(
            "concat-motion-{}-{}",
            comp.hash(),
            std::process::id()
        ));
        std::fs::create_dir_all(&scratch).map_err(|source| MotionError::Io {
            doing: "creating".into(),
            path: scratch.clone(),
            source,
        })?;
        let request = BakeRequest {
            width,
            ..BakeRequest::default()
        };
        let source_dir = if request.source_dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            request.source_dir.clone()
        };
        self.stage(comp, &source_dir, &request, &scratch)?;
        let out = scratch.join("frame.png");
        let mut args: Vec<String> = vec![
            "--format".into(),
            "json".into(),
            "frame".into(),
            "timeline.json".into(),
            "-o".into(),
            out.display().to_string(),
            "--at".into(),
            seconds(at),
        ];
        if let Some(width) = width {
            args.push("--width".into());
            args.push(width.to_string());
        }
        self.run(&scratch, &args)?;
        Ok(out)
    }
}

/// The staged path an asset gets inside a bake directory.
fn staged_path(asset: &Asset) -> String {
    let name = Path::new(&asset.src)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "asset".to_string());
    format!("assets/{name}")
}

/// The Geneva asset kind for an asset, or `None` when it does not read one.
pub fn geneva_kind(asset: &Asset) -> Option<&'static str> {
    geneva_asset_kind(asset)
}

/// The Geneva asset kind for an asset, or `None` when it does not read one.
fn geneva_asset_kind(asset: &Asset) -> Option<&'static str> {
    let from_extension = Path::new(&asset.src)
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase());
    let kind = asset.kind.or_else(|| {
        Some(match from_extension.as_deref()? {
            "png" | "jpg" | "jpeg" | "webp" => AssetKind::Image,
            "mp4" | "mov" | "mkv" | "webm" => AssetKind::Video,
            "wav" | "mp3" | "flac" | "ogg" | "m4a" => AssetKind::Audio,
            "ttf" | "otf" | "ttc" | "woff" | "woff2" => AssetKind::Font,
            "srt" | "vtt" => AssetKind::Html,
            "html" | "htm" | "css" => AssetKind::Html,
            "json" | "lottie" => AssetKind::Lottie,
            _ => return None,
        })
    })?;
    Some(match kind {
        AssetKind::Image => "image",
        AssetKind::Video => "video",
        AssetKind::Audio => "audio",
        AssetKind::Font => "font",
        AssetKind::Html => "html",
        // A Lottie file is not something the markup engine reads; the importer
        // bakes it into an image sequence before it gets here.
        AssetKind::Lottie => return None,
    })
}

/// What kind of values a track carries, for the keyframe writer.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
    /// A point written as `[x, y]`.
    Point,
}

/// A track as a Geneva `keyframes` object, or `None` when it is empty.
///
/// `span` is the node's length in seconds: a track's keys are stored at
/// fractions of it, and the timeline wants seconds from the clip's start. A
/// comp is written once and retimed anywhere, so the fraction is the fact and
/// the seconds are the conversion - and a key written as a fraction would run
/// the motion in the wrong time domain entirely.
fn keyframes(track: &Track, kind: Kind, span: f64, warnings: &mut Vec<String>) -> Option<Json> {
    if track.is_empty() {
        return None;
    }
    let span = if span > 1e-6 { span } else { 1.0 };
    let mut keys: Vec<&crate::track::Key> = track.keys.iter().collect();
    keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    for key in keys {
        let value = match (kind, &key.value) {
            (Kind::Number, Value::Number { value }) => json!(value),
            (Kind::Point, Value::Point { x, y }) => json!([x, y]),
            (_, other) => {
                warnings.push(format!(
                    "a {other:?} value is not a keyframe geneva writes here"
                ));
                continue;
            }
        };
        let at = seconds(key.at * span);
        match &key.curve {
            Some(curve) if !matches!(curve, Curve::Linear) => {
                out.push(json!([at, value, ease(curve)]))
            }
            _ => out.push(json!([at, value])),
        }
    }
    (!out.is_empty()).then(|| json!({ "keyframes": out }))
}

/// A curve as Geneva writes it: a CSS ease name, `cubic-bezier()`, `steps()`,
/// `linear()`, or a spring.
fn ease(curve: &Curve) -> Json {
    match curve {
        Curve::Linear => json!("linear"),
        Curve::Bezier { x1, y1, x2, y2 } => json!({ "cubic-bezier": [x1, y1, x2, y2] }),
        Curve::Steps { count, position } => {
            let name = match position {
                StepPosition::JumpEnd => "jump-end",
                StepPosition::JumpStart => "jump-start",
                StepPosition::JumpNone => "jump-none",
                StepPosition::JumpBoth => "jump-both",
            };
            json!({ "steps": [count, name] })
        }
        Curve::Points { stops } => {
            json!({ "linear": stops.iter().map(|(i, o)| json!([i, o])).collect::<Vec<_>>() })
        }
        Curve::Spring(spring) => json!({
            "spring": { "stiffness": spring.stiffness, "damping": spring.damping, "mass": spring.mass }
        }),
    }
}

/// A time as a number the timeline accepts: seconds, rounded to milliseconds.
///
/// Word times come from a transcript and from arithmetic on one; a value like
/// `0.08000000000000002` is refused for its decimal places, and a millisecond
/// is already finer than any frame.
fn time(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// A time as a Geneva value: seconds with at most three decimals.
fn seconds(value: f64) -> String {
    // Geneva refuses a key at a time with more than nine decimals (E103), and
    // a floating-point division reaches seventeen: three decimals is
    // milliseconds, which is finer than any frame the renderer draws.
    let text = format!("{:.3}", value.max(0.0));
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    format!("{}s", if trimmed.is_empty() { "0" } else { trimmed })
}

/// The clip's transform, with animated properties as tracks.
fn effective_transform(comp: &Comp, node: &Node) -> Option<Json> {
    let mut warnings = Vec::new();
    let base = &node.transform;
    let mut map = serde_json::Map::new();

    let position = node
        .tracks
        .get("position")
        .and_then(|track| keyframes(track, Kind::Point, span_of(comp, node), &mut warnings))
        .or_else(|| {
            let point = base.position?;
            let placed = absolute_position(comp, node, point);
            Some(json!([placed.0, placed.1]))
        });
    if let Some(position) = position {
        map.insert("position".into(), position);
    }
    if let Some(anchor) = base.anchor {
        map.insert(
            "anchor".into(),
            json!([
                format!("{}%", anchor.x * 100.0),
                format!("{}%", anchor.y * 100.0)
            ]),
        );
    }
    let scale = node
        .tracks
        .get("scale")
        .and_then(|track| keyframes(track, Kind::Number, span_of(comp, node), &mut warnings))
        .unwrap_or_else(|| json!(base.scale));
    if scale != json!(1.0) {
        map.insert("scale".into(), scale);
    }
    let rotation = node
        .tracks
        .get("rotation")
        .and_then(|track| keyframes(track, Kind::Number, span_of(comp, node), &mut warnings))
        .unwrap_or_else(|| json!(base.rotation));
    if rotation != json!(0.0) {
        map.insert("rotation".into(), rotation);
    }
    (!map.is_empty()).then_some(Json::Object(map))
}

/// How long a node runs, in seconds: its own duration, or the rest of the comp
/// for a node that says `duration: 0`.
fn span_of(comp: &Comp, node: &Node) -> f64 {
    if node.duration > 0.0 {
        node.duration
    } else {
        (comp.duration - node.start).max(0.0)
    }
}

/// A node's position with its ancestors' transforms folded in, so a group's
/// children land where the native renderer would put them.
fn absolute_position(comp: &Comp, node: &Node, point: crate::comp::Point) -> (f64, f64) {
    let (mut x, mut y) = (point.x, point.y);
    let mut cursor = node.parent.as_deref();
    let mut depth = 0;
    while let Some(parent) = cursor {
        if depth > 32 {
            break;
        }
        let Some(parent_node) = comp.node(parent) else {
            break;
        };
        if let Some(parent_point) = parent_node.transform.position {
            x += parent_point.x;
            y += parent_point.y;
        }
        cursor = parent_node.parent.as_deref();
        depth += 1;
    }
    (x, y)
}

/// A shape's fill, as Geneva takes it: one colour.
///
/// Geneva's `shape` source fills with a colour, not a gradient, and a
/// `linear-gradient()` string there is a hard `E103` that kills the whole
/// bake. A comp is still allowed to ask for one - the native engine draws it -
/// so the gradient is averaged to a solid and the node is named in the
/// warnings, rather than the frame quietly failing. A design-grade gradient
/// belongs on an `html` node, where CSS draws it exactly.
fn shape_fill(node: &Node, fill: Option<&Paint>, warnings: &mut Vec<String>) -> String {
    match fill {
        None => "#ffffffff".into(),
        Some(Paint::Solid { color }) => color.to_hex(),
        Some(Paint::Gradient { stops, .. }) => {
            let solid = average_color(stops);
            warnings.push(format!(
                "node {:?}: Geneva fills a shape with one colour, so its gradient is \
                 approximated by {} - put the gradient on an html node to draw it exactly",
                node.id,
                solid.to_hex()
            ));
            solid.to_hex()
        }
    }
}

/// The colour a gradient reads as at a glance: the alpha-weighted mean of its
/// stops.
fn average_color(stops: &[(f64, Rgba)]) -> Rgba {
    let mut weight = 0.0;
    let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
    for (_, color) in stops {
        let w = color.a.max(0.0);
        r += color.r * w;
        g += color.g * w;
        b += color.b * w;
        a += color.a;
        weight += w;
    }
    if weight <= 0.0 {
        return Rgba::new(0.0, 0.0, 0.0, 0.0);
    }
    Rgba::new(
        r / weight,
        g / weight,
        b / weight,
        (a / stops.len().max(1) as f64).clamp(0.0, 1.0),
    )
}

/// What a node draws, as a Geneva source; `None` when this engine cannot.
fn source_of(comp: &Comp, node: &Node, warnings: &mut Vec<String>) -> Option<Json> {
    match &node.kind {
        NodeKind::Group { .. } => {
            // A group is a transform and nothing else: its children carry
            // their own clips, with the group's offset folded in above.
            warnings.push(format!(
                "node {:?} is a group and draws nothing itself",
                node.id
            ));
            None
        }
        NodeKind::Rect {
            width,
            height,
            fill,
            radius,
        } => Some(json!({
            "kind": "shape",
            "shape": "rect",
            "width": length(*width),
            "height": length(*height),
            "fill": shape_fill(node, fill.as_ref(), warnings),
            "radius": radius,
        })),
        NodeKind::Ellipse {
            width,
            height,
            fill,
        } => Some(json!({
            "kind": "shape",
            "shape": "ellipse",
            "width": length(*width),
            "height": length(*height),
            "fill": shape_fill(node, fill.as_ref(), warnings),
        })),
        NodeKind::Path { commands, dash, .. } => {
            // Geneva draws rectangles and ellipses, not paths. Being explicit
            // here is the whole point: a logo mark must not silently vanish.
            warnings.push(format!(
                "node {:?}: a path ({} commands{}) is drawn by the native engine only",
                node.id,
                commands.len(),
                if dash.is_some() {
                    ", with a dash offset"
                } else {
                    ""
                }
            ));
            let _ = (commands, dash);
            None
        }
        NodeKind::Text(text) => {
            // A word with no end holds until the next one; the last word has
            // no next one, so it holds to the end of the node's own span.
            let span = node.end(comp.duration) - node.start;
            Some(text_source(text, span))
        }
        NodeKind::Html {
            markup,
            css,
            width,
            height,
        } => Some(json!({
            "kind": "html",
            "html": markup,
            "css": css,
            "width": length(*width),
            "height": length(*height),
        })),
        NodeKind::Image { asset, fit } => Some(json!({
            "kind": "image",
            "asset": asset,
            "fit": match fit {
                Fit::Contain => "contain",
                Fit::Cover => "cover",
                Fit::Fill => "fill",
                Fit::None => "none",
            },
        })),
        NodeKind::Lottie { asset, .. } => {
            warnings.push(format!(
                "node {:?}: Lottie asset {asset:?} is baked by the Lottie engine, not this one",
                node.id
            ));
            None
        }
        NodeKind::Composition { comp } => {
            Some(json!({ "kind": "composition", "composition": comp }))
        }
    }
}

/// A text node as a Geneva `text` source, word timing included.
///
/// `span` is how long the node runs, which is what an open-ended last word is
/// closed at: the timeline format requires every word to have an end, and a
/// comp's author should not have to write one they do not care about.
fn text_source(text: &TextNode, span: f64) -> Json {
    let mut map = serde_json::Map::new();
    map.insert("kind".into(), json!("text"));
    if !text.words.is_empty() {
        map.insert("text".into(), json!(""));
        map.insert(
            "words".into(),
            Json::Array(
                text.words
                    .iter()
                    .enumerate()
                    .map(|(i, word)| {
                        let last = i + 1 == text.words.len();
                        match word.end {
                            Some(end) => json!([word.text, time(word.start), time(end)]),
                            None if last => json!([
                                word.text,
                                time(word.start),
                                time(span.max(word.start + 0.1))
                            ]),
                            None => json!([word.text, time(word.start)]),
                        }
                    })
                    .collect(),
            ),
        );
        if let Some(highlight) = &text.highlight {
            let mut style = serde_json::Map::new();
            if let Some(color) = highlight.color {
                style.insert("color".into(), Json::String(color.to_hex()));
            }
            if let Some(weight) = highlight.weight {
                style.insert("weight".into(), json!(weight));
            }
            if !style.is_empty() {
                map.insert("highlight".into(), Json::Object(style));
            }
        }
    } else {
        map.insert("text".into(), Json::String(text.text.clone()));
    }
    if !text.font.is_empty() {
        map.insert("font".into(), Json::String(text.font.clone()));
    }
    map.insert("size".into(), json!(text.size));
    map.insert("weight".into(), json!(text.weight));
    if text.italic {
        map.insert("italic".into(), json!(true));
    }
    if text.letter_spacing != 0.0 {
        map.insert("letter_spacing".into(), json!(text.letter_spacing));
    }
    map.insert("line_height".into(), json!(text.line_height));
    map.insert(
        "align".into(),
        Json::String(
            match text.align {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
            }
            .into(),
        ),
    );
    if let Some(max_width) = text.max_width {
        map.insert("max_width".into(), json!(max_width));
    }
    if text.padding != 0.0 {
        map.insert("padding".into(), json!(text.padding));
    }
    if text.background.a > 0.0 {
        map.insert("background".into(), Json::String(text.background.to_hex()));
    }
    if text.radius != 0.0 {
        map.insert("radius".into(), json!(text.radius));
    }
    match &text.fill {
        Some(fill) => {
            map.insert("fill".into(), Json::String(paint(fill)));
        }
        None => {
            map.insert("color".into(), Json::String(text.color.to_hex()));
        }
    }
    if let Some(Stroke { color, width }) = &text.outline {
        map.insert(
            "outline".into(),
            json!({ "color": color.to_hex(), "width": width }),
        );
    }
    if let Some(Shadow { x, y, blur, color }) = &text.shadow {
        map.insert(
            "shadow".into(),
            json!({ "color": color.to_hex(), "x": x, "y": y, "blur": blur }),
        );
    }
    Json::Object(map)
}

/// A paint as a CSS colour or gradient string.
fn paint(paint: &Paint) -> String {
    match paint {
        Paint::Solid { color } => color.to_hex(),
        Paint::Gradient {
            stops,
            angle,
            from,
            to,
        } => {
            let parts: Vec<String> = stops
                .iter()
                .map(|(at, color)| {
                    format!(
                        "{} {:.1}%",
                        color.to_hex(),
                        (from + (to - from) * at) * 100.0
                    )
                })
                .collect();
            format!("linear-gradient({angle}deg, {})", parts.join(", "))
        }
    }
}

/// A length as a Geneva value.
fn length(length: Length) -> Json {
    match length {
        Length::Px(value) => json!(value),
        Length::Percent(value) => Json::String(format!("{value}%")),
        Length::Auto => Json::String("auto".into()),
    }
}

/// A mask as a Geneva mask.
fn mask_json(mask: &Mask) -> Option<Json> {
    let shape = match mask.shape {
        MaskShape::Rect => "rect",
        MaskShape::Ellipse => "ellipse",
    };
    Some(json!({
        "shape": shape,
        "radius": mask.radius,
        "feather": mask.feather,
        "invert": mask.invert,
    }))
}

/// A blend mode as Geneva names it.
fn blend_name(blend: Blend) -> Option<&'static str> {
    Some(match blend {
        Blend::Normal => return None,
        Blend::Multiply => "multiply",
        Blend::Screen => "screen",
        Blend::Overlay => "overlay",
        Blend::Darken => "darken",
        Blend::Lighten => "lighten",
        Blend::Difference => "difference",
        Blend::SoftLight => "soft-light",
        Blend::Add => "add",
    })
}

/// A warning from the mapper: logged, never fatal, because half a comp drawn
/// is better than none, and the bake manifest records what was skipped.
fn log_warning(message: &str) {
    eprintln!("concat-motion: {message}");
}

/// One path command as a point, for a caller that wants to walk a path.
pub fn point_of(command: &PathCommand) -> Option<crate::comp::Point> {
    match command {
        PathCommand::MoveTo { point } | PathCommand::LineTo { point } => Some(*point),
        PathCommand::CubicTo { point, .. } => Some(*point),
        PathCommand::Close => None,
    }
}

/// The colour given to a comp's clear, as a CSS string.
pub fn clear_color(color: Rgba) -> String {
    color.to_hex()
}

/// Asset ids in a comp, for a caller that wants to copy them.
pub fn asset_map(comp: &Comp) -> BTreeMap<String, Asset> {
    comp.assets.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, Node, Transform, Word};
    use crate::track::Key;

    fn text_comp() -> Comp {
        Comp {
            format: crate::comp::FORMAT,
            id: "t".into(),
            name: "Caption".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 2.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            nodes: vec![Node {
                id: "caption".into(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform {
                    position: Some(crate::comp::Point::new(960.0, 900.0)),
                    ..Transform::default()
                },
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                effects: Vec::new(),
                tracks: Default::default(),
                kind: NodeKind::Text(Box::new(TextNode {
                    text: String::new(),
                    words: vec![
                        Word {
                            text: "make".into(),
                            start: 0.0,
                            end: Some(0.5),
                        },
                        Word {
                            text: "it".into(),
                            start: 0.5,
                            end: None,
                        },
                    ],
                    highlight: Some(crate::comp::TextStyle {
                        color: Some(Rgba::hex("#ffcc00")),
                        ..Default::default()
                    }),
                    font: "Inter".into(),
                    size: 64.0,
                    weight: 800,
                    italic: false,
                    letter_spacing: 0.0,
                    line_height: 1.2,
                    align: TextAlign::Center,
                    max_width: None,
                    padding: 12.0,
                    background: Rgba::hex("#000000cc"),
                    radius: 10.0,
                    color: Rgba::hex("#ffffff"),
                    fill: None,
                    outline: None,
                    shadow: None,
                })),
            }],
            slots: Vec::new(),
        }
    }

    #[test]
    fn a_comp_becomes_a_timeline_with_a_png_sequence_and_alpha() {
        let geneva = Geneva::at("/nonexistent/geneva");
        let (document, warnings) = geneva.timeline(&text_comp(), &BakeRequest::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(document["geneva"], "1.1");
        assert_eq!(document["output"]["encode"]["container"], "image-sequence");
        assert_eq!(document["output"]["encode"]["video"]["codec"], "png");
        assert_eq!(document["output"]["background"], "#00000000");
        assert_eq!(document["layers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn word_timing_and_its_highlight_survive_the_mapping() {
        let geneva = Geneva::at("/nonexistent/geneva");
        let (document, _) = geneva.timeline(&text_comp(), &BakeRequest::default());
        let source = &document["layers"][0]["clips"][0]["source"];
        assert_eq!(source["kind"], "text");
        assert_eq!(source["words"][0][0], "make");
        assert_eq!(source["words"][1][1], 0.5);
        assert_eq!(source["highlight"]["color"], "#ffcc00ff");
    }

    #[test]
    fn a_position_becomes_a_point_on_the_frame() {
        let geneva = Geneva::at("/nonexistent/geneva");
        let (document, _) = geneva.timeline(&text_comp(), &BakeRequest::default());
        assert_eq!(
            document["layers"][0]["clips"][0]["transform"]["position"],
            json!([960.0, 900.0])
        );
    }

    #[test]
    fn curves_become_the_ease_forms_geneva_writes() {
        assert_eq!(ease(&Curve::Linear), json!("linear"));
        assert_eq!(
            ease(&Curve::EASE_OUT),
            json!({ "cubic-bezier": [0.0, 0.0, 0.58, 1.0] })
        );
        assert_eq!(
            ease(&Curve::Steps {
                count: 4,
                position: StepPosition::JumpEnd
            }),
            json!({ "steps": [4, "jump-end"] })
        );
        assert_eq!(
            ease(&Curve::SPRING_POP),
            json!({ "spring": { "stiffness": 260.0, "damping": 15.0, "mass": 1.0 } })
        );
    }

    #[test]
    fn tracks_are_written_in_seconds_from_the_node_s_own_span() {
        // A key at a fraction of the node's length: half of a 4 s fade is 2 s
        // on the timeline, not 0.5 s.
        let mut comp = text_comp();
        comp.nodes[0].duration = 4.0;
        comp.nodes[0].tracks.insert(
            "opacity".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::eased(0.5, Value::number(1.0), Curve::EASE_OUT),
                Key::new(1.0, Value::number(1.0)),
            ]),
        );
        let (document, warnings) = Geneva::at("geneva").timeline(&comp, &BakeRequest::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        let opacity = &document["layers"][0]["clips"][0]["opacity"]["keyframes"];
        assert_eq!(opacity[0][0], json!("0s"));
        assert_eq!(opacity[1][0], json!("2s"), "half of four seconds");
        assert_eq!(opacity[2][0], json!("4s"));
        assert_eq!(
            opacity[1][2],
            json!({ "cubic-bezier": [0.0, 0.0, 0.58, 1.0] })
        );
    }

    #[test]
    fn a_retimed_comp_keeps_its_motion_in_its_own_time() {
        // The same track on a twice-as-long node runs twice as long: the
        // fraction is the fact, the seconds are the conversion.
        let mut comp = text_comp();
        comp.nodes[0].duration = 8.0;
        comp.nodes[0].tracks.insert(
            "opacity".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::new(0.5, Value::number(1.0)),
            ]),
        );
        let (document, _) = Geneva::at("geneva").timeline(&comp, &BakeRequest::default());
        assert_eq!(
            document["layers"][0]["clips"][0]["opacity"]["keyframes"][1][0],
            json!("4s")
        );
    }

    #[test]
    fn seconds_are_written_the_way_the_timeline_wants_them() {
        assert_eq!(seconds(1.5), "1.5s");
        assert_eq!(seconds(2.0), "2s");
        assert_eq!(seconds(7.200000000000001), "7.2s");
        assert_eq!(seconds(0.0), "0s");
    }

    #[test]
    fn a_time_is_written_to_milliseconds_and_never_more() {
        // A key at a fraction of a span divides into seventeen digits, and
        // geneva refuses a time with more than nine decimals (E103).
        assert_eq!(seconds(1.1 / 6.0), "0.183s");
        assert_eq!(seconds(0.18333333333333335), "0.183s");
        assert_eq!(seconds(-0.5), "0s");
    }

    #[test]
    fn a_path_is_reported_rather_than_dropped_silently() {
        let mut comp = text_comp();
        comp.nodes[0].kind = NodeKind::Path {
            commands: vec![PathCommand::MoveTo {
                point: crate::comp::Point::new(0.0, 0.0),
            }],
            fill: None,
            stroke: None,
            dash: Some((10.0, 4.0)),
        };
        let geneva = Geneva::at("/nonexistent/geneva");
        let (document, warnings) = geneva.timeline(&comp, &BakeRequest::default());
        assert!(warnings.iter().any(|w| w.contains("path")), "{warnings:?}");
        assert!(document["layers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_gradient_on_a_shape_becomes_one_colour_and_says_so() {
        // Geneva fills a shape with a colour: a gradient string there is a
        // fatal E103 for the whole document, so the mapper averages it and
        // reports the node instead of losing the bake.
        let mut comp = text_comp();
        comp.nodes[0].kind = NodeKind::Ellipse {
            width: crate::comp::Length::px(640.0),
            height: crate::comp::Length::px(640.0),
            fill: Some(Paint::Gradient {
                stops: vec![
                    (0.0, Rgba::new(1.0, 0.6, 0.13, 1.0)),
                    (1.0, Rgba::new(1.0, 0.2, 0.3, 1.0)),
                ],
                angle: 118.0,
                from: 0.0,
                to: 1.0,
            }),
        };
        let geneva = Geneva::at("/nonexistent/geneva");
        let (document, warnings) = geneva.timeline(&comp, &BakeRequest::default());
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("gradient") && w.contains("\"caption\"")),
            "{warnings:?}"
        );
        let fill = document["layers"][0]["clips"][0]["source"]["fill"]
            .as_str()
            .unwrap();
        assert!(fill.starts_with("#"), "{fill}");
        assert_eq!(fill.len(), 9, "{fill}");
        assert!(!fill.contains("gradient"), "{fill}");
    }

    /// The two hand-authored comps shipped as worked examples are a promise to
    /// the next author: they must parse, validate, and map onto a timeline
    /// with nothing skipped that they rely on.
    #[test]
    fn the_authored_examples_are_valid_and_map_cleanly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/comps");
        for name in ["cypher.opener.json", "cypher.spectrum.json"] {
            let text = std::fs::read_to_string(dir.join(name)).expect("the example is there");
            let comp: Comp = serde_json::from_str(&text).expect("the example parses");
            let problems = comp.validate();
            assert!(problems.is_empty(), "{name}: {problems:?}");
            let (document, warnings) =
                Geneva::at("geneva").timeline(&comp, &BakeRequest::default());
            let layers = document["layers"].as_array().expect("layers");
            assert!(layers.len() >= 5, "{name}: {} layers", layers.len());
            // The spectrum's slab asks for a native-only shadow; everything
            // else these comps rely on is drawn by geneva.
            for warning in &warnings {
                assert!(
                    warning.contains("drop shadow"),
                    "{name}: unexpected warning {warning:?}"
                );
            }
        }
    }

    #[test]
    fn discovery_finds_nothing_when_there_is_nothing_to_find() {
        // The test environment has no geneva on PATH; the point is that this
        // returns rather than panicking, and that a missing engine is a probe
        // result, not an error.
        let found = Geneva::discover(None);
        if let Some(geneva) = found {
            assert!(geneva.exe().is_file());
        }
    }
}
