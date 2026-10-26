// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! A whole motion reel, built the way a project builds one: twelve comps from
//! the library, authored per scene with the operation vocabulary, placed on one
//! timeline over plates, dipped between, scored, and rendered in one pass.
//!
//! ```sh
//! # the storyboard, printed as the timeline it becomes
//! cargo run -p concat-motion --example reel -- --plan > reel.json
//! # one still, to check a cut
//! cargo run -p concat-motion --example reel -- --frame 12.4 -o check.jpg
//! # the reel
//! cargo run -p concat-motion --example reel -- -o reel.mp4 --assets . --cache cache/reel
//! ```
//!
//! `--assets` is the folder the plates and the score resolve from; the render
//! directory (`--cache`) is filled with copies of everything the timeline
//! refers to, so it renders on a machine that has nothing else.

use std::path::PathBuf;

use concat_motion::authoring::Op;
use concat_motion::comp::{Asset, Rgba};
use concat_motion::curve::Curve;
use concat_motion::geneva::Geneva;
use concat_motion::presets;
use concat_motion::sequence::{AudioBed, Curtain, Scene, Sequence};

fn preset(id: &str) -> concat_motion::comp::Comp {
    presets::build(id).unwrap_or_else(|| panic!("no preset {id:?}"))
}

/// The storyboard: twelve beats, each a comp from the library with a few
/// operations on it, over a plate, under a curtain and a bed of sound.
fn reel(assets: PathBuf) -> Sequence {
    let mut sequence = Sequence::new(1920, 1080, 30.0);
    sequence.source_dir = assets;
    sequence.background = Rgba::hex("#05060a");
    sequence.curtain = Some(Curtain {
        color: Rgba::hex("#05060a"),
        open: 0.6,
        close: 0.8,
        dip: 0.20,
        depth: 1.0,
    });

    // The plates, and the fonts the comps name. A font shipped as a `font`
    // asset is the family the markup asks for, on a machine that has none.
    for (id, file) in [
        ("plate_dark", "assets/plate_dark.jpg"),
        ("plate_studio", "assets/plate_studio.jpg"),
        ("plate_sea", "assets/plate_sea.jpg"),
        ("plate_night", "assets/plate_night.jpg"),
        ("plate_forest", "assets/plate_forest.jpg"),
        ("plate_desert", "assets/plate_desert.jpg"),
        ("Inter", "assets/Inter.ttf"),
        ("Space Grotesk", "assets/SpaceGrotesk.ttf"),
        ("JetBrains Mono", "assets/JetBrainsMono.ttf"),
        ("Archivo", "assets/Archivo.ttf"),
        ("score", "assets/score_v2.wav"),
    ] {
        sequence.asset(
            id,
            Asset {
                src: file.into(),
                kind: None,
            },
        );
    }

    // 1. The opening: an opaque backdrop, titled by two added text nodes.
    //    Nothing is inherited from a template here - it is built from
    //    primitives, which is the case the vocabulary exists for.
    sequence.push(
        Scene::new(preset("funcut.backdrop.mesh"))
            .holding(4.0)
            .with_ops(vec![
                Op::AddText {
                    id: "title".into(),
                    text: "TWELVE BEATS".into(),
                    size: 132.0,
                    weight: 900,
                    color: "#f7f9ff".into(),
                    x: 960.0,
                    y: 500.0,
                },
                Op::AddText {
                    id: "kicker".into(),
                    text: "MOTION, BY THE COMP".into(),
                    size: 26.0,
                    weight: 700,
                    color: "#22d3c5".into(),
                    x: 960.0,
                    y: 620.0,
                },
                Op::FadeIn {
                    node: "title".into(),
                    seconds: 1.1,
                    curve: Curve::EMPHASIS,
                },
                Op::FadeIn {
                    node: "kicker".into(),
                    seconds: 1.4,
                    curve: Curve::EASE_OUT,
                },
                Op::Pop {
                    node: "title".into(),
                    from: 0.94,
                    seconds: 1.2,
                },
            ]),
    );

    // 2. The wordmark, over a plate.
    sequence.push(
        Scene::new(preset("funcut.sting.wordmark"))
            .over("plate_dark", 0.05)
            .with_ops(vec![Op::SetSlot {
                slot: "Wordmark".into(),
                value: "FUNCUT".into(),
            }]),
    );

    // 3. A chapter card.
    sequence.push(
        Scene::new(preset("funcut.chapter.chip"))
            .holding(3.0)
            .over("plate_night", 0.04)
            .with_ops(vec![Op::SetSlot {
                slot: "Chapter".into(),
                value: "TWELVE BEATS".into(),
            }]),
    );

    // 4. An interval: a lower third, arriving after the voice would have
    //    started and popping a touch as it lands.
    sequence.push(
        Scene::new(preset("funcut.lower-third.glass"))
            .holding(3.6)
            .over("plate_studio", 0.05)
            .with_ops(vec![
                Op::SetSlot {
                    slot: "Name".into(),
                    value: "Grace Hopper".into(),
                },
                Op::Move {
                    node: "card".into(),
                    start: 0.15,
                },
                Op::Pop {
                    node: "card".into(),
                    from: 0.96,
                    seconds: 0.6,
                },
            ]),
    );

    // 5. Captions, word by word: the reason a comp beats a title.
    sequence.push(
        Scene::new(preset("funcut.caption.kinetic"))
            .holding(4.0)
            .over("plate_sea", 0.05)
            .with_ops(vec![Op::SetWords {
                node: "caption".into(),
                words: vec![
                    ("Every".into(), 0.15, Some(0.55)),
                    ("cut".into(), 0.55, Some(0.95)),
                    ("is".into(), 0.95, Some(1.25)),
                    ("a".into(), 1.25, Some(1.5)),
                    ("decision.".into(), 1.5, Some(2.35)),
                    ("Make".into(), 2.35, Some(2.9)),
                    ("it".into(), 2.9, Some(3.2)),
                    ("move.".into(), 3.2, None),
                ],
            }]),
    );

    // 6. Three numbers, one line.
    sequence.push(
        Scene::new(preset("funcut.stats.triptych"))
            .holding(3.6)
            .over("plate_forest", 0.05)
            .with_ops(vec![Op::SetSlot {
                slot: "Numbers".into(),
                value: "COMPS SHIPPED".into(),
            }]),
    );

    // 7. A chart that builds on, titled.
    sequence.push(
        Scene::new(preset("funcut.chart.bars"))
            .holding(3.6)
            .over("plate_night", 0.04)
            .with_ops(vec![Op::SetSlot {
                slot: "Chart title".into(),
                value: "WHAT THE LAYER ADDS".into(),
            }]),
    );

    // 8. One number, counted in.
    sequence.push(
        Scene::new(preset("funcut.ticker.count"))
            .holding(3.0)
            .over("plate_desert", 0.05)
            .with_ops(vec![
                Op::SetSlot {
                    slot: "Number".into(),
                    value: "12".into(),
                },
                Op::FadeIn {
                    node: "num".into(),
                    seconds: 0.5,
                    curve: Curve::EMPHASIS,
                },
            ]),
    );

    // 9. A terminal that types itself.
    sequence.push(
        Scene::new(preset("funcut.code.terminal"))
            .holding(3.6)
            .over("plate_dark", 0.04)
            .with_ops(vec![Op::SetSlot {
                slot: "Window title".into(),
                value: "funcut — concat-motion".into(),
            }]),
    );

    // 10. Before and after, wiped.
    sequence.push(
        Scene::new(preset("funcut.compare.split"))
            .holding(3.6)
            .over("plate_sea", 0.04)
            .with_ops(vec![Op::SetSlot {
                slot: "After label".into(),
                value: "graded + motion".into(),
            }]),
    );

    // 11. A pull quote, allowed to breathe.
    sequence.push(
        Scene::new(preset("funcut.quote.pull"))
            .holding(4.0)
            .over("plate_night", 0.03)
            .with_ops(vec![Op::SetSlot {
                slot: "Quote".into(),
                value: "A comp is a document. The renderer is not allowed an opinion.".into(),
            }]),
    );

    // 12. The call to action.
    sequence.push(
        Scene::new(preset("funcut.cta.subscribe"))
            .holding(3.4)
            .over("plate_forest", 0.04)
            .with_ops(vec![Op::SetSlot {
                slot: "Headline".into(),
                value: "Twelve beats. None of them a browser.".into(),
            }]),
    );

    // 13. And the last card, over the tail of the curtain.
    sequence.push(
        Scene::new(preset("funcut.chapter.chip"))
            .holding(2.6)
            .over("plate_dark", 0.03)
            .with_ops(vec![
                Op::SetSlot {
                    slot: "Chapter".into(),
                    value: "SHIP IT".into(),
                },
                Op::FadeIn {
                    node: "chap".into(),
                    seconds: 0.5,
                    curve: Curve::EMPHASIS,
                },
            ]),
    );

    sequence.bed(AudioBed {
        asset: "score".into(),
        gain_db: -2.0,
        fade_in: 0.4,
        fade_out: 1.4,
    });

    sequence
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut assets = PathBuf::from(".");
    let mut cache = PathBuf::from("cache/reel");
    let mut out: Option<PathBuf> = None;
    let mut frame: Option<f64> = None;
    let mut plan_only = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--assets" => assets = PathBuf::from(args.next().expect("--assets wants a path")),
            "--cache" => cache = PathBuf::from(args.next().expect("--cache wants a path")),
            "-o" => out = Some(PathBuf::from(args.next().expect("-o wants a path"))),
            "--frame" => frame = Some(args.next().expect("--frame wants a time").parse()?),
            "--plan" => plan_only = true,
            "-h" | "--help" => {
                println!(
                    "reel [-o out.mp4] [--assets DIR] [--cache DIR] [--frame SECONDS] [--plan]"
                );
                return Ok(());
            }
            other => return Err(format!("unknown argument {other:?}").into()),
        }
    }

    let geneva = Geneva::discover(None)
        .ok_or("no geneva binary: set CONCAT_GENEVA_BIN or put geneva on the path")?;
    let sequence = reel(assets);
    let plan = concat_motion::sequence::plan(&geneva, &sequence)?;

    for warning in &plan.warnings {
        eprintln!("warning: {warning}");
    }

    if plan_only {
        println!("{}", plan.json());
        return Ok(());
    }

    if let Some(at) = frame {
        let path = out.unwrap_or_else(|| PathBuf::from("frame.jpg"));
        let report = plan.frame(&geneva, &cache, at, &path)?;
        println!("{report}");
        return Ok(());
    }

    let path = out.unwrap_or_else(|| PathBuf::from("reel.mp4"));
    let started = std::time::Instant::now();
    let report = plan.render(&geneva, &cache, &path)?;
    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "{} scenes, {:.1}s of picture, rendered in {:.1}s ({:.1} fps) -> {}",
        plan.scenes,
        plan.duration,
        seconds,
        plan.duration * sequence.fps / seconds,
        path.display()
    );
    println!("{report}");
    Ok(())
}
