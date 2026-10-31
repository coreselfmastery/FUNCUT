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
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// Why a version cannot be trusted, when its major is not the one this crate
/// was written for. Optional fields may arrive within a major; a different
/// major is a different format, and is never silently baked with.
fn major_mismatch(version: &str) -> Option<String> {
    let tested = TESTED_VERSION.split('.').next().unwrap_or_default();
    let found = version.split('.').next().unwrap_or_default();
    (tested.is_empty() || found.is_empty() || tested != found).then(|| {
        format!(
            "geneva {version} is a different major version than the {TESTED_VERSION} this was written for"
        )
    })
}

/// The environment variable that points at the binary, for a build that brings
/// its own.
pub const ENV_BIN: &str = "CONCAT_GENEVA_BIN";

/// How long one Geneva call may run before it is killed.
///
/// Two limits, because the two kinds of call differ by an order of
/// magnitude: a plan, a validate or a frame is a document read and one
/// still, and a minute is already generous; a render is every frame of a
/// reel, and 300 s covers the long ones this crate's own examples
/// measure (300 frames of 1080p30 in 23 s on two cores - a tenfold
/// margin). The breaker window and these limits are the whole answer to
/// "what if the sidecar hangs": a call that outlives its limit is
/// killed, and a sidecar that fails three times in a minute is refused
/// until it has had a minute to be fixed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CallLimits {
    /// Plan, validate, frame, probe, version: the quick calls.
    pub plan: Duration,
    /// Render: every frame of a comp or a reel, plus the encode.
    pub render: Duration,
}

impl Default for CallLimits {
    fn default() -> Self {
        CallLimits {
            plan: Duration::from_secs(60),
            render: Duration::from_secs(300),
        }
    }
}

impl CallLimits {
    /// Limits from one cap, the shape a `--geneva-timeout` flag or a
    /// tool parameter gives: no call may run past `cap`, and the quick
    /// calls are capped at their own default when the cap is looser
    /// than it.
    pub fn capped_at(cap: Duration) -> CallLimits {
        CallLimits {
            plan: cap.min(Duration::from_secs(60)),
            render: cap,
        }
    }
}

/// The circuit breaker's state, as [`Geneva::breaker`] reports it: the
/// count is the failures inside the window, and the window itself is
/// [`BREAKER_WINDOW_SECS`] - three inside sixty seconds trips it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Breaker {
    /// Closed: the sidecar is being called as usual.
    Closed,
    /// Open: `failures` calls failed inside the window, and further
    /// calls are refused until the window has passed without more of
    /// them.
    Open {
        /// How many failures tripped it.
        failures: usize,
    },
}

/// How many failures inside the window trip the breaker.
const BREAKER_TRIPS_AT: usize = 3;
/// The breaker's window, in seconds: failures older than this fall out
/// of the count, and an open breaker closes again after it.
const BREAKER_WINDOW_SECS: f64 = 60.0;

/// One lock for every test that touches the process-wide breaker: the
/// guard tests trip it, the reel tests' stubs clear it with their
/// successes, and neither may happen inside the other's span.
#[cfg(test)]
pub(crate) static BREAKER_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Forget the breaker's failures, for the test about to make its own.
/// Every test that drives the sidecar calls this under the lock, so one
/// test's open circuit is never the next test's surprise.
#[cfg(test)]
pub(crate) fn reset_breaker_for_tests() {
    if let Ok(mut failures) = BREAKER.lock() {
        failures.clear();
    }
}

/// The breaker is process-wide, because the sidecar is: three failures
/// from any door - a bake, a frame, a reel - are three failures against
/// the same binary, and the next caller whoever it is should not pay to
/// learn that.
static BREAKER: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

/// Count a failure into the breaker, dropping the ones that have aged
/// out of the window first.
fn breaker_failure() {
    if let Ok(mut failures) = BREAKER.lock() {
        let now = Instant::now();
        failures.retain(|at| now.duration_since(*at).as_secs_f64() < BREAKER_WINDOW_SECS);
        failures.push(now);
    }
}

/// Forget the failures: a call succeeded, and a sidecar that answers is
/// a sidecar to keep calling.
fn breaker_success() {
    if let Ok(mut failures) = BREAKER.lock() {
        failures.clear();
    }
}

/// The breaker's state, with the aged-out failures dropped.
fn breaker_state() -> Breaker {
    let Ok(failures) = BREAKER.lock() else {
        return Breaker::Closed;
    };
    let now = Instant::now();
    let live = failures
        .iter()
        .filter(|at| now.duration_since(**at).as_secs_f64() < BREAKER_WINDOW_SECS)
        .count();
    if live >= BREAKER_TRIPS_AT {
        Breaker::Open { failures: live }
    } else {
        Breaker::Closed
    }
}

