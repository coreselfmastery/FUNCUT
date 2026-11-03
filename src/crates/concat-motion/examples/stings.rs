// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The sting, three times: scoped sub-compositions, placed.
//!
//! One comp document carries another under `comps`, and places it any
//! number of times - each placement with its own timing, position, scale
//! and rotation, each drawn from the one definition. The document scopes
//! (the sting's `bar` is its own, its assets would be its own); the
//! timeline flattens (`compositions` is the engine's one top-level map,
//! names spelled by scope); the bake carries every scope's files in its
//! key. This example is the worked proof: the same sting at three sizes,
//! the third tilted, arriving one after another.
//!
//! ```sh
//! cargo run -p concat-motion --example stings -- /tmp/stings
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use concat_motion::comp::{
    Comp, FORMAT, Length, Node, NodeKind, Paint, Point, Rgba, TextNode, Transform,
};
use concat_motion::engine::{BakeRequest, CompEngine};
use concat_motion::geneva::Geneva;

/// One placement: a composition source at a place, time and size.
fn place(id: &str, comp: &str, start: f64, x: f64, y: f64, scale: f64, rotation: f64) -> Node {
    Node {
        id: id.into(),
        parent: None,
        start,
        duration: 0.0,
        transform: Transform {
            position: Some(Point::new(x, y)),
            scale,
            rotation,
            ..Transform::default()
        },
        opacity: 1.0,
        blend: Default::default(),
        mask: None,
        effects: Vec::new(),
        tracks: Default::default(),
        kind: NodeKind::Composition { comp: comp.into() },
    }
}

/// The sting itself: a dark plate, a gold bar, a word - 640x360 of
/// reusable comp, the definition the placements share.
fn sting() -> Comp {
    Comp {
        format: FORMAT,
        id: "funcut.sting.unit".into(),
        name: "The sting".into(),
        width: 640,
        height: 360,
        fps: 30.0,
        duration: 1.2,
        background: Rgba::hex("#101319ff"),
        assets: BTreeMap::new(),
        curves: BTreeMap::new(),
        comps: BTreeMap::new(),
        nodes: vec![
            Node {
                id: "bar".into(),
                parent: None,
                start: 0.1,
                duration: 0.0,
                transform: Transform {
                    position: Some(Point::new(320.0, 168.0)),
                    ..Transform::default()
                },
                opacity: 1.0,
                blend: Default::default(),
                mask: None,
                effects: Vec::new(),
                tracks: Default::default(),
                kind: NodeKind::Rect {
                    width: Length::Px(360.0),
                    height: Length::Px(96.0),
                    fill: Some(Paint::Solid {
                        color: Rgba::hex("#d9a441ff"),
                    }),
                    radius: 12.0,
                },
            },
            Node {
                id: "word".into(),
                parent: None,
                start: 0.2,
                duration: 0.0,
                transform: Transform {
                    position: Some(Point::new(320.0, 216.0)),
                    ..Transform::default()
                },
                opacity: 1.0,
                blend: Default::default(),
                mask: None,
                effects: Vec::new(),
                tracks: Default::default(),
                kind: NodeKind::Text(Box::new(TextNode {
                    text: "STING".into(),
                    font: "Archivo, Inter, DejaVu Sans".into(),
                    size: 64.0,
                    weight: 800,
                    color: Rgba::hex("#101319ff"),
                    ..TextNode::default()
                })),
            },
        ],
        slots: Vec::new(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "stings".into());
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");

    let mut comp = Comp {
        format: FORMAT,
        id: "funcut.sting.thrice".into(),
        name: "The sting, three times".into(),
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration: 3.0,
        background: Rgba::hex("#0a0b10ff"),
        assets: BTreeMap::new(),
        curves: BTreeMap::new(),
        comps: {
            let mut m = BTreeMap::new();
            m.insert("sting".into(), sting());
            m
        },
        nodes: vec![
            place("one", "sting", 0.0, 400.0, 540.0, 1.0, 0.0),
            place("two", "sting", 0.9, 960.0, 540.0, 0.9, 0.0),
            place("three", "sting", 1.8, 1520.0, 540.0, 0.8, -6.0),
        ],
        slots: Vec::new(),
    };
    comp.nodes[2].transform.rotation = -6.0;

    let problems = comp.validate();
    assert!(problems.is_empty(), "{problems:?}");

    let geneva = Geneva::discover(None).ok_or("no geneva binary: set CONCAT_GENEVA_BIN")?;
    let dir = PathBuf::from(&out);
    let baked = geneva.bake(
        &comp,
        &BakeRequest {
            alpha: false,
            source_dir: here,
            ..Default::default()
        },
        &dir,
    )?;
    println!(
        "baked {} frames at {}x{} into {}",
        baked.frames,
        baked.width,
        baked.height,
        dir.display()
    );
    Ok(())
}
