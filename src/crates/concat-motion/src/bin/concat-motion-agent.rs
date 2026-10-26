// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The agent's door into motion comps: one binary, JSON in, JSON out.
//!
//! This is what an MCP tool, a Forge run or a shell script calls. Every verb
//! prints exactly one JSON document and exits non-zero on failure, so a model
//! can pipe it and parse it without guessing.
//!
//! ```sh
//! # what is in the library
//! agent list
//! # what is in a comp: nodes, tracks, slots, whether it needs a markup engine
//! agent inspect comps/funcut.lower-third.glass.json
//! # change it: a batch of operations, validated before anything is written
//! agent edit comps/lower-third.json ops.json -o edited.json
//! # see one frame (tens of milliseconds) and the whole comp
//! agent frame edited.json 1.5 -o still.png
//! agent bake edited.json cache/lower-third
//! # a whole reel: several comps, plates, a curtain, a bed of sound
//! agent reel storyboard.json -o reel.mp4
//! ```

use std::path::{Path, PathBuf};

use concat_motion::authoring::{CompSummary, apply_json};
use concat_motion::comp::{Asset, Comp, Rgba};
use concat_motion::engine::{BakeRequest, CompEngine, EnginePreference};
use concat_motion::geneva::Geneva;
use concat_motion::presets;
use concat_motion::sequence::Curtain;
use serde_json::json;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print(&usage());
        std::process::exit(2);
    }
    let verb = args.remove(0);
    let result = match verb.as_str() {
        "list" => list(),
        "inspect" => inspect(&args),
        "edit" => edit(&args),
        "frame" => frame(&args),
        "bake" => bake(&args),
        "reel" => reel(&args),
        "help" | "--help" | "-h" => Ok(usage()),
        other => Err(format!("no verb called {other:?}")),
    };
    match result {
        Ok(value) => print(&value),
        Err(message) => {
            print(&json!({ "ok": false, "error": message }));
            std::process::exit(1);
        }
    }
}

fn print(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into())
    );
}

fn usage() -> serde_json::Value {
    json!({
        "ok": true,
        "usage": {
            "list": "agent list - every preset in the motion library",
            "inspect": "agent inspect <comp.json>",
            "edit": "agent edit <comp.json> <ops.json|-> [-o out.json]",
            "frame": "agent frame <comp.json> <seconds> [-o frame.png] [--width N]",
            "bake": "agent bake <comp.json> <dir> [--width N] [--height N] [--fps N] [--opaque]",
            "reel": "agent reel <storyboard.json> [-o reel.mp4] [--cache DIR] [--frame SECONDS] [--plan]"
        },
        "ops": [
            {"op": "set-slot", "slot": "Name", "value": "Grace Hopper"},
            {"op": "set-text", "node": "caption", "text": "Make it move."},
            {"op": "set-words", "node": "caption",
             "words": [["Make", 0.0, 0.4], ["it", 0.4, 0.8], ["move.", 0.8, null]]},
            {"op": "move", "node": "card", "start": 3.5},
            {"op": "retime", "node": "card", "duration": 2.5},
            {"op": "set-track", "node": "card", "property": "scale",
             "keys": [[0.0, 0.9, {"kind": "spring", "stiffness": 260, "damping": 15, "mass": 1}], [0.6, 1.0, null]]},
            {"op": "fade-in", "node": "card", "seconds": 0.8, "curve": {"kind": "steps", "count": 6, "position": "jump-end"}},
            {"op": "slide-to", "node": "card", "x": 960, "y": 540, "seconds": 0.7},
            {"op": "pop", "node": "card", "from": 0.9, "seconds": 0.5},
            {"op": "add-group", "id": "row", "x": 960, "y": 540},
            {"op": "add-rect", "id": "bar", "width": 600, "height": 6, "fill": "#22d3c5", "radius": 3, "x": 960, "y": 700},
            {"op": "add-text", "id": "kicker", "text": "CHAPTER 02", "size": 28, "weight": 700, "color": "#22d3c5", "x": 240, "y": 140},
            {"op": "set-blend", "node": "glow", "blend": "screen"},
            {"op": "set-opacity", "node": "card", "opacity": 0.9},
            {"op": "set-duration", "seconds": 6.0},
            {"op": "set-background", "color": "#06070bff"},
            {"op": "delete", "node": "kicker"}
        ]
    })
}

