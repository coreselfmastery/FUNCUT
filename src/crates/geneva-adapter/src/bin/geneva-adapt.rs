// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The adapter's front door on the command line, for the agent and the
//! curious:
//!
//! ```text
//! geneva-adapt lottie animation.json -o comp.json
//! geneva-adapt gsap comp.json timeline.json -o comp.json
//! ```
//!
//! The report is printed to stderr as JSON, the comp to stdout (or `-o`),
//! and the exit code is non-zero when the document is not the format the
//! verb reads. Notes are the conversion's conscience: they go to stderr
//! where a caller piping the comp onwards still sees them.

use std::io::Read;

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("lottie") => match lottie(&args[1..]) {
            Ok(()) => 0,
            Err(said) => {
                eprintln!("{said}");
                2
            }
        },
        Some("gsap") => match gsap(&args[1..]) {
            Ok(()) => 0,
            Err(said) => {
                eprintln!("{said}");
                2
            }
        },
        _ => {
            eprintln!(
                "usage: geneva-adapt lottie <animation.json> [-o comp.json]\n       geneva-adapt gsap <comp.json> <timeline.json> [-o comp.json]"
            );
            64
        }
    }
}

/// The lottie verb: read a bodymovin document, write the comp and print
/// the report.
fn lottie(args: &[String]) -> Result<(), String> {
    let input = args
        .first()
        .ok_or("usage: geneva-adapt lottie <animation.json> [-o comp.json]")?;
    let out = flag(args, "-o");
    let mut text = String::new();
    std::fs::File::open(input)
        .map_err(|source| format!("could not read {input:?}: {source}"))?
        .read_to_string(&mut text)
        .map_err(|source| format!("could not read {input:?}: {source}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|source| format!("{input:?} is not JSON: {source}"))?;
    let import = geneva_adapter::lottie::import(&document)
        .map_err(|error| format!("the Lottie document does not convert: {error}"))?;
    write(&import.comp, &import.notes, out)
}

/// The gsap verb: read a comp and a timeline, write the comp the timeline
/// was applied to and print the report.
fn gsap(args: &[String]) -> Result<(), String> {
    let comp_path = args
        .first()
        .ok_or("usage: geneva-adapt gsap <comp.json> <timeline.json> [-o comp.json]")?;
    let timeline_path = args
        .get(1)
        .ok_or("usage: geneva-adapt gsap <comp.json> <timeline.json> [-o comp.json]")?;
    let out = flag(args, "-o").or_else(|| Some(comp_path.clone()));
    let mut text = String::new();
    std::fs::File::open(comp_path)
        .map_err(|source| format!("could not read {comp_path:?}: {source}"))?
        .read_to_string(&mut text)
        .map_err(|source| format!("could not read {comp_path:?}: {source}"))?;
    let comp: serde_json::Value = serde_json::from_str(&text)
        .map_err(|source| format!("{comp_path:?} is not JSON: {source}"))?;
    let comp = serde_json::from_value::<concat_motion::comp::Comp>(comp)
        .map_err(|source| format!("{comp_path:?} is not a comp: {source}"))?;
    let mut text = String::new();
    std::fs::File::open(timeline_path)
        .map_err(|source| format!("could not read {timeline_path:?}: {source}"))?
        .read_to_string(&mut text)
        .map_err(|source| format!("could not read {timeline_path:?}: {source}"))?;
    let timeline: serde_json::Value = serde_json::from_str(&text)
        .map_err(|source| format!("{timeline_path:?} is not JSON: {source}"))?;
    let (made, notes) = geneva_adapter::gsap::apply(&comp, &timeline)
        .map_err(|error| format!("the timeline does not apply: {error}"))?;
    write(&made, &notes, out)
}

/// Write the comp where it belongs and print the report to stderr.
fn write(
    comp: &concat_motion::comp::Comp,
    notes: &[String],
    out: Option<String>,
) -> Result<(), String> {
    let document = serde_json::to_string_pretty(comp)
        .map_err(|source| format!("the comp does not serialise: {source}"))?;
    let report = serde_json::json!({
        "ok": true,
        "notes": notes,
        "skipped": notes.iter().filter(|n| n.starts_with("skipped: ")).count(),
        "comp": { "id": comp.id, "nodes": comp.nodes.len(), "duration": comp.duration },
    });
    eprintln!("{report}");
    match out {
        Some(path) => std::fs::write(&path, document)
            .map_err(|source| format!("could not write {path:?}: {source}")),
        None => {
            println!("{document}");
            Ok(())
        }
    }
}

/// The value after a flag, when the flag is there.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1).cloned())
}
