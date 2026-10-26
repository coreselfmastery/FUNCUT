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
//! ```

use std::path::{Path, PathBuf};

use concat_motion::authoring::{apply_json, CompSummary};
use concat_motion::comp::Comp;
use concat_motion::engine::{BakeRequest, CompEngine, EnginePreference};
use concat_motion::geneva::Geneva;
use concat_motion::presets;
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
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into()));
}

fn usage() -> serde_json::Value {
    json!({
        "ok": true,
        "usage": {
            "list": "agent list - every preset in the motion library",
            "inspect": "agent inspect <comp.json>",
            "edit": "agent edit <comp.json> <ops.json|-> [-o out.json]",
            "frame": "agent frame <comp.json> <seconds> [-o frame.png] [--width N]",
            "bake": "agent bake <comp.json> <dir> [--width N] [--height N] [--fps N] [--opaque]"
        },
        "ops": [
            {"op": "set-slot", "slot": "Name", "value": "Grace Hopper"},
            {"op": "set-text", "node": "caption", "text": "Make it move."},
            {"op": "set-words", "node": "caption",
             "words": [["Make", 0.0, 0.4], ["it", 0.4, 0.8], ["move.", 0.8, null]]},
            {"op": "move", "node": "card", "start": 3.5},
            {"op": "retime", "node": "card", "duration": 2.5},
            {"op": "set-track", "node": "card", "property": "scale",
             "keys": [[0.0, 0.9, {"kind": "spring", "spring": {"stiffness": 260, "damping": 15, "mass": 1}}], [0.6, 1.0, null]]},
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
    let ops_path = args.get(1).ok_or("edit needs an operations file (or - for stdin)")?;
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
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
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
    let produced = geneva.frame(&comp, at, width).map_err(|error| error.to_string())?;
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
    let baked = geneva.bake(&comp, &request, &dir).map_err(|error| error.to_string())?;
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
    args.iter().position(|arg| arg == name).and_then(|at| args.get(at + 1)).cloned()
}
