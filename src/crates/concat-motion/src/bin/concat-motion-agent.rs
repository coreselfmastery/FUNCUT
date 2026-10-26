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
//! # the same comp as one file the timeline plays: bake, then encode
//! agent bake edited.json cache/lower-third --encode comp.mp4
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
        "captions" => captions(&args),
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
            "bake": "agent bake <comp.json> <dir> [--width N] [--height N] [--fps N] [--opaque] [--encode out.mp4|webm|mov] [--crf N]",
            "reel": "agent reel <storyboard.json> [-o reel.mp4] [--cache DIR] [--frame SECONDS] [--plan]",
            "captions": "agent captions <words.json> [-o storyboard.json] [--words-per-line N] [--max-line S] [--preset ID] [--plate IMG] [--audio WAV|MP3]"
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
            {"op": "set-track-points", "node": "card", "keys": [[0.0, 240, 540, null], [0.7, 960, 540, {"kind": "spring", "stiffness": 170, "damping": 26, "mass": 1}]]},
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

/// `agent captions <words.json> [-o storyboard.json] [...]`
///
/// A transcript in, a storyboard out: the words are broken into caption lines
/// (sentence ends, silences, word count and length all break a line), each line
/// becomes one caption comp, and the result is the same storyboard `reel`
/// renders. Two commands between a transcript and a finished caption video.
///
/// The transcript is a list of words with times, in any of the shapes a
/// recogniser gives: `[["Make", 0.2, 0.55], ...]`, a `{"words": [...]}`
/// wrapper, or objects `{"text": "Make", "start": 0.2, "end": 0.55}`.
fn captions(args: &[String]) -> Result<serde_json::Value, String> {
    let path = args.first().ok_or("captions needs a transcript file")?;
    let out = flag(args, "-o").map(PathBuf::from);
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    let words = read_words(&document).map_err(|error| format!("{path}: {error}"))?;

    let mut caps = concat_motion::captions::Captions::default();
    if let Some(count) = flag(args, "--words-per-line").and_then(|value| value.parse().ok()) {
        caps.words_per_line = count;
    }
    if let Some(line) = flag(args, "--max-line").and_then(|value| value.parse().ok()) {
        caps.max_line = line;
    }
    if let Some(id) = flag(args, "--preset") {
        caps.comp = id;
    }
    let plate = flag(args, "--plate");
    let audio = flag(args, "--audio");

    let lines = concat_motion::captions::lay_out(&words, &caps);
    let mut sequence = concat_motion::captions::sequence(&words, &caps, 1920, 1080, 30.0);
    if let Some(plate) = &plate {
        sequence.asset(
            "plate",
            Asset {
                src: plate.clone(),
                kind: None,
            },
        );
        for scene in &mut sequence.scenes {
            scene.plate = Some(concat_motion::sequence::Plate {
                asset: "plate".into(),
                zoom: 0.04,
            });
        }
    }
    if let Some(audio) = &audio {
        sequence.asset(
            "score",
            Asset {
                src: audio.clone(),
                kind: None,
            },
        );
        sequence.bed(concat_motion::sequence::AudioBed {
            asset: "score".into(),
            gain_db: -3.0,
            fade_in: 0.3,
            fade_out: 1.0,
        });
    }

    // The storyboard the `reel` verb eats, written from the same sequence.
    let scenes: Vec<serde_json::Value> = sequence
        .scenes
        .iter()
        .map(|scene| {
            json!({
                "preset": scene.comp.id,
                "at": scene.at,
                "hold": scene.hold,
                "plate": scene.plate.as_ref().map(|plate| plate.asset.clone()),
                "zoom": scene.plate.as_ref().map(|plate| plate.zoom),
                "ops": scene.ops,
            })
        })
        .collect();
    let mut assets = serde_json::Map::new();
    for (id, asset) in &sequence.assets {
        assets.insert(id.clone(), json!(asset.src));
    }
    let storyboard = json!({
        "width": sequence.width,
        "height": sequence.height,
        "fps": sequence.fps,
        "assets": assets,
        "scenes": scenes,
        "audio": sequence.audio.iter().map(|bed| json!({
            "asset": bed.asset, "gain_db": bed.gain_db,
            "fade_in": bed.fade_in, "fade_out": bed.fade_out
        })).collect::<Vec<_>>(),
    });

    if let Some(path) = &out {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(&storyboard).map_err(|error| error.to_string())?;
        std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }

    Ok(json!({
        "ok": true,
        "words": words.len(),
        "lines": lines.len(),
        "duration": sequence.duration(),
        "written": out.map(|path| path.display().to_string()),
        "text": lines.iter().map(|line| line.text.clone()).collect::<Vec<_>>(),
        "storyboard": storyboard,
    }))
}

