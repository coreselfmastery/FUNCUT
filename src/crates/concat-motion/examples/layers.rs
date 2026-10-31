// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! One multi-layer scene, built the way a project builds one: a background
//! plate drifting, a midground sliding the other way, a wordmark over both,
//! a screen-blended light leak and a grain tile over the lot - five layers,
//! five clocks, one render.
//!
//! ```sh
//! # the storyboard, printed as the timeline it becomes
//! cargo run -p concat-motion --example layers -- --plan > layers.json
//! # one still, to check the stack
//! cargo run -p concat-motion --example layers -- --frame 2.0 -o check.jpg
//! # the reel
//! cargo run -p concat-motion --example layers -- -o layers.mp4 --assets . --cache cache/layers
//! ```
//!
//! `--assets` is the folder the pictures resolve from; the render directory
//! (`--cache`) is filled with copies of everything the timeline refers to,
//! so it renders on a machine that has nothing else.

use std::path::PathBuf;

use concat_motion::comp::{Asset, Blend, Fit};
use concat_motion::geneva::Geneva;
use concat_motion::presets;
use concat_motion::sequence::{Scene, SceneLayer, Sequence};

fn preset(id: &str) -> concat_motion::comp::Comp {
    presets::build(id).unwrap_or_else(|| panic!("no preset {id:?}"))
}

/// The scene: a stack of stills that moves on more than one clock, which is
/// what makes it read as a scene rather than a collage.
fn layered(assets: PathBuf) -> Sequence {
    let mut sequence = Sequence::new(1920, 1080, 30.0);
    sequence.source_dir = assets;
    for (id, file) in [
        ("back", "assets/plate_dark.png"),
        ("mid", "assets/plate_studio.png"),
        ("leak", "assets/leak.png"),
        ("grain", "assets/grain.png"),
        // The fonts the comps name, as `font` assets: without them the
        // engine draws its fallback and says W405 - honestly, in the reply.
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

    sequence.push(
        Scene::new(preset("funcut.sting.wordmark"))
            .holding(5.0)
            // The background: the classic plate, drifting five percent.
            .over("back", 0.05)
            // The midground: sliding left three percent while the
            // background grows - the differing drift is the depth.
            .layer(SceneLayer::new("mid").panning(-0.03, 0.0).fading(0.6, 0.9))
            // The light leak: a soft radial picture, screen-blended over
            // the comp, sitting up and left at four fifths size.
            .layer(
                SceneLayer::new("leak")
                    .blending(Blend::Screen)
                    .at_offset(-0.25, -0.2)
                    .scaled(0.8)
                    .fading(0.5, 0.5)
                    .above(),
            )
            // The grain: a tile at eight percent, over everything but the
            // curtain, running the whole scene.
            .layer(SceneLayer::new("grain").opacity(0.08).above()),
    );

    // A second scene, to show a foreground card arriving late: it starts
    // 1.2 s after its scene does, holds 2.4 s, and leaves before the cut.
    sequence.push(
        Scene::new(preset("funcut.chapter.chip"))
            .holding(5.0)
            .layer(
                SceneLayer::new("mid")
                    .fitted(Fit::Contain)
                    .scaled(0.6)
                    .starting_at(1.2)
                    .holding(2.4)
                    .fading(0.3, 0.3)
                    .above(),
            ),
    );
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
    let at_frame: Option<f64> = flag("--frame").and_then(|value| value.parse().ok());
    let plan_only = args.iter().any(|arg| arg == "--plan");
    let out = flag("-o")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("layers.mp4"));
    let assets = flag("--assets")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let cache = flag("--cache")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cache/layers"));
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
    let sequence = layered(assets);
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
    let report = if let Some(at) = at_frame {
        planned.frame(&geneva, &cache, at, &out)
    } else {
        planned.render(&geneva, &cache, &out)
    };
    match report {
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
    println!("usage: layers [--plan] [--frame SECONDS] [-o OUT] [--assets DIR] [--cache DIR]");
}
