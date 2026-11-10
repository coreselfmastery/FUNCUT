// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Dump the data-graphics example comps: the documents an `add-chart`,
//! `add-counter`, `add-particles` or `set-camera` operation builds - the
//! same JSON an agent sends, and the same documents a project stores and
//! the sidecar renders.
//!
//! Every domain here is deliberately not finance: books finished, a
//! marathon's metres, a week's hours, a fire's embers. The operations are
//! the same whatever the numbers measure.
//!
//! ```sh
//! cargo run -p concat-motion --example dump_graphics -- examples/comps
//! ```

use std::collections::BTreeMap;

use concat_motion::authoring::apply_json;
use concat_motion::comp::{Comp, FORMAT, Rgba};
use serde_json::json;

/// A comp to hang the operations on: the shape a template or an agent
/// starts from, before any node is drawn.
fn base(id: &str, name: &str, seconds: f64) -> Comp {
    Comp {
        format: FORMAT,
        id: id.into(),
        name: name.into(),
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration: seconds,
        background: Rgba::new(0.0, 0.0, 0.0, 0.0),
        assets: BTreeMap::new(),
        curves: BTreeMap::new(),
        nodes: Vec::new(),
        comps: BTreeMap::new(),
        slots: Vec::new(),
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "comps".into());
    std::fs::create_dir_all(&out).unwrap();

    let examples: Vec<(&str, &str, f64, serde_json::Value)> = vec![
        (
            "data.chart.bars",
            "Books finished, by quarter",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 220, "y": 180, "width": 1480, "height": 760,
                 "chart": {"kind": "bar", "title": "Books finished, by quarter",
                           "subtitle": "paper and pixels, counted as finished",
                           "data": [
                               {"label": "Q1", "value": 12},
                               {"label": "Q2", "value": 9},
                               {"label": "Q3", "value": 14},
                               {"label": "Q4", "value": 17}]}}
            ]}),
        ),
        (
            "data.chart.line",
            "The Sunday long run",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 220, "y": 180, "width": 1480, "height": 760,
                 "chart": {"kind": "line", "title": "The Sunday long run",
                           "subtitle": "kilometres, week by week",
                           "format": {"suffix": " km", "decimals": 0},
                           "data": [
                               {"label": "W1", "value": 12},
                               {"label": "W2", "value": 14},
                               {"label": "W3", "value": 13},
                               {"label": "W4", "value": 16},
                               {"label": "W5", "value": 18},
                               {"label": "W6", "value": 21}]}}
            ]}),
        ),
        (
            "data.chart.donut",
            "Where the week goes",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 260, "y": 160, "width": 1400, "height": 800,
                 "chart": {"kind": "donut", "title": "Where the week goes",
                           "subtitle": "hours of one hundred and sixty-eight",
                           "center": "168 h",
                           "format": {"suffix": "%", "decimals": 0},
                           "data": [
                               {"label": "sleep", "value": 56},
                               {"label": "work", "value": 40},
                               {"label": "learning", "value": 6},
                               {"label": "everything else", "value": 66}]}}
            ]}),
        ),
        (
            "data.chart.dot",
            "The roast lab, cupped",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 240, "y": 180, "width": 1440, "height": 760,
                 "chart": {"kind": "dot", "title": "The roast lab, cupped",
                           "subtitle": "extraction yield, percent of the coffee's own weight",
                           "format": {"suffix": "%", "decimals": 1},
                           "data": [
                               {"label": "light, day one", "value": 18.2},
                               {"label": "light, day six", "value": 19.4},
                               {"label": "medium, day one", "value": 20.1},
                               {"label": "medium, day six", "value": 21.6},
                               {"label": "dark, day one", "value": 22.4},
                               {"label": "dark, day six", "value": 21.9}]}}
            ]}),
        ),
        (
            "data.chart.compare",
            "Paper against screen",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 240, "y": 180, "width": 1440, "height": 760,
                 "chart": {"kind": "compare", "title": "Paper against screen",
                           "subtitle": "books finished, by quarter",
                           "series": ["paper", "screen"],
                           "format": {"suffix": " books"},
                           "data": [
                               {"label": "Q1", "value": 7, "b": 5},
                               {"label": "Q2", "value": 2, "b": 7},
                               {"label": "Q3", "value": 6, "b": 8},
                               {"label": "Q4", "value": 8, "b": 9}]}}
            ]}),
        ),
        (
            "data.chart.progress",
            "The pledge drive",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 320, "y": 200, "width": 1280, "height": 720,
                 "chart": {"kind": "progress", "title": "The pledge drive",
                           "subtitle": "raised of each cause's own goal",
                           "format": {"suffix": "%", "decimals": 0},
                           "data": [
                               {"label": "the shelter's winter beds", "value": 4200, "b": 5000},
                               {"label": "the library's reading room", "value": 2600, "b": 4000},
                               {"label": "the pitch's new goalposts", "value": 6100, "b": 5000}]}}
            ]}),
        ),
        (
            "data.chart.candlestick",
            "A week on the roof",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 220, "y": 180, "width": 1480, "height": 760,
                 "chart": {"kind": "candlestick", "title": "A week on the roof",
                           "subtitle": "morning to noon, the day's range lettered",
                           "format": {"suffix": " °C", "decimals": 0},
                           "data": [
                               {"label": "Mon", "open": 11, "high": 19, "low": 9, "value": 17},
                               {"label": "Tue", "open": 14, "high": 21, "low": 12, "value": 15},
                               {"label": "Wed", "open": 15, "high": 16, "low": 10, "value": 11},
                               {"label": "Thu", "open": 9, "high": 14, "low": 7, "value": 13},
                               {"label": "Fri", "open": 12, "high": 23, "low": 11, "value": 22},
                               {"label": "Sat", "open": 18, "high": 26, "low": 15, "value": 24},
                               {"label": "Sun", "open": 20, "high": 22, "low": 14, "value": 15}]}}
            ]}),
        ),
        (
            "data.chart.timeline",
            "The observatory's decade",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 220, "y": 180, "width": 1480, "height": 760,
                 "chart": {"kind": "timeline", "title": "The observatory's decade",
                           "subtitle": "one axis, first light to the visitor wing",
                           "format": {"decimals": 0},
                           "data": [
                               {"label": "first light", "value": 2018},
                               {"label": "the new mirror", "value": 2020},
                               {"label": "the comet award", "value": 2022},
                               {"label": "the dome rebuilt", "value": 2025},
                               {"label": "the visitor wing", "value": 2026}]}}
            ]}),
        ),
        (
            "data.chart.table",
            "The pub quiz league",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-chart", "id": "chart", "x": 320, "y": 200, "width": 1280, "height": 720,
                 "chart": {"kind": "table", "title": "The pub quiz league",
                           "subtitle": "autumn standings, week nine",
                           "format": {"suffix": " pts", "decimals": 0},
                           "data": [
                               {"label": "The Buzzer Beaters", "value": 87, "color": "#d9a441"},
                               {"label": "Quizzy Riggers", "value": 81},
                               {"label": "The Decimal Points", "value": 78},
                               {"label": "Nacho Average Team", "value": 74},
                               {"label": "The Procrastinators", "value": 69}]}}
            ]}),
        ),
        (
            "data.counter",
            "One marathon",
            4.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-counter", "id": "metres", "x": 960, "y": 460,
                 "counter": {"from": 0, "to": 42195, "seconds": 1.6, "steps": 32,
                             "format": {"suffix": " m"},
                             "label": "one marathon, start line to finish"}}
            ]}),
        ),
        (
            "data.particles",
            "Embers",
            4.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0b0c0fff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-particles", "id": "embers", "x": 360, "y": 940,
                 "width": 1200, "height": 80,
                 "system": {"seed": 7, "count": 48, "lifetime": [1.4, 2.8],
                            "speed": [30, 120], "angle": [-100, -80],
                            "gravity": [0, -60], "drag": 0.4,
                            "turbulence": {"strength": 24, "scale": 0.006, "rate": 0.8},
                            "size": [3, 1.1], "opacity": [0.95, 0],
                            "colors": ["#d9a441", "#e8c982", "#b0764a"]}},
                {"op": "add-text", "id": "title", "text": "EMBERS", "size": 56,
                 "weight": 800, "color": "#f4f1ea", "x": 960, "y": 820}
            ]}),
        ),
        (
            "data.camera",
            "The year in books, pushed in on",
            5.0,
            json!({"ops": [
                {"op": "add-rect", "id": "plate", "width": 1920, "height": 1080,
                 "fill": "#0e0f12ff", "radius": 0, "x": 960, "y": 540},
                {"op": "add-text", "id": "title", "text": "THE YEAR IN BOOKS", "size": 64,
                 "weight": 800, "color": "#f4f1ea", "x": 960, "y": 130},
                {"op": "add-chart", "id": "chart", "x": 260, "y": 220, "width": 1400, "height": 760,
                 "chart": {"kind": "bar",
                           "data": [
                               {"label": "Q1", "value": 12},
                               {"label": "Q2", "value": 9},
                               {"label": "Q3", "value": 14},
                               {"label": "Q4", "value": 17}]}},
                {"op": "set-camera", "camera": {"zoom": [1.0, 1.08], "pan": [0, -30],
                                                "seconds": 3.5, "keys": 28,
                                                "depths": [["plate", 0.4], ["title", 1.3]]}}
            ]}),
        ),
    ];

    for (id, name, seconds, ops) in examples {
        let comp = apply_json(&base(id, name, seconds), &ops)
            .unwrap_or_else(|error| panic!("{id}: the example ops build: {error}"));
        assert!(comp.validate().is_empty(), "{id}: {:#?}", comp.validate());
        let path = format!("{out}/{id}.json");
        std::fs::write(&path, serde_json::to_string_pretty(&comp).unwrap()).unwrap();
        println!(
            "{path}  hash {:016x}  {} nodes",
            comp.hash(),
            comp.nodes.len()
        );
    }
}