/// A discovered Geneva binary.
#[derive(Clone, PartialEq, Debug)]
pub struct Geneva {
    exe: PathBuf,
    limits: CallLimits,
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
            .map(|exe| Geneva {
                exe,
                limits: CallLimits::default(),
            })
    }

    /// A Geneva at a known path, without checking that it runs.
    pub fn at(exe: impl Into<PathBuf>) -> Geneva {
        Geneva {
            exe: exe.into(),
            limits: CallLimits::default(),
        }
    }

    /// Tighten or loosen the per-call limits, for a flag, a tool
    /// parameter or a test.
    pub fn with_limits(mut self, limits: CallLimits) -> Geneva {
        self.limits = limits;
        self
    }

    /// The limits this Geneva is called under.
    pub fn limits(&self) -> CallLimits {
        self.limits
    }

    /// The breaker's state right now, for a caller deciding what to draw
    /// with - see [`crate::engine::EnginePreference::choose`], which
    /// turns an open breaker into a native-engine fallback for every
    /// comp that can be drawn natively.
    pub fn breaker(&self) -> Breaker {
        breaker_state()
    }

    /// The binary's path.
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Refuse if the breaker is open: the message is the fallback story,
    /// so the caller hears what happens next instead of spending a
    /// minute learning the sidecar is not answering.
    fn gate(&self) -> Result<(), MotionError> {
        match breaker_state() {
            Breaker::Closed => Ok(()),
            Breaker::Open { failures } => Err(MotionError::CircuitOpen {
                failures,
                window: BREAKER_WINDOW_SECS,
            }),
        }
    }

    /// The version it reports, e.g. `1.6.0`.
    ///
    /// A binary that does not exit cleanly, or that prints nothing reading as
    /// a version, is a failure to answer rather than a version `""`: a wrong
    /// executable answering to the sidecar's name would otherwise be
    /// believed forever.
    pub fn version(&self) -> Result<String, MotionError> {
        self.gate()?;
        let output = self.call("probing", &["--version".to_owned()], self.limits.plan)?;
        let text = String::from_utf8_lossy(&output.stdout);
        // "geneva 1.6.0": the version is the first token that begins as one.
        let version = text.split_whitespace().find(|token| {
            token
                .split('.')
                .next()
                .is_some_and(|first| !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()))
        });
        match version {
            Some(version) if output.status.success() => {
                breaker_success();
                Ok(version.to_string())
            }
            _ => {
                breaker_failure();
                Err(MotionError::Engine {
                    code: None,
                    message: format!(
                        "{} did not report a version (exited with {})",
                        self.exe.display(),
                        output.status
                    ),
                    path: None,
                    help: Some(
                        "the sidecar is geneva-render/geneva; check what answers to its name"
                            .into(),
                    ),
                })
            }
        }
    }

    /// Whether this binary can draw markup comps, and what it is.
    ///
    /// A different major version is reported as the risk it is: the probe
    /// still finds it - a settings page wants to - but a bake is refused
    /// with the same reason, so a format the crate was not written for is
    /// never quietly drawn with.
    pub fn probe(&self) -> Probe {
        match self.version() {
            Ok(version) => {
                let major_ok = major_mismatch(&version).is_none();
                Probe {
                    available: true,
                    version: Some(version.clone()),
                    detail: if major_ok {
                        format!("geneva {version} at {}", self.exe.display())
                    } else {
                        format!(
                            "geneva {version} is a different major version than the {TESTED_VERSION} this was written for; bakes are refused with the reason"
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
        let mut args = vec!["guide".to_owned()];
        args.extend(topic.split_whitespace().map(str::to_owned));
        self.call("reading the manual", &args, self.limits.plan)
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// What a diagnostic code means, in the sidecar's own words:
    /// `geneva explain E302`. The answer's first line is the sentence a
    /// caller wants to carry into its own message; the rest is a pointer
    /// at the guide page, which stays in the manual.
    pub fn explain(&self, code: &str) -> Option<String> {
        let output = self
            .call(
                "explaining a diagnostic",
                &["explain".to_owned(), code.to_owned()],
                self.limits.plan,
            )
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty()
                    && !trimmed.starts_with("error[")
                    && !trimmed.starts_with("The whole page")
            })
            .map(str::to_owned)
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
                json!({ "src": staged_path(id, asset), "kind": kind }),
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
            let transform = effective_transform(comp, node, &mut warnings);
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
        // Frames with alpha: the one carrier the engine guarantees, and
        // the one the compositor samples without a conversion. An
        // image-sequence of PNGs carries its alpha channel whole - that
        // is what the container is - so a motion card's bake lands
        // translucent where the comp was, which is what lets it sit on
        // footage in an export instead of covering it. (The engine's
        // schema has no `pixel_format` field anywhere: the PNG sequence
        // IS the alpha, and saying so twice was refused as E101 - a
        // refusal every comp bake and every comp frame met.)
        output.insert(
            "encode".into(),
            json!({
                "container": "image-sequence",
                "video": { "codec": "png" }
            }),
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
        for (id, asset) in &comp.assets {
            let from = source_dir.join(&asset.src);
            if !from.is_file() {
                continue;
            }
            let to = dir.join(staged_path(id, asset));
            if to.is_file() {
                continue;
            }
            // A copy that did not finish must not be believed whole by the
            // next bake: it lands beside the bake as a `.part` and is
            // renamed in only when it is complete.
            let part = to.with_extension("part");
            std::fs::copy(&from, &part).map_err(|source| MotionError::Io {
                doing: "copying an asset into the bake".into(),
                path: from,
                source,
            })?;
            std::fs::rename(&part, &to).map_err(|source| MotionError::Io {
                doing: "finishing an asset copy".into(),
                path: to.clone(),
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
                path: None,
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
    ///
    /// Every call is bounded and counted. Bounded: the call runs under the
    /// limit its verb earns - `render` under [`CallLimits::render`],
    /// everything else under [`CallLimits::plan`] - and a call that
    /// outlives it is killed and named, so a sidecar that hangs costs a
    /// minute and a message, not a session. Counted: a failed call counts
    /// against the circuit breaker and a successful one closes it, so a
    /// sidecar that fails three times in a minute is refused with the
    /// fallback story instead of being asked a fourth.
    fn run(&self, dir: &Path, args: &[String]) -> Result<Json, MotionError> {
        self.gate()?;
        let what = if args.iter().any(|arg| arg == "render") {
            "rendering"
        } else {
            "planning"
        };
        let limit = if what == "rendering" {
            self.limits.render
        } else {
            self.limits.plan
        };
        let output = self.call_in(what, dir, args, limit)?;
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
            breaker_success();
            return Ok(report);
        }
        breaker_failure();
        // The first error diagnostic is the one worth showing; the rest are
        // usually consequences of it. Its code, message and location are
        // kept whole, and the help is the most specific of what the engine
        // itself suggested, what this crate knows about the code in
        // FUNCUT's own terms, and what the sidecar's manual says the code
        // means.
        let diagnostics = Diagnostic::list(&report);
        let first = diagnostics.iter().find(|d| d.is_error());
        let (code, message, path) = match first {
            Some(diagnostic) => (
                diagnostic.code.clone(),
                diagnostic.message.clone(),
                diagnostic.path.clone(),
            ),
            None => (None, format!("geneva exited with {}", output.status), None),
        };
        let help = match first {
            Some(diagnostic) => diagnostic
                .help
                .clone()
                .or_else(|| code.as_deref().and_then(fix_for).map(str::to_owned))
                .or_else(|| code.as_deref().and_then(|code| self.explain(code))),
            None => None,
        };
        Err(MotionError::Engine {
            code,
            message,
            path,
            help,
        })
    }

    /// Run the binary once, under a deadline: a watcher polls the child
    /// while reader threads drain its pipes (a child that fills a pipe
    /// blocks on writing, and a watcher that does not read would hang a
    /// chatty child at the buffer's edge), and a child that outlives the
    /// deadline is killed and said so - the one behaviour a caller can
    /// never otherwise get out of a hung sidecar.
    fn call(&self, what: &str, args: &[String], limit: Duration) -> Result<Output, MotionError> {
        self.call_in(what, Path::new("."), args, limit)
    }

    /// [`call`], in a working directory.
    fn call_in(
        &self,
        what: &str,
        dir: &Path,
        args: &[String],
        limit: Duration,
    ) -> Result<Output, MotionError> {
        let result = self.spawn_and_wait(what, dir, args, limit);
        if result.is_err() {
            // A call that could not start, or that outlived its limit,
            // is a failure against the breaker exactly as a refusal is:
            // the next caller should not pay to learn the same thing.
            breaker_failure();
        }
        result
    }

    /// Spawn the binary, wait for it under a deadline, and drain its
    /// pipes on their own threads (a child that fills a pipe blocks on
    /// writing, and a watcher that does not read would hang a chatty
    /// child at the buffer's edge). A child that outlives the deadline
    /// is killed and said so - the one behaviour a caller can never
    /// otherwise get out of a hung sidecar.
    fn spawn_and_wait(
        &self,
        what: &str,
        dir: &Path,
        args: &[String],
        limit: Duration,
    ) -> Result<Output, MotionError> {
        let mut child = Command::new(&self.exe)
            .current_dir(dir)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| MotionError::Io {
                doing: "running geneva".into(),
                path: self.exe.clone(),
                source,
            })?;
        // Drain the pipes on their own threads, so a report bigger than a
        // pipe never blocks the child that is writing it.
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut bytes);
                }
                bytes
            })
        };
        let out_thread = drain(stdout.map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>));
        let err_thread = drain(stderr.map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>));

        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(source) => {
                    return Err(MotionError::Io {
                        doing: "waiting for geneva".into(),
                        path: self.exe.clone(),
                        source,
                    });
                }
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(MotionError::TimedOut {
                    what: what.to_owned(),
                    limit: limit.as_secs_f64(),
                });
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}