/// Read a transcript in any of the shapes a recogniser gives.
fn read_words(document: &serde_json::Value) -> Result<Vec<concat_motion::captions::Word>, String> {
    use concat_motion::captions::Word;
    let list = match document {
        serde_json::Value::Array(list) => list,
        serde_json::Value::Object(map) => match map.get("words") {
            Some(serde_json::Value::Array(list)) => list,
            _ => return Err("an object transcript needs a `words` array".into()),
        },
        _ => return Err("a transcript is an array of words or {\"words\": [...]}".into()),
    };
    let mut words = Vec::with_capacity(list.len());
    for (index, entry) in list.iter().enumerate() {
        let word = match entry {
            serde_json::Value::Array(parts) => {
                let text = parts
                    .first()
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| format!("word {index}: the first field is its text"))?;
                let start = parts
                    .get(1)
                    .and_then(|value| value.as_f64())
                    .ok_or_else(|| format!("word {index}: the second field is its start time"))?;
                let end = parts
                    .get(2)
                    .and_then(|value| value.as_f64())
                    .unwrap_or(start + 0.25);
                Word::new(text, start, end)
            }
            serde_json::Value::Object(fields) => {
                let text = fields
                    .get("text")
                    .or_else(|| fields.get("word"))
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| format!("word {index}: it needs a `text`"))?;
                let start = fields
                    .get("start")
                    .and_then(|value| value.as_f64())
                    .ok_or_else(|| format!("word {index}: it needs a `start`"))?;
                let end = fields
                    .get("end")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(start + 0.25);
                Word::new(text, start, end)
            }
            _ => return Err(format!("word {index} is neither a triple nor an object")),
        };
        words.push(word);
    }
    Ok(words)
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
    // The comp's assets resolve from its own folder, like a bake's do.
    let source_dir = Path::new(comp_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let produced = geneva
        .frame(&comp, at, width, &source_dir)
        .map_err(|error| error.to_string())?;
    match out {
        Some(path) => {
            std::fs::copy(&produced, &path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            // The scratch this frame was drawn in is the call's own; the
            // file the caller asked for is copied out of it and the rest
            // is not kept.
            let _ = produced
                .parent()
                .and_then(|dir| std::fs::remove_dir_all(dir).ok());
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
    // `--encode out.mp4`: the timeline the bake staged is rendered once
    // more with its encode instruction switched to a video codec, straight
    // to a file the editor's timeline plays like any other footage. The
    // codec is the extension's: `.webm` asks Geneva for VP9, `.mov` for
    // ProRes 4444, and anything else for the H.264 every player reads.
    let mut notes: Vec<String> = Vec::new();
    let file = match flag(args, "--encode") {
        Some(target) => {
            let crf: u8 = flag(args, "--crf")
                .and_then(|value| value.parse().ok())
                .unwrap_or(request.crf);
            let out = PathBuf::from(target);
            let out = if out.is_absolute() {
                out
            } else {
                std::env::current_dir().unwrap_or_default().join(out)
            };
            if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("{}: {error}", parent.display()))?;
            }
            let (container, codec, profile) = match out.extension().and_then(|ext| ext.to_str()) {
                Some(ext) if ext.eq_ignore_ascii_case("webm") => ("webm", "vp9", None),
                Some(ext) if ext.eq_ignore_ascii_case("mov") => ("mov", "prores", Some("4444")),
                _ => ("mp4", "h264", None),
            };
            let timeline = dir.join("timeline.json");
            let text = std::fs::read_to_string(&timeline)
                .map_err(|error| format!("{}: {error}", timeline.display()))?;
            let mut document: serde_json::Value = serde_json::from_str(&text)
                .map_err(|error| format!("{}: {error}", timeline.display()))?;
            let mut video = serde_json::Map::new();
            video.insert("codec".to_string(), json!(codec));
            match profile {
                // ProRes quality is the profile; a crf is not a thing there.
                Some(profile) => {
                    video.insert("profile".to_string(), json!(profile));
                }
                None => {
                    video.insert("crf".to_string(), json!(crf));
                }
            }
            document["output"]["encode"] = json!({
                "container": container,
                "video": video,
            });
            std::fs::write(
                &timeline,
                serde_json::to_string_pretty(&document).map_err(|error| {
                    format!("the staged timeline could not be written: {error}")
                })?,
            )
            .map_err(|error| format!("{}: {error}", timeline.display()))?;
            let render: Vec<String> = vec![
                "--format".into(),
                "json".into(),
                "render".into(),
                "timeline.json".into(),
                "-o".into(),
                out.display().to_string(),
            ];
            geneva
                .run_in(&dir, &render)
                .map_err(|error| error.to_string())?;
            // A render that says yes and leaves nothing is a bug in the
            // chain, not a file: the reply says so instead of a path.
            if !out.is_file() {
                return Err(format!(
                    "geneva reported success but {} was not written",
                    out.display()
                ));
            }
            // The honest note every overlay comp earns: the sidecar's video
            // codecs are 4:2:0 or 4:4:4 with no alpha channel, and only the
            // PNG sequence keeps what a transparent background meant.
            if comp.background.a < 1.0 {
                notes.push(format!(
                    "this comp has a transparent background; {codec} cannot carry alpha, so the encoded file is opaque where the comp was not - the PNG sequence in the bake keeps it"
                ));
            }
            Some(out.display().to_string())
        }
        None => None,
    };
    let mut reply = json!({
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
    });
    if let Some(file) = file {
        reply["file"] = json!(file);
    }
    if !notes.is_empty() {
        reply["notes"] = json!(notes);
    }
    Ok(reply)
}

/// The value after `name`, when it is present.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1))
        .cloned()
}