/// A storyboard: several comps, in order, as one render.
#[derive(serde::Deserialize)]
struct Storyboard {
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default = "default_fps")]
    fps: f64,
    /// Where asset paths resolve from; the storyboard's own folder by default.
    #[serde(default)]
    source_dir: Option<PathBuf>,
    #[serde(default)]
    assets: std::collections::BTreeMap<String, String>,
    scenes: Vec<SceneCard>,
    #[serde(default)]
    audio: Vec<AudioCard>,
    #[serde(default)]
    curtain: Option<CurtainCard>,
}

#[derive(serde::Deserialize)]
struct SceneCard {
    /// A preset id from `agent list`.
    #[serde(default)]
    preset: Option<String>,
    /// Or a comp document, relative to the storyboard.
    #[serde(default)]
    comp: Option<String>,
    /// Where it starts; omitted means after the scene before it.
    #[serde(default)]
    at: Option<f64>,
    /// How long it holds; omitted means the comp's own length.
    #[serde(default)]
    hold: Option<f64>,
    /// A still underneath it.
    #[serde(default)]
    plate: Option<String>,
    /// How much the plate grows over the scene.
    #[serde(default)]
    zoom: Option<f64>,
    /// Operations applied to the comp before it is placed.
    #[serde(default)]
    ops: Vec<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct AudioCard {
    asset: String,
    #[serde(default)]
    gain_db: f64,
    #[serde(default)]
    fade_in: Option<f64>,
    #[serde(default)]
    fade_out: Option<f64>,
}

#[derive(serde::Deserialize)]
struct CurtainCard {
    #[serde(default = "default_curtain")]
    color: String,
    #[serde(default = "default_open")]
    open: f64,
    #[serde(default = "default_close")]
    close: f64,
    #[serde(default = "default_dip")]
    dip: f64,
    #[serde(default = "default_depth")]
    depth: f64,
}

fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_fps() -> f64 {
    30.0
}
fn default_curtain() -> String {
    "#05060aff".into()
}
fn default_open() -> f64 {
    0.6
}
fn default_close() -> f64 {
    0.8
}
fn default_dip() -> f64 {
    0.2
}
fn default_depth() -> f64 {
    1.0
}

/// `agent reel <storyboard.json> [-o out.mp4] [--cache DIR] [--frame SECONDS] [--plan]`
///
/// The storyboard is the contract:
///
/// ```json
/// { "width": 1920, "height": 1080, "fps": 30,
///   "assets": { "plate": "assets/plate.jpg", "score": "assets/score.wav" },
///   "scenes": [
///     { "preset": "funcut.chapter.chip", "hold": 3.0, "plate": "plate", "zoom": 0.04,
///       "ops": [ { "op": "set-slot", "slot": "Chapter", "value": "01 / INTRO" } ] },
///     { "comp": "comps/mine.json", "at": 3.0 }
///   ],
///   "audio": [ { "asset": "score", "gain_db": -2.0 } ],
///   "curtain": { "color": "#05060aff", "open": 0.6, "close": 0.8, "dip": 0.2, "depth": 1.0 } }
/// ```
///
/// Every scene is resolved and every operation applied before anything renders, so a storyboard that
/// does not make sense comes back as one JSON error naming the scene - not as a broken file.
fn reel(args: &[String]) -> Result<serde_json::Value, String> {
    let script_path = args.first().ok_or("reel needs a storyboard file")?;
    let out = flag(args, "-o").map(PathBuf::from);
    let cache = flag(args, "--cache")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cache/reel"));
    let at_frame: Option<f64> = flag(args, "--frame").and_then(|value| value.parse().ok());
    let plan_only = args.iter().any(|arg| arg == "--plan");

    let script = Path::new(script_path);
    let text =
        std::fs::read_to_string(script).map_err(|error| format!("{script_path}: {error}"))?;
    let board: Storyboard =
        serde_json::from_str(&text).map_err(|error| format!("{script_path}: {error}"))?;
    let root = board
        .source_dir
        .clone()
        .or_else(|| script.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut sequence = concat_motion::sequence::Sequence::new(board.width, board.height, board.fps);
    sequence.source_dir = root.clone();
    for (id, src) in &board.assets {
        sequence.asset(
            id.clone(),
            Asset {
                src: src.clone(),
                kind: None,
            },
        );
    }
    if let Some(curtain) = &board.curtain {
        sequence.curtain = Some(Curtain {
            color: Rgba::hex(&curtain.color),
            open: curtain.open,
            close: curtain.close,
            dip: curtain.dip,
            depth: curtain.depth,
        });
    }
    for card in &board.scenes {
        let comp = match (&card.preset, &card.comp) {
            (Some(id), _) => {
                presets::build(id).ok_or_else(|| format!("no preset called {id:?}"))?
            }
            (None, Some(file)) => load(&root.join(file).display().to_string())?,
            (None, None) => return Err("a scene needs a `preset` or a `comp`".into()),
        };
        let ops = card
            .ops
            .iter()
            .map(|op| serde_json::from_value(op.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("a scene operation is not one: {error}"))?;
        let mut scene = concat_motion::sequence::Scene::new(comp).with_ops(ops);
        if let Some(at) = card.at {
            scene = scene.starting_at(at);
        }
        if let Some(hold) = card.hold {
            scene = scene.holding(hold);
        }
        if let Some(plate) = &card.plate {
            scene = scene.over(plate.clone(), card.zoom.unwrap_or(0.0));
        }
        sequence.push(scene);
    }
    for bed in &board.audio {
        let mut sound = concat_motion::sequence::AudioBed::new(bed.asset.clone()).at(bed.gain_db);
        if let Some(fade) = bed.fade_in {
            sound.fade_in = fade;
        }
        if let Some(fade) = bed.fade_out {
            sound.fade_out = fade;
        }
        sequence.bed(sound);
    }

    let geneva = engine()?;
    let planned =
        concat_motion::sequence::plan(&geneva, &sequence).map_err(|error| error.to_string())?;
    let summary = json!({
        "ok": true,
        "scenes": planned.scenes,
        "duration": planned.duration,
        "width": board.width,
        "height": board.height,
        "fps": board.fps,
        "layers": planned.document["layers"].as_array().map(Vec::len).unwrap_or(0),
        "assets": planned.files.len(),
        "warnings": planned.warnings,
    });
    if plan_only {
        let mut value = summary;
        value["timeline"] = planned.document.clone();
        return Ok(value);
    }
    if let Some(at) = at_frame {
        let path = out.unwrap_or_else(|| PathBuf::from("frame.jpg"));
        let report = planned
            .frame(&geneva, &cache, at, &path)
            .map_err(|error| error.to_string())?;
        return Ok(
            json!({ "ok": true, "at": at, "frame": path.display().to_string(),
                          "report": report, "plan": summary }),
        );
    }

    let path = out.unwrap_or_else(|| PathBuf::from("reel.mp4"));
    let started = std::time::Instant::now();
    let report = planned
        .render(&geneva, &cache, &path)
        .map_err(|error| error.to_string())?;
    let seconds = started.elapsed().as_secs_f64();
    let mut value = summary;
    value["written"] = json!(path.display().to_string());
    value["seconds"] = json!(seconds);
    value["frames_per_second"] = json!(planned.duration * board.fps / seconds.max(1e-9));
    value["report"] = report;
    Ok(value)
}

fn list() -> Result<serde_json::Value, String> {
    let presets: Vec<serde_json::Value> = presets::ALL
        .iter()
        .map(|preset| {
            json!({
                "id": preset.id,
                "name": preset.name,
                "category": preset.category.as_str(),
                "engine": preset.engine.as_str(),
            })
        })
        .collect();
    Ok(json!({ "ok": true, "count": presets.len(), "presets": presets }))
}

fn load(path: &str) -> Result<Comp, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let comp: Comp = serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    Ok(comp)
}

fn inspect(args: &[String]) -> Result<serde_json::Value, String> {
    let comp = load(args.first().ok_or("inspect needs a comp file")?)?;
    Ok(json!({
        "ok": true,
        "comp": CompSummary::of(&comp),
        "hash": format!("{:016x}", comp.hash()),
        "problems": comp.validate().iter().map(|problem| json!({
            "path": problem.path, "message": problem.message, "help": problem.help
        })).collect::<Vec<_>>(),
    }))
}

fn edit(args: &[String]) -> Result<serde_json::Value, String> {
    let comp_path = args.first().ok_or("edit needs a comp file")?;
    let ops_path = args
        .get(1)
        .ok_or("edit needs an operations file (or - for stdin)")?;
    let out = flag(args, "-o").map(PathBuf::from);

    let comp = load(comp_path)?;
    let batch = if ops_path == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .map_err(|error| format!("reading stdin: {error}"))?;
        serde_json::from_str(&text).map_err(|error| format!("stdin: {error}"))?
    } else {
        let text =
            std::fs::read_to_string(ops_path).map_err(|error| format!("{ops_path}: {error}"))?;
        serde_json::from_str(&text).map_err(|error| format!("{ops_path}: {error}"))?
    };

    let edited = apply_json(&comp, &batch).map_err(|error| error.to_string())?;
    if let Some(path) = &out {
        write_comp(&edited, path)?;
    }
    Ok(json!({
        "ok": true,
        "written": out.map(|path| path.display().to_string()),
        "hash": format!("{:016x}", edited.hash()),
        "comp": CompSummary::of(&edited),
    }))
}

fn write_comp(comp: &Comp, path: &Path) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(comp).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn engine() -> Result<Geneva, String> {
    let preference = EnginePreference::default();
    let geneva = Geneva::discover(preference.geneva.as_deref())
        .ok_or("no geneva binary: set CONCAT_GENEVA_BIN, put it on PATH, or install the sidecar")?;
    let probe = geneva.probe();
    if !probe.available {
        return Err(format!("geneva does not run: {}", probe.detail));
    }
    Ok(geneva)
}

fn frame(args: &[String]) -> Result<serde_json::Value, String> {
    let comp_path = args.first().ok_or("frame needs a comp file")?;
    let at: f64 = args
        .get(1)
        .ok_or("frame needs a time in seconds")?
        .parse()
        .map_err(|_| "the time is a number of seconds".to_string())?;
    let out = flag(args, "-o").map(PathBuf::from);
    let width: Option<u32> = flag(args, "--width").and_then(|value| value.parse().ok());

    let comp = load(comp_path)?;
    let geneva = engine()?;
    let produced = geneva
        .frame(&comp, at, width)
        .map_err(|error| error.to_string())?;
    match out {
        Some(path) => {
            std::fs::copy(&produced, &path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Ok(json!({ "ok": true, "at": at, "frame": path.display().to_string() }))
        }
        None => Ok(json!({ "ok": true, "at": at, "frame": produced.display().to_string() })),
    }
}

fn bake(args: &[String]) -> Result<serde_json::Value, String> {
    let comp_path = args.first().ok_or("bake needs a comp file")?;
    let dir = PathBuf::from(args.get(1).ok_or("bake needs an output directory")?);

    let comp = load(comp_path)?;
    let geneva = engine()?;
    let request = BakeRequest {
        width: flag(args, "--width").and_then(|value| value.parse().ok()),
        height: flag(args, "--height").and_then(|value| value.parse().ok()),
        fps: flag(args, "--fps").and_then(|value| value.parse().ok()),
        alpha: !args.iter().any(|arg| arg == "--opaque"),
        source_dir: Path::new(comp_path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
        ..BakeRequest::default()
    };
    let started = std::time::Instant::now();
    let baked = geneva
        .bake(&comp, &request, &dir)
        .map_err(|error| error.to_string())?;
    let seconds = started.elapsed().as_secs_f64();
    Ok(json!({
        "ok": true,
        "frames": baked.frames,
        "width": baked.width,
        "height": baked.height,
        "fps": baked.fps,
        "alpha": baked.alpha,
        "engine": baked.engine.as_str(),
        "engine_version": baked.engine_version,
        "hash": format!("{:016x}", baked.hash),
        "dir": baked.dir.display().to_string(),
        "seconds": seconds,
        "frames_per_second": baked.frames as f64 / seconds.max(1e-9),
    }))
}

/// The value after `name`, when it is present.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1))
        .cloned()
}
