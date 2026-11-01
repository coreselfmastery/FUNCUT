// SPDX-License-Identifier: AGPL-3.0-or-later
// FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! One layout, three personalities: the stat-card preset rendered as
//! `calm`, as `punchy` and as `editorial`, over the same plate - the
//! style axis over the layout templates, made visible. The type carries
//! a different weight and tracking, the cards rise on a different hand
//! at a different tempo, and each scene wears its own grade.
//!
//! ```sh
//! # three stills, one per personality, at the same moment of each scene
//! cargo run -p concat-motion --example style -- --still 1.5 \
//!     --assets . --cache cache/style -o style.png
//! # the whole reel, the personalities in sequence
//! cargo run -p concat-motion --example style -- -o style.mp4 --assets . --cache cache/style
//! ```
//!
//! The stills land beside `-o` as `<stem>-calm.png`, `<stem>-punchy.png`
//! and `<stem>-editorial.png`, and the plan prints as the timeline it
//! becomes. `scripts/motion/inspect-styles.py` measures that the three
//! are three visibly different scenes rather than three names for one.

use std::path::PathBuf;

use concat_motion::comp::Asset;
use concat_motion::geneva::Geneva;
use concat_motion::presets;
use concat_motion::sequence::{Scene, Sequence};
use concat_motion::style::Style;

/// The reel: the same stat-card layout three times, each scene in its
/// own personality, over the same drifting plate.
fn personalities(assets: PathBuf) -> Sequence {
    let mut sequence = Sequence::new(1920, 1080, 30.0);
    sequence.source_dir = assets;
    for (id, file) in [
        ("back", "assets/plate_dark.png"),
        ("Archivo", "assets/Archivo.ttf"),
        ("Inter", "assets/Inter.ttf"),
        ("JetBrains Mono", "assets/JetBrainsMono.ttf"),
    ] {
        sequence.asset(
            id,
            Asset {
                src: file.into(),
                kind: None,
            },
        );
    }
    for style in [Style::Calm, Style::Punchy, Style::Editorial] {
        sequence.push(
            Scene::new(presets::build("funcut.stats.triptych").expect("the preset"))
                .holding(5.0)
                .styled(style)
                .over("back", 0.03),
        );
    }
    sequence
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        std::process::exit(2);
    }
    let flag = |name: &str| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };
    let still_at: Option<f64> = flag("--still").and_then(|value| value.parse().ok());
    let plan_only = args.iter().any(|arg| arg == "--plan");
    let out = flag("-o")
        .map(|stem| {
            let stem = PathBuf::from(stem);
            match stem.extension().and_then(|e| e.to_str()) {
                Some("png") => stem.with_extension(""),
                _ => stem,
            }
        })
        .unwrap_or_else(|| PathBuf::from("style"));
    let assets = flag("--assets")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let cache = flag("--cache")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cache/style"));
    args.clear();

    let geneva = match Geneva::discover(None) {
        Some(geneva) => geneva,
        None => {
            eprintln!(
                "no geneva binary: set CONCAT_GENEVA_BIN, put it on PATH, or install the sidecar"
            );
            std::process::exit(1);
        }
    };
    let sequence = personalities(assets);
    let planned = match concat_motion::sequence::plan(&geneva, &sequence) {
        Ok(planned) => planned,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "ok": true,
            "scenes": planned.scenes,
            "duration": planned.duration,
            "layers": planned.document["layers"].as_array().map(Vec::len).unwrap_or(0),
            "warnings": planned.warnings,
        }))
        .unwrap_or_default()
    );
    if plan_only {
        println!(
            "{}",
            serde_json::to_string_pretty(&planned.document).unwrap_or_default()
        );
        return;
    }
    if let Some(at) = still_at {
        // The same moment of each scene, one still per personality: the
        // comparable three, at the stem the caller named.
        for (index, name) in ["calm", "punchy", "editorial"].iter().enumerate() {
            let start = 5.0 * index as f64;
            let file = out.with_file_name(format!(
                "{}-{name}.png",
                out.file_name().and_then(|s| s.to_str()).unwrap_or("style")
            ));
            match planned.frame(&geneva, &cache, start + at, &file) {
                Ok(report) => println!(
                    "{}",
                    serde_json::to_string_pretty(&report).unwrap_or_default()
                ),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        return;
    }
    let reel = out.with_extension("mp4");
    match planned.render(&geneva, &cache, &reel) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!("usage: style [--plan] [--still SECONDS] [-o STEM] [--assets DIR] [--cache DIR]");
}
