// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Bake one preset to an RGBA frame sequence, then draw a single frame - the
//! two calls the editor and the export path make.
//!
//! ```sh
//! CONCAT_GENEVA_BIN=/path/to/geneva \
//!   cargo run -p concat-motion --example bake_preset -- funcut.lower-third.glass /tmp/bake
//! ```

use std::path::PathBuf;

use concat_motion::engine::{BakeRequest, CompEngine, EnginePreference};
use concat_motion::geneva::Geneva;
use concat_motion::presets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let id = args
        .next()
        .unwrap_or_else(|| "funcut.lower-third.glass".to_string());
    let out = PathBuf::from(args.next().unwrap_or_else(|| "bake".to_string()));

    let comp = presets::build(&id).ok_or_else(|| format!("no preset called {id:?}"))?;
    let problems = comp.validate();
    if !problems.is_empty() {
        for problem in &problems {
            eprintln!("{}: {}", problem.path, problem.message);
        }
        return Err("the preset does not validate".into());
    }
    println!(
        "comp {} - {}x{} @ {} fps, {:.1}s, hash {:016x}",
        comp.id,
        comp.width,
        comp.height,
        comp.fps,
        comp.duration,
        comp.hash()
    );
    for slot in &comp.slots {
        println!("  slot {:?} on {}: {}", slot.label, slot.node, slot.sample);
    }

    let preference = EnginePreference::default();
    let geneva = Geneva::discover(preference.geneva.as_deref())
        .ok_or("no geneva binary: set CONCAT_GENEVA_BIN, put it on PATH, or install the sidecar")?;
    let probe = geneva.probe();
    println!("engine {} - {}", probe.available, probe.detail);
    if !probe.available {
        return Err("geneva is present but does not run".into());
    }

    let request = BakeRequest {
        alpha: true,
        ..BakeRequest::default()
    };
    let started = std::time::Instant::now();
    let baked = geneva.bake(&comp, &request, &out)?;
    let elapsed = started.elapsed();
    println!(
        "baked {} frames {}x{} alpha={} with {} {} in {:.2}s ({:.1} fps) into {}",
        baked.frames,
        baked.width,
        baked.height,
        baked.alpha,
        baked.engine.as_str(),
        baked.engine_version,
        elapsed.as_secs_f64(),
        baked.frames as f64 / elapsed.as_secs_f64().max(1e-9),
        baked.dir.display()
    );

    let started = std::time::Instant::now();
    let frame = geneva.frame(&comp, comp.duration * 0.4, Some(960))?;
    println!(
        "single frame in {:.0} ms: {}",
        started.elapsed().as_millis(),
        frame.display()
    );
    Ok(())
}
