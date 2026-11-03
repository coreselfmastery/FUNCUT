// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The designed moment: the typography step's done-when, as a comp.
//!
//! A headline whose three words stagger in eighty milliseconds apart
//! with their weights ramping from 100 to 900 across them - a static
//! `font-weight` per word, the crescendo the engine draws, because it
//! cannot animate a weight at all (font-variation-settings is W450,
//! font-weight in a keyframe is E442) - a karaoke caption whose
//! words light when a Whisper transcript says they are spoken - a
//! three-line paragraph revealing one line after another - and a
//! gradient-ink headline - one comp, validated, mapped onto a Geneva
//! timeline, and written as a document a project can place.
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
    caption.transform.position = Some(concat_motion::comp::Point::new(960.0, 950.0));

    // A paragraph: two lines, 180 ms apart, the editorial reveal - at
    // its own smaller size, stacked under the headline the way a
    // title card stacks its argument.
    let paragraph_style = TextNode {
        font: "Archivo, Inter".into(),
        size: 56.0,
        ..headline_style.clone()
    };
    let mut paragraph = concat_motion::typography::paragraph(
        "paragraph",
        &[
            "A comp is a document.",
            "The renderer is not allowed an opinion.",
        ],
        &paragraph_style,
        &concat_motion::typography::Kinetic::lines().flexing_weight(300, 900),
    );
    paragraph.transform.position = Some(concat_motion::comp::Point::new(960.0, 700.0));

    // A gradient headline: the ink is a clip, not a colour, arriving on
    // its tracking.
    let mut gradient = concat_motion::typography::gradient_headline(
        "gradient",
        "EIGHTEEN BEATS",
        &caption_style,
        &concat_motion::typography::Kinetic::words()
            .entrance(concat_motion::typography::Entrance::TrackIn),
        "#d9a441",
        "#f4f1ea",
    );
    gradient.transform.position = Some(concat_motion::comp::Point::new(960.0, 160.0));

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
        comps: BTreeMap::new(),
        nodes: vec![headline, caption, paragraph, gradient],
        slots: Vec::new(),
    };

    let problems = comp.validate();
    assert!(problems.is_empty(), "{problems:?}");
    let (document, warnings) = geneva::Geneva::at("geneva")
        .timeline(&comp, &concat_motion::engine::BakeRequest::default());
    assert!(warnings.is_empty(), "{warnings:?}");
    let layers = document["layers"].as_array().expect("layers");
    println!(
        "{}: four nodes, {} timeline layers, {} bytes",
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
