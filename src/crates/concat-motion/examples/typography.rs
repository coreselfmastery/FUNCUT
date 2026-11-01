// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The designed moment: the typography step's done-when, as a comp.
//!
//! A variable-weight headline flexing from 100 to 900 while its three
//! words stagger in eighty milliseconds apart, and a karaoke caption
//! whose words light when a Whisper transcript says they are spoken -
//! one comp, validated, mapped onto a Geneva timeline, and written as a
//! document a project can place.
//!
//! ```sh
//! cargo run -p concat-motion --example typography -- /tmp/moment.json
//! ```

use std::collections::BTreeMap;

use concat_motion::comp::{Comp, Rgba, TextNode, Word};
use concat_motion::typography::{Entrance, Kinetic};
use concat_motion::{FORMAT, geneva};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "typography-moment.json".into());

    let kinetic = Kinetic::words()
        .delay(0.08)
        .entrance(Entrance::BlurIn)
        .flexing_weight(100, 900);

    let headline_style = TextNode {
        font: "Archivo, Inter".into(),
        size: 148.0,
        weight: 700,
        letter_spacing: -2.0,
        line_height: 1.05,
        color: Rgba::hex("#f4f1ea"),
        ..TextNode::default()
    };
    let mut headline =
        concat_motion::typography::headline("headline", "Make it move", &headline_style, &kinetic);
    headline.transform.position = Some(concat_motion::comp::Point::new(960.0, 420.0));

    // The recogniser's own word times, as a transcript arrives them.
    let words = vec![
        Word {
            text: "Make".into(),
            start: 0.4,
            end: Some(0.85),
        },
        Word {
            text: "it".into(),
            start: 0.85,
            end: Some(1.25),
        },
        Word {
            text: "yours.".into(),
            start: 1.25,
            end: None,
        },
    ];
    let caption_style = TextNode {
        font: "Archivo, Inter".into(),
        size: 84.0,
        weight: 800,
        letter_spacing: -1.0,
        line_height: 1.15,
        color: Rgba::hex("#f4f1ea"),
        highlight: Some(concat_motion::comp::TextStyle {
            color: Some(Rgba::hex("#d9a441")),
            weight: Some(800),
            ..Default::default()
        }),
        ..TextNode::default()
    };
    let mut caption =
        concat_motion::typography::karaoke(&words, 2.4, "caption", &caption_style, &kinetic);
    caption.transform.position = Some(concat_motion::comp::Point::new(960.0, 920.0));

    let comp = Comp {
        format: FORMAT,
        id: "funcut.typography.moment".into(),
        name: "The designed moment".into(),
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration: 3.0,
        background: Rgba::new(0.02, 0.024, 0.04, 1.0),
        assets: BTreeMap::new(),
        curves: BTreeMap::new(),
        nodes: vec![headline, caption],
        slots: Vec::new(),
    };

    let problems = comp.validate();
    assert!(problems.is_empty(), "{problems:?}");
    let (document, warnings) = geneva::Geneva::at("geneva")
        .timeline(&comp, &concat_motion::engine::BakeRequest::default());
    assert!(warnings.is_empty(), "{warnings:?}");
    let layers = document["layers"].as_array().expect("layers");
    println!(
        "{}: two nodes, {} timeline layers, {} bytes",
        comp.id,
        layers.len(),
        serde_json::to_string_pretty(&comp)
            .map(|text| text.len())
            .unwrap_or_default(),
    );
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&comp).expect("serialises"),
    )
    .unwrap_or_else(|error| panic!("could not write {out}: {error}"));
    println!("written to {out}");
}