/// What a finished child left behind: its exit status and its two pipes.
struct Output {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// One diagnostic from a Geneva report, parsed whole: the stable code, the
/// severity, the JSON-pointer path into the timeline, the message, the
/// offending value and the engine's own suggested fix.
///
/// `--format json` is asked for on every invocation, so every refusal the
/// engine makes arrives as these rather than as an exit code to guess at;
/// see `geneva guide errors` for the code tables.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Diagnostic {
    /// `error`, `warning` or `note`.
    pub severity: String,
    /// The stable code, e.g. `E302` or `W405`.
    pub code: Option<String>,
    /// Where in the document: a JSON pointer into the timeline, or a file
    /// and line for a markup problem.
    pub path: Option<String>,
    /// What went wrong, in one sentence.
    pub message: String,
    /// The offending value, when the engine names it.
    pub value: Option<String>,
    /// What the engine itself suggests.
    pub help: Option<String>,
}

impl Diagnostic {
    /// Whether this diagnostic stops the render.
    pub fn is_error(&self) -> bool {
        self.severity == "error"
    }

    /// Every diagnostic in a report, errors first-parsed-as-found.
    fn list(report: &Json) -> Vec<Diagnostic> {
        report
            .get("diagnostics")
            .and_then(Json::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|entry| {
                        let text = |field: &str| {
                            entry.get(field).and_then(Json::as_str).map(str::to_owned)
                        };
                        let severity = text("severity").unwrap_or_default();
                        if severity.is_empty() {
                            return None;
                        }
                        Some(Diagnostic {
                            severity,
                            code: text("code"),
                            path: text("path"),
                            message: text("message").unwrap_or_default(),
                            value: text("value"),
                            help: text("help"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// What a Geneva diagnostic code means in FUNCUT's own terms: the same fix
/// the engine's help offers, said in the words of the storyboard and the
/// comp that caused it. The codes are the sidecar's own tables (`geneva
/// guide errors`); an unknown code falls back to [`Geneva::explain`],
/// which asks the sidecar itself.
pub fn fix_for(code: &str) -> Option<&'static str> {
    match code {
        // Structure: the document itself.
        "E100" => Some(
            "the timeline the planner wrote is not valid JSON, which is this crate's bug - \
             please report it with the storyboard that produced it",
        ),
        "E101" | "E102" | "E103" | "E104" | "E105" => Some(
            "the timeline a comp produced does not match the format the sidecar reads; a \
             hand-edited comp is the usual cause - check the comp's JSON against \
             `geneva schema`",
        ),
        "E110" => Some(
            "the sidecar is a different major version than the timeline this build writes; \
             reinstall the pinned geneva (install-linux.sh) or point CONCAT_GENEVA_BIN at \
             the one this build was written for",
        ),
        // References: what a document names.
        "E200" => Some(
            "a layer or an audio bed names an asset the storyboard does not carry; check the \
             id against the storyboard's `assets`",
        ),
        "E201" => Some(
            "an asset is used as a kind it is not: a picture where a sound is named, or the \
             reverse; check the asset's file and the layer that names it",
        ),
        "E202" => Some(
            "an asset path must be a name under the project, not a route out of it; check \
             the storyboard's `assets`",
        ),
        "E203" => Some(
            "give the asset a `kind`, or a file name whose extension names one: the sidecar \
             could not tell what the file is",
        ),
        "E206" => Some(
            "the composition a clip names is not declared; check the comp's compositions \
             against the clip that points at one",
        ),
        "E207" => Some(
            "a composition contains itself, directly or through another; a scene cannot be \
             built from itself",
        ),
        // Timing: what a storyboard lays out.
        "E300" => {
            Some("a time is negative; check the layer's `start` and `hold` and the scene's `at`")
        }
        "E301" => Some(
            "an empty or inverted range; a layer's `hold` must outlast its fades, and a \
             scene's `at` cannot swallow the scene before it",
        ),
        "E302" => Some(
            "two clips of one layer overlap without a transition; the planner gives every \
             layer its own, so this is a hand-edited timeline - move one clip or add the \
             transition that blends them",
        ),
        "E303" => Some(
            "keyframes must strictly increase in time; the planner holds every fade to half \
             its span for exactly this, so check a hand-edited timeline's keyframe times",
        ),
        "E305" => Some(
            "a clip's length could not be determined; a storyboard layer needs a `hold` or a \
             scene long enough to end it",
        ),
        "E306" => Some(
            "a transition is longer than the overlap before it; shorten the transition or \
             lengthen the overlap",
        ),
        // Values and markup: what a comp carries.
        "E401" => Some(
            "an easing is malformed: cubic-bezier control points outside 0..=1, or \
             non-positive spring parameters; check the comp's curves",
        ),
        "E402" => Some(
            "a value is out of range: opacity outside 0..=1, a non-positive size or \
             duration, a font weight outside 100..=900; check the comp's numbers",
        ),
        "E440" => Some(
            "the comp's animation names a keyframes rule the document does not carry; add \
             the missing @keyframes block to the comp's CSS or take the animation off the \
             node",
        ),
        "E441" => Some(
            "an `animation` value is malformed: a rule name, one duration and one timing \
             function - check the node's animation",
        ),
        "E442" => Some(
            "a keyframes rule is malformed: offsets are `from`, `to` or a percentage in \
             0%..100%, and a clip's rule animates transform and opacity only; check the \
             comp's CSS",
        ),
        "E443" => Some(
            "one property is driven by the clip's keyframes and by an animation at once; \
             keep the property in one of the two",
        ),
        "E450" => Some(
            "an HTML source carries either `html` or `asset`, not both and not neither; \
             check the comp's HTML node",
        ),
        "E451" => Some(
            "the comp's markup or CSS did not parse; the message and the location say where \
             - an unclosed tag, a selector the engine does not support, a block that never \
             closes",
        ),
        "E452" => Some(
            "the markup points at a file that is not there: paths inside markup are relative \
             to the markup itself, and must stay under the asset root; carry the file as an \
             asset and point at its staged name",
        ),
        "E453" => Some(
            "a caption file could not be read or parsed; check that it is .srt, .vtt or a \
             recogniser's .json, and that its word times are seconds",
        ),
        // Rendering: what a render meets.
        "E501" => Some(
            "an asset file could not be opened or decoded; check the asset's path under the \
             project and that the file is a readable picture, video, sound or font",
        ),
        "E502" => Some(
            "a frame was asked for outside the composition; check the time against the \
             reel's duration",
        ),
        "E503" => Some(
            "the renderer's device failed or ran out of memory; the CPU renderer never \
             reports it - retry, and if it repeats, report it with the comp",
        ),
        "E504" => Some(
            "this output cannot be split into parts for parallel rendering; render it in one \
             call",
        ),
        // Warnings a storyboard author meets often enough to want in
        // their own words.
        "W405" => Some(
            "a font family this machine has no face of; carry the face as a `font` asset in \
             the storyboard's `assets` and the comp draws the same everywhere",
        ),
        "W407" => Some(
            "letters no available face has a glyph for; add a font asset that covers them, \
             or the boxes another machine will not show",
        ),
        "W440" => Some(
            "an animation rule interpolates nothing; give the rule a property at two \
             offsets",
        ),
        _ => None,
    }
}

/// A bake that already sits in `dir`, when its manifest matches the comp,
/// the engine, the engine's version, the size, the rate and the alpha -
/// and the frames it promises are actually still on disk. The comp's hash
/// is the cache key, so "an unchanged comp re-renders nothing" stops being
/// a promise the bake loop quietly broke on every call.
fn bake_cache_hit(dir: &Path, comp: &Comp, request: &BakeRequest, version: &str) -> Option<Baked> {
    if request.frames.is_some() {
        // A ranged (parallel) bake is not a full cache entry; render it.
        return None;
    }
    let manifest: Baked =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    if manifest.hash != comp.hash()
        || manifest.engine != EngineKind::Geneva
        || manifest.engine_version != version
        || manifest.width != request.width(comp)
        || manifest.height != request.height(comp)
        || manifest.fps != request.fps(comp)
        || manifest.alpha != request.alpha
        || manifest.frames == 0
    {
        return None;
    }
    // The manifest's own `dir` was written relative to wherever the bake
    // ran; count the frames where this caller says the bake lives, so a
    // moved project or a trimmed cache misses instead of lying.
    let frames_dir = dir.join("frames");
    let present = std::fs::read_dir(&frames_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
        .count();
    if present < manifest.frames {
        return None;
    }
    let mut hit = manifest;
    hit.cached = true;
    Some(hit)
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
        if let Some(reason) = major_mismatch(&version) {
            return Err(MotionError::Engine {
                code: None,
                message: reason,
                path: None,
                help: Some(format!(
                    "point {ENV_BIN} at a geneva the comp was written for, or reinstall the sidecar"
                )),
            });
        }
        let source_dir = if request.source_dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            request.source_dir.clone()
        };
        // The cache answers before anything is staged or rendered: an
        // unchanged comp at the same size and rate has nothing new to say,
        // and the inspect -> frame -> bake loop stops paying for a full
        // re-render on every iteration.
        if let Some(hit) = bake_cache_hit(dir, comp, request, &version) {
            return Ok(hit);
        }
        self.stage(comp, &source_dir, request, dir)?;

        let frames = dir.join("frames");
        // A bake counts what the render wrote, so a stale `frames/` left by
        // an interrupted run must not survive into the count: it is emptied
        // first.
        let _ = std::fs::remove_dir_all(&frames);
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
        if written == 0 {
            return Err(MotionError::Engine {
                code: None,
                message: "the render reported success but wrote no frames".into(),
                path: None,
                help: Some(
                    "the bake directory is left as it is; baking into a fresh one answers whether the sidecar is at fault"
                        .into(),
                ),
            });
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
            cached: false,
        };
        let manifest = serde_json::to_string_pretty(&baked).unwrap_or_default();
        std::fs::write(dir.join("manifest.json"), manifest).map_err(|source| MotionError::Io {
            doing: "writing the bake manifest".into(),
            path: dir.join("manifest.json"),
            source,
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
        // One scratch directory a call, so two frames asked for one comp do
        // not write the same file over each other.
        static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let scratch = std::env::temp_dir().join(format!(
            "concat-motion-{}-{}-{}",
            comp.hash(),
            std::process::id(),
            CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&scratch).map_err(|source| MotionError::Io {
            doing: "creating".into(),
            path: scratch.clone(),
            source,
        })?;
        let request = BakeRequest {
            width,
            source_dir: source_dir.to_path_buf(),
            ..BakeRequest::default()
        };
        self.stage(comp, source_dir, &request, &scratch)?;
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

/// The staged path an asset gets inside a bake directory: the id is kept in
/// the name, so two assets that merely share a file name cannot overwrite
/// one another - the same rule the sequence builder stages by.
fn staged_path(id: &str, asset: &Asset) -> String {
    let name = Path::new(&asset.src)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "asset".to_string());
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("assets/{safe}.{name}")
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

/// The clip's transform, with animated properties as tracks. What cannot
/// be expressed in a geneva timeline is reported through `warnings`, like
/// every other thing the timeline leaves out.
fn effective_transform(comp: &Comp, node: &Node, warnings: &mut Vec<String>) -> Option<Json> {
    let base = &node.transform;
    let mut map = serde_json::Map::new();

    let position = node
        .tracks
        .get("position")
        .and_then(|track| keyframes(track, Kind::Point, span_of(comp, node), warnings))
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
        .and_then(|track| keyframes(track, Kind::Number, span_of(comp, node), warnings))
        .unwrap_or_else(|| json!(base.scale));
    if scale != json!(1.0) {
        map.insert("scale".into(), scale);
    }
    let rotation = node
        .tracks
        .get("rotation")
        .and_then(|track| keyframes(track, Kind::Number, span_of(comp, node), warnings))
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
        // No `pixel_format`: the engine's schema does not carry one, and
        // the PNG sequence is the alpha by being a PNG sequence.
        assert!(
            document["output"]["encode"]["video"]
                .get("pixel_format")
                .is_none()
        );
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

    /// The mapper is the whole contract between a comp and the engine: a
    /// preset that needs something geneva does not draw would bake with a
    /// hole in it and no one would notice until the cut screen.
    #[test]
    fn every_preset_maps_onto_a_timeline_with_nothing_skipped() {
        for preset in crate::presets::ALL {
            let comp = crate::presets::build(preset.id).expect("a preset");
            let (document, warnings) =
                Geneva::at("geneva").timeline(&comp, &BakeRequest::default());
            assert!(warnings.is_empty(), "{}: {warnings:?}", preset.id);
            assert_eq!(
                document["layers"].as_array().map(|l| l.len()),
                Some(1),
                "{}: one layer per preset",
                preset.id
            );
        }
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

    #[test]
    fn staged_paths_cannot_collide_on_a_shared_file_name() {
        let a = Asset {
            src: "img/logo.png".into(),
            kind: None,
        };
        let b = Asset {
            src: "other/logo.png".into(),
            kind: None,
        };
        // Two assets that merely share a file name stage apart: the id is
        // kept, so one never renders over the other.
        assert_ne!(staged_path("left", &a), staged_path("right", &b));
    }

    #[test]
    fn a_different_major_is_a_mismatch_and_a_newer_minor_is_not() {
        assert!(major_mismatch("2.0.0").is_some());
        assert!(major_mismatch("").is_some());
        assert!(major_mismatch("1.6.0").is_none());
        assert!(major_mismatch("1.99.9").is_none());
    }

    #[test]
    fn a_matching_manifest_short_circuits_and_every_mismatch_misses() {
        // A cache hit must answer without the sidecar: no binary, no
        // staging, no render - only the manifest and the frames it
        // promises. Every mismatch (engine version, size, missing frames,
        // a ranged request) is a miss that re-renders.
        let comp = crate::presets::build("funcut.chapter.chip").expect("a preset");
        let dir = std::env::temp_dir().join(format!(
            "concat-motion-cache-{}-{}",
            std::process::id(),
            comp.hash()
        ));
        let frames = dir.join("frames");
        std::fs::create_dir_all(&frames).expect("the frames dir");
        std::fs::write(frames.join("0001.png"), b"not really a png").expect("a frame");
        let manifest = Baked {
            dir: frames.clone(),
            frames: 1,
            width: comp.width,
            height: comp.height,
            fps: comp.fps,
            hash: comp.hash(),
            engine: EngineKind::Geneva,
            engine_version: "1.6.0".to_owned(),
            alpha: true,
            cached: false,
        };
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string(&manifest).expect("the manifest serialises"),
        )
        .expect("the manifest is written");

        let request = BakeRequest {
            alpha: true,
            source_dir: ".".into(),
            ..BakeRequest::default()
        };
        let hit = bake_cache_hit(&dir, &comp, &request, "1.6.0").expect("a cache hit");
        assert!(hit.cached);
        assert_eq!(hit.frames, 1);
        // `cached` is never persisted: the manifest on disk still reads
        // as a description of the bake it was written for.
        let on_disk: Baked =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                .unwrap();
        assert!(!on_disk.cached);

        assert!(
            bake_cache_hit(&dir, &comp, &request, "1.7.0").is_none(),
            "an engine change must miss"
        );
        let narrow = BakeRequest {
            width: Some(640),
            height: Some(360),
            alpha: true,
            source_dir: ".".into(),
            ..BakeRequest::default()
        };
        assert!(
            bake_cache_hit(&dir, &comp, &narrow, "1.6.0").is_none(),
            "a size change must miss"
        );
        let ranged = BakeRequest {
            frames: Some((1, 1)),
            alpha: true,
            source_dir: ".".into(),
            ..BakeRequest::default()
        };
        assert!(
            bake_cache_hit(&dir, &comp, &ranged, "1.6.0").is_none(),
            "a ranged bake is never a cache hit"
        );
        std::fs::remove_file(frames.join("0001.png")).expect("the frame is removed");
        assert!(
            bake_cache_hit(&dir, &comp, &request, "1.6.0").is_none(),
            "a cache that lost its frames must miss"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The guard: per-call limits, the circuit breaker, and the
    // diagnostics parsing. Every test here drives a stub executable
    // written into a tempdir, so nothing needs the real sidecar and
    // nothing depends on the machine it runs on. Unix only, because a
    // stub executable is a shell script with its bit set.
    #[cfg(unix)]
    mod guard {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use tempfile::TempDir;

        /// The breaker is process-wide, and the test runner runs these
        /// as threads in one process: the lock makes one test's
        /// failures nobody else's, which is what a tempdir does for
        /// files.
        use super::BREAKER_TEST_LOCK as ONE_AT_A_TIME;

        /// A stub executable at `dir/name`, run under tight limits, and
        /// the breaker reset so one test's failures are not the next
        /// test's open circuit.
        fn stub(dir: &TempDir, name: &str, body: &str) -> Geneva {
            let path = dir.path().join(name);
            std::fs::write(&path, body).expect("the stub is written");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("the stub is executable");
            reset_breaker();
            Geneva::at(&path).with_limits(CallLimits {
                plan: Duration::from_millis(400),
                render: Duration::from_millis(400),
            })
        }

        fn reset_breaker() {
            if let Ok(mut failures) = BREAKER.lock() {
                failures.clear();
            }
        }

        #[test]
        fn a_call_that_outlives_its_limit_is_killed_and_named() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = TempDir::new().unwrap();
            let geneva = stub(&dir, "sleepy", "#!/bin/sh\nexec sleep 30\n");
            let started = Instant::now();
            let error = geneva
                .run_in(
                    dir.path(),
                    &["--format".into(), "json".into(), "render".into()],
                )
                .expect_err("a sleeper is refused");
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the call returned in {:?}, not after the sleeper finished",
                started.elapsed()
            );
            match error {
                MotionError::TimedOut { what, limit } => {
                    assert_eq!(what, "rendering");
                    assert!((limit - 0.4).abs() < 0.05, "{limit}");
                }
                other => panic!("a timeout is a timeout, not {other:?}"),
            }
        }

        #[test]
        fn three_failures_open_the_breaker_and_the_fourth_is_refused_fast() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = TempDir::new().unwrap();
            // A sidecar that answers with nothing parseable and exits
            // badly, three times, then a fourth call that must not reach
            // it at all.
            let geneva = stub(&dir, "broken", "#!/bin/sh\necho 'garbage'\nexit 1\n");
            for _ in 0..3 {
                assert!(geneva.run_in(dir.path(), &["--format".into()]).is_err());
            }
            assert_eq!(geneva.breaker(), Breaker::Open { failures: 3 });
            let started = Instant::now();
            let error = geneva
                .run_in(dir.path(), &["--format".into()])
                .expect_err("the breaker refuses");
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "the refusal is immediate, not another call: {:?}",
                started.elapsed()
            );
            match error {
                MotionError::CircuitOpen { failures, window } => {
                    assert_eq!(failures, 3);
                    assert!((window - 60.0).abs() < 0.001);
                }
                other => panic!("an open breaker is an open breaker, not {other:?}"),
            }
        }

        #[test]
        fn a_success_closes_the_breaker() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = TempDir::new().unwrap();
            // Two failures, then a call that succeeds: the breaker
            // forgets them, and the next failure is a first failure
            // again.
            let geneva = stub(
                &dir,
                "moody",
                "#!/bin/sh\nif [ -f mood ]; then echo '{\"ok\":true,\"diagnostics\":[],\"summary\":{}}'; exit 0; fi\necho 'nope'; exit 1\n",
            );
            let mood = dir.path().join("mood");
            for _ in 0..2 {
                let _ = std::fs::remove_file(&mood);
                let _ = geneva.run_in(dir.path(), &[]);
            }
            assert_eq!(
                geneva.breaker(),
                Breaker::Closed,
                "two failures do not trip it"
            );
            std::fs::write(&mood, "good").unwrap();
            geneva
                .run_in(dir.path(), &[])
                .expect("the third call succeeds");
            assert_eq!(geneva.breaker(), Breaker::Closed);
            let _ = std::fs::remove_file(&mood);
            let _ = geneva.run_in(dir.path(), &[]);
            assert_eq!(
                geneva.breaker(),
                Breaker::Closed,
                "a success cleared the two failures, so one more is not three"
            );
        }

        #[test]
        fn a_reported_diagnostic_arrives_whole_with_its_fix() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = TempDir::new().unwrap();
            let geneva = stub(
                &dir,
                "fussy",
                concat!(
                    "#!/bin/sh\n",
                    "cat <<'EOF'\n",
                    "{\"ok\":false,\"diagnostics\":[{\"severity\":\"error\",\"code\":\"E302\",",
                    "\"path\":\"/layers/1/clips/1/start\",",
                    "\"message\":\"clip 1 of overlay starts at 1s, before clip 0 ends at 2s\",",
                    "\"value\":\"1s\",",
                    "\"help\":\"start it at 2s or later\"}],\"summary\":{}}\n",
                    "EOF\n",
                ),
            );
            let error = geneva
                .run_in(dir.path(), &["--format".into()])
                .expect_err("ok:false is a refusal");
            match error {
                MotionError::Engine {
                    ref code,
                    ref message,
                    ref path,
                    ref help,
                } => {
                    assert_eq!(code.as_deref(), Some("E302"));
                    assert!(message.contains("clip 1 of overlay"), "{message}");
                    assert_eq!(path.as_deref(), Some("/layers/1/clips/1/start"));
                    // The engine's own help wins over the table's.
                    assert!(help.as_deref().unwrap_or_default().contains("2s or later"));
                }
                other => panic!("a diagnostic is an Engine refusal, not {other:?}"),
            }
            let text = error.to_string();
            assert!(text.contains("E302"), "{text}");
            assert!(text.contains("/layers/1/clips/1/start"), "{text}");
        }

        #[test]
        fn a_known_code_without_its_own_help_gets_the_funcut_fix() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = TempDir::new().unwrap();
            let geneva = stub(
                &dir,
                "terse",
                concat!(
                    "#!/bin/sh\n",
                    "cat <<'EOF'\n",
                    "{\"ok\":false,\"diagnostics\":[{\"severity\":\"error\",\"code\":\"W405\",",
                    "\"message\":\"the family Archivo is not on this machine\"}],\"summary\":{}}\n",
                    "EOF\n",
                ),
            );
            let error = geneva
                .run_in(dir.path(), &["--format".into()])
                .expect_err("refused");
            let help = match error {
                MotionError::Engine { help, .. } => help,
                other => panic!("{other:?}"),
            };
            let help = help.expect("the table answers for W405");
            assert!(help.contains("font"), "the fix is about fonts: {help}");
            assert!(help.contains("assets"), "the fix names where: {help}");
        }

        #[test]
        fn an_unknown_code_is_explained_by_the_sidecar_itself() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = TempDir::new().unwrap();
            let geneva = stub(
                &dir,
                "explainer",
                concat!(
                    "#!/bin/sh\n",
                    "if [ \"$1\" = \"explain\" ]; then\n",
                    "  echo 'error[E999]: a made-up code'\n",
                    "  echo 'E999 means the frobnicator is misaligned.'\n",
                    "  exit 0\n",
                    "fi\n",
                    "cat <<'EOF'\n",
                    "{\"ok\":false,\"diagnostics\":[{\"severity\":\"error\",\"code\":\"E999\",",
                    "\"message\":\"the frobnicator\"}],\"summary\":{}}\n",
                    "EOF\n",
                ),
            );
            let error = geneva
                .run_in(dir.path(), &["--format".into()])
                .expect_err("refused");
            match error {
                MotionError::Engine { code, help, .. } => {
                    assert_eq!(code.as_deref(), Some("E999"));
                    assert_eq!(
                        help.as_deref(),
                        Some("E999 means the frobnicator is misaligned.")
                    );
                }
                other => panic!("{other:?}"),
            }
        }

        #[test]
        fn the_fix_table_only_names_codes_the_sidecar_defines() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            // The table's codes come from `geneva guide errors`; a code
            // the sidecar has never defined is a typo in the table, and
            // an unknown one falls through to explain(). The shape of
            // the check: every tabled code parses as E/W/N + digits.
            for code in [
                "E100", "E101", "E102", "E103", "E104", "E105", "E110", "E200", "E201", "E202",
                "E203", "E206", "E207", "E300", "E301", "E302", "E303", "E305", "E306", "E401",
                "E402", "E440", "E441", "E442", "E443", "E450", "E451", "E452", "E453", "E501",
                "E502", "E503", "E504", "W405", "W407", "W440",
            ] {
                assert!(fix_for(code).is_some(), "{code} is tabled");
                let digits = &code[1..];
                assert!(!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()));
            }
            assert!(fix_for("E999").is_none());
            assert!(fix_for("not-a-code").is_none());
        }

        #[test]
        fn a_cap_that_tightens_the_render_limit_tightens_the_plan_less() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let limits = CallLimits::capped_at(Duration::from_secs(30));
            assert_eq!(limits.render, Duration::from_secs(30));
            assert_eq!(limits.plan, Duration::from_secs(30));
            let loose = CallLimits::capped_at(Duration::from_secs(600));
            assert_eq!(loose.render, Duration::from_secs(600));
            assert_eq!(loose.plan, Duration::from_secs(60));
        }

        #[test]
        fn the_version_is_read_under_the_same_limits() {
            let _serial = ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = TempDir::new().unwrap();
            let geneva = stub(&dir, "versioned", "#!/bin/sh\necho 'geneva 1.6.0'\n");
            assert_eq!(geneva.version().expect("a version"), "1.6.0");
            let slow = stub(&dir, "slow-version", "#!/bin/sh\nexec sleep 30\n");
            assert!(matches!(
                slow.version().expect_err("a sleeper has no version"),
                MotionError::TimedOut { .. }
            ));
        }
    }
}
