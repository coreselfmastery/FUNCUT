// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The starting library: the comps a project gets before anyone opens the
//! motion editor.
//!
//! Each preset is an ordinary [`Comp`] - the same document the editor writes
//! and an agent can edit - built here in code so the library cannot drift from
//! the format. They are deliberately *slots*, not pictures: a lower third with
//! a name and a role, a caption row whose words come from a transcript, a stat
//! card with a number and a label. Filling a slot is what makes a preset a
//! template instead of a demo.
//!
//! The markup these carry is the reason Geneva is the default engine: flexbox,
//! gradients, `backdrop-filter`, `box-shadow`, `background-clip: text` and CSS
//! keyframes are all things a designer writes in a browser and expects to see,
//! and none of them exist in a shape-and-text model.

use std::collections::BTreeMap;

use crate::comp::{
    Comp, FORMAT, Length, Node, NodeKind, Rgba, Slot, SlotField, TextAlign, TextNode, TextStyle,
    Transform, Word,
};
use crate::curve::Curve;
use crate::track::{Key, Track, Value};

/// A preset: what it is called, and how to build it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Preset {
    /// The id a template, a saved project or an MCP call names.
    pub id: &'static str,
    /// What the gallery shows.
    pub name: &'static str,
    /// The section of the library it belongs to.
    pub category: Category,
    /// The engine it is meant for.
    pub engine: crate::engine::EngineKind,
}

/// The sections of the preset library.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Names, roles, badges: the thing over an interview.
    LowerThird,
    /// Word-timed text: what short-form video is made of.
    Captions,
    /// Numbers with a label and a bar.
    Stats,
    /// A logo with a line under it.
    Sting,
    /// A claim and its attribution.
    Quote,
    /// A named section of the video.
    Chapter,
    /// The end of a video: subscribe, follow, buy.
    CallToAction,
    /// A terminal, a code diff, a command: developer-facing motion.
    Code,
    /// Data drawn as bars, built on.
    Chart,
    /// Two versions of the same frame, split by a wipe.
    Compare,
    /// A moving background: mesh gradients and grid light.
    Backdrop,
    /// Rows that race: bars ranked live.
    Leaderboard,
    /// A feature table: two columns, one verdict.
    Table,
    /// The card a platform's own words sit on.
    Social,
    /// A headline band: the flash a bulletin is.
    News,
    /// A screen in a frame: phone, tablet, laptop.
    Device,
    /// A route drawn between pins.
    Map,
    /// Milestones along one rule.
    Timeline,
    /// Boxes and the lines that join them.
    Diagram,
}

impl Category {
    /// The name the gallery groups by.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::LowerThird => "Lower thirds",
            Category::Captions => "Captions",
            Category::Stats => "Stats",
            Category::Sting => "Stings",
            Category::Quote => "Quotes",
            Category::Chapter => "Chapters",
            Category::CallToAction => "Calls to action",
            Category::Code => "Code",
            Category::Chart => "Charts",
            Category::Compare => "Before / after",
            Category::Backdrop => "Backdrops",
            Category::Leaderboard => "Leaderboards",
            Category::Table => "Tables",
            Category::Social => "Social",
            Category::News => "News",
            Category::Device => "Devices",
            Category::Map => "Maps",
            Category::Timeline => "Timelines",
            Category::Diagram => "Diagrams",
        }
    }
}

/// Every preset, in library order.
pub const ALL: [Preset; 33] = [
    Preset {
        id: "funcut.lower-third.glass",
        name: "Glass lower third",
        category: Category::LowerThird,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.lower-third.minimal",
        name: "Minimal lower third",
        category: Category::LowerThird,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.caption.kinetic",
        name: "Kinetic caption",
        category: Category::Captions,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.stats.triptych",
        name: "Stat cards",
        category: Category::Stats,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.sting.wordmark",
        name: "Wordmark sting",
        category: Category::Sting,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chapter.numbered",
        name: "Numbered chapter",
        category: Category::Chapter,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chapter.agenda",
        name: "Agenda list",
        category: Category::Chapter,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.quote.pull",
        name: "Pull quote",
        category: Category::Quote,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chapter.chip",
        name: "Chapter chip",
        category: Category::Chapter,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.cta.subscribe",
        name: "Subscribe card",
        category: Category::CallToAction,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.cta.endcard",
        name: "End card",
        category: Category::CallToAction,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.code.terminal",
        name: "Terminal reveal",
        category: Category::Code,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.code.diff",
        name: "Code diff",
        category: Category::Code,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chart.bars",
        name: "Bar chart",
        category: Category::Chart,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chart.progress",
        name: "Progress bar",
        category: Category::Chart,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.compare.split",
        name: "Before / after split",
        category: Category::Compare,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.ticker.count",
        name: "Counting number",
        category: Category::Stats,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.backdrop.mesh",
        name: "Mesh backdrop",
        category: Category::Backdrop,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chart.line",
        name: "Line chart",
        category: Category::Chart,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chart.donut",
        name: "Donut chart",
        category: Category::Chart,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.chart.area",
        name: "Area chart",
        category: Category::Chart,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.leaderboard.bar-race",
        name: "Bar race",
        category: Category::Leaderboard,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.table.comparison",
        name: "Comparison table",
        category: Category::Table,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.social.quote",
        name: "Social quote card",
        category: Category::Social,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.social.stat",
        name: "Social stat card",
        category: Category::Social,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.news.headline-flash",
        name: "Headline flash",
        category: Category::News,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.device.phone",
        name: "Phone frame",
        category: Category::Device,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.device.tablet",
        name: "Tablet frame",
        category: Category::Device,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.device.laptop",
        name: "Laptop frame",
        category: Category::Device,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.map.pin-route",
        name: "Pin route map",
        category: Category::Map,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.timeline.horizontal",
        name: "Horizontal timeline",
        category: Category::Timeline,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.diagram.flowchart",
        name: "Flowchart",
        category: Category::Diagram,
        engine: crate::engine::EngineKind::Geneva,
    },
    Preset {
        id: "funcut.scene3d.showcase",
        name: "3D showcase",
        category: Category::Backdrop,
        engine: crate::engine::EngineKind::Native,
    },
];

/// Build a preset by id.
pub fn build(id: &str) -> Option<Comp> {
    match id {
        "funcut.lower-third.glass" => Some(lower_third()),
        "funcut.lower-third.minimal" => Some(minimal_lower_third()),
        "funcut.caption.kinetic" => Some(kinetic_caption()),
        "funcut.stats.triptych" => Some(stat_cards()),
        "funcut.sting.wordmark" => Some(wordmark_sting()),
        "funcut.quote.pull" => Some(pull_quote()),
        "funcut.chapter.chip" => Some(chapter_chip()),
        "funcut.chapter.numbered" => Some(numbered_chapter()),
        "funcut.chapter.agenda" => Some(agenda_list()),
        "funcut.cta.subscribe" => Some(subscribe_card()),
        "funcut.cta.endcard" => Some(end_card()),
        "funcut.code.terminal" => Some(terminal_reveal()),
        "funcut.code.diff" => Some(code_diff()),
        "funcut.chart.bars" => Some(bar_chart()),
        "funcut.chart.progress" => Some(progress_bar()),
        "funcut.compare.split" => Some(split_compare()),
        "funcut.ticker.count" => Some(counting_number()),
        "funcut.backdrop.mesh" => Some(mesh_backdrop()),
        "funcut.chart.line" => Some(line_chart()),
        "funcut.chart.donut" => Some(donut_chart()),
        "funcut.chart.area" => Some(area_chart()),
        "funcut.leaderboard.bar-race" => Some(bar_race()),
        "funcut.table.comparison" => Some(comparison_table()),
        "funcut.social.quote" => Some(social_quote()),
        "funcut.social.stat" => Some(social_stat()),
        "funcut.news.headline-flash" => Some(headline_flash()),
        "funcut.device.phone" => Some(device_phone()),
        "funcut.device.tablet" => Some(device_tablet()),
        "funcut.device.laptop" => Some(device_laptop()),
        "funcut.map.pin-route" => Some(pin_route()),
        "funcut.timeline.horizontal" => Some(horizontal_timeline()),
        "funcut.diagram.flowchart" => Some(flowchart()),
        "funcut.scene3d.showcase" => Some(scene3d_showcase()),
        _ => None,
    }
}

/// A 3D showcase: a box, a sphere and a torus over a floor, with a slow
/// camera move. The scene is drawn by the native engine, so the preset is
/// the library's native example rather than a markup one.
pub fn scene3d_showcase() -> Comp {
    let mut comp = comp("funcut.scene3d.showcase", "3D showcase", 1920, 1080, 8.0);
    comp.background = Rgba::hex("#070a0f");
    comp.nodes.push(Node {
        id: "scene".into(),
        parent: None,
        start: 0.0,
        duration: 0.0,
        transform: Transform::default(),
        opacity: 1.0,
        blend: crate::comp::Blend::Normal,
        mask: None,
        matte: None,
        effects: Vec::new(),
        motion_blur: None,
        tracks: BTreeMap::new(),
        kind: NodeKind::Scene3D {
            scene: crate::scene3d::Scene::showcase(1920, 1080),
        },
    });
    comp
}

// ------------------------------------------------------------------ helpers

fn comp(id: &str, name: &str, width: u32, height: u32, duration: f64) -> Comp {
    Comp {
        format: FORMAT,
        id: id.into(),
        name: name.into(),
        width,
        height,
        fps: 30.0,
        duration,
        background: Rgba::new(0.0, 0.0, 0.0, 0.0),
        assets: BTreeMap::new(),
        curves: BTreeMap::new(),
        nodes: Vec::new(),
        comps: BTreeMap::new(),
        slots: Vec::new(),
    }
}

/// A markup node whose box is the frame.
///
/// The box is the page, so the CSS inside can position with `left`/`top` the
/// way a browser page does - and `auto` is left for a card that should shrink
/// to its own content.
fn html_node(id: &str, markup: &str, css: &str, width: Length, height: Length) -> Node {
    Node {
        id: id.into(),
        parent: None,
        start: 0.0,
        duration: 0.0,
        transform: Transform::default(),
        opacity: 1.0,
        blend: crate::comp::Blend::Normal,
        mask: None,
        matte: None,
        motion_blur: None,
        effects: Vec::new(),
        tracks: BTreeMap::new(),
        kind: NodeKind::Html {
            markup: markup.into(),
            css: css.into(),
            width,
            height,
        },
    }
}

/// A node whose whole entrance is one spring-settled move, which is what makes
/// a card feel placed rather than pasted.
fn with_track(mut node: Node, name: &str, track: Track) -> Node {
    node.tracks.insert(name.into(), track);
    node
}

/// The shared font stack: a project pins its fonts as assets, and these names
/// are what the pinned families are called.
const MONO: &str = "JetBrains Mono, DejaVu Sans Mono, monospace";
const DISPLAY: &str = "Archivo, Inter, DejaVu Sans";

// ------------------------------------------------------- the library's look
//
// One design system, so a video cut from several presets reads as one hand:
// warm paper ink on a flat near-black plate, hairline borders, and a single
// editorial gold accent. No glass blur, no drop-shadow pyramids, no gradient
// text, no rainbow - the things that date an overlay the year after it
// shipped. Motion is staggered reveals and rules that draw; a spring only
// where something is placed, once per comp.
//
// `pub(crate)`: the typography builder highlights in the same ink and the
// same accent, so a kinetic caption reads as the same hand as the preset
// the words came from.
pub(crate) const INK: &str = "#f4f1ea";
pub(crate) const MUTED: &str = "#9a978e";
pub(crate) const ACCENT: &str = "#d9a441";
pub(crate) const SANS: &str = "Inter, DejaVu Sans";

// ----------------------------------------------------------------- presets

/// An editorial lower third: an accent bar that wipes down, a kicker, a
/// name, a role. The interview overlay, not the streamer's - no avatar
/// block, no LIVE badge, no glass blur: those read as a template the
/// moment they read at all.
pub fn lower_third() -> Comp {
    let css = format!(
        r#"
.card {{
  position: absolute; left: 120px; top: 776px;
  display: flex; flex-direction: row; align-items: stretch;
  background: rgba(12,13,16,0.92);
  border: 1px solid rgba(244,241,234,0.12); border-radius: 14px;
  box-shadow: 0 10px 28px rgba(0,0,0,0.35);
  overflow: hidden;
}}
.bar {{ width: 7px; background: {ACCENT}; transform-origin: top center;
  animation: wipe 0.55s cubic-bezier(0.16,1,0.3,1) 0.05s both; }}
.body {{ display: flex; flex-direction: column; justify-content: center;
  gap: 7px; padding: 26px 44px 28px 30px; }}
.kicker {{ font-family: {MONO}; font-size: 15px; font-weight: 600;
  letter-spacing: 0.28em; color: {ACCENT};
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.18s both; }}
.name {{ font-family: {DISPLAY}; font-size: 52px; font-weight: 800;
  letter-spacing: -0.02em; color: {INK}; line-height: 1;
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.26s both; }}
.role {{ font-family: {SANS}; font-size: 23px; font-weight: 400;
  color: {MUTED}; line-height: 1;
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.34s both; }}
@keyframes wipe {{ from {{ transform: scaleY(0) }} to {{ transform: scaleY(1) }} }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(10px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = r#"
<div class="card">
  <div class="bar"></div>
  <div class="body">
    <div class="kicker">WHY MONEY COMPOUNDS</div>
    <div class="name">Ada Lovelace</div>
    <div class="role">Analytical Engine · London, 1843</div>
  </div>
</div>
"#;
    let mut built = comp(
        "funcut.lower-third.glass",
        "Editorial lower third",
        1920,
        1080,
        6.0,
    );
    // One settle: the card arrives 14px low and rises - placed, not bounced.
    let node = with_track(
        html_node("card", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "position",
        Track::new(vec![
            Key::eased(0.0, Value::Point { x: 960.0, y: 554.0 }, Curve::EASE_OUT),
            Key::new(0.45, Value::Point { x: 960.0, y: 540.0 }),
        ]),
    );
    built.nodes.push(with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.3, Value::number(1.0)),
        ]),
    ));
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "Ada Lovelace".into(),
        label: "Name".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "Analytical Engine · London, 1843".into(),
        label: "Role".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "WHY MONEY COMPOUNDS".into(),
        label: "Kicker".into(),
    });
    built
}

/// Word-timed captions: the shape a transcript becomes.
///
/// The words arrive as data - `(word, start, end)` - and the highlight is a
/// style override, so the animation is the same document for any transcript.
pub fn kinetic_caption() -> Comp {
    let words = vec![
        Word {
            text: "Make".into(),
            start: 0.0,
            end: Some(0.45),
        },
        Word {
            text: "it".into(),
            start: 0.45,
            end: Some(0.85),
        },
        Word {
            text: "move.".into(),
            start: 0.85,
            end: Some(1.6),
        },
        Word {
            text: "Make".into(),
            start: 1.6,
            end: Some(2.0),
        },
        Word {
            text: "it".into(),
            start: 2.0,
            end: Some(2.4),
        },
        Word {
            text: "yours.".into(),
            start: 2.4,
            end: None,
        },
    ];
    let mut built = comp("funcut.caption.kinetic", "Kinetic caption", 1920, 1080, 4.0);
    let text = TextNode {
        text: String::new(),
        words,
        highlight: Some(TextStyle {
            color: Some(Rgba::hex(ACCENT)),
            fill: None,
            weight: Some(800),
            scale: None,
        }),
        font: DISPLAY.into(),
        size: 88.0,
        weight: 800,
        italic: false,
        letter_spacing: -2.0,
        line_height: 1.15,
        align: TextAlign::Center,
        max_width: Some(1500.0),
        fit: None,
        min_size: None,
        on_path: None,
        padding: 16.0,
        background: Rgba::hex("#05060a00"),
        radius: 0.0,
        color: Rgba::hex(INK),
        fill: None,
        outline: None,
        shadow: Some(crate::comp::Shadow {
            x: 0.0,
            y: 6.0,
            blur: 24.0,
            color: Rgba::hex("#00000099"),
        }),
    };
    let mut node = Node {
        id: "caption".into(),
        parent: None,
        start: 0.0,
        duration: 0.0,
        transform: Transform {
            position: Some(crate::comp::Point::new(960.0, 900.0)),
            ..Transform::default()
        },
        opacity: 1.0,
        blend: crate::comp::Blend::Normal,
        mask: None,
        matte: None,
        motion_blur: None,
        effects: Vec::new(),
        tracks: BTreeMap::new(),
        kind: NodeKind::Text(Box::new(text)),
    };
    node.tracks.insert(
        "opacity".into(),
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.12, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "caption".into(),
        field: SlotField::Text,
        sample: "Make it move. Make it yours.".into(),
        label: "Transcript line".into(),
    });
    built
}

/// Three stat cards that rise on a stagger, each with an accent top
/// hairline and one bar. Flat plates: the numbers carry it.
pub fn stat_cards() -> Comp {
    let css = format!(
        r#"
.row {{ position: absolute; left: 132px; width: 1656px; top: 360px;
  display: flex; flex-direction: row; gap: 28px; }}
.card {{ flex: 1; display: flex; flex-direction: column; gap: 10px;
  padding: 36px 36px 40px 36px; border-radius: 14px;
  background: rgba(12,13,16,0.92); border: 1px solid rgba(244,241,234,0.10);
  border-top: 3px solid {ACCENT};
  animation: rise 0.55s cubic-bezier(0.16,1,0.3,1) both; }}
.card:nth-child(2) {{ animation-delay: 0.12s; }}
.card:nth-child(3) {{ animation-delay: 0.24s; }}
.num {{ font-family: {DISPLAY}; font-size: 108px; font-weight: 800; letter-spacing: -0.04em; color: {INK}; line-height: 1; }}
.lab {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.24em; color: {MUTED}; }}
.track {{ height: 4px; border-radius: 2px; background: rgba(244,241,234,0.10); margin-top: 16px; }}
.fill {{ height: 4px; border-radius: 2px; background: {ACCENT}; transform-origin: left center;
  animation: widen 1.0s cubic-bezier(0.16,1,0.3,1) 0.5s both; }}
@keyframes widen {{ from {{ transform: scaleX(0) }} to {{ transform: scaleX(1) }} }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(14px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = r#"
<div class="row">
  <div class="card"><div class="num">127</div><div class="lab">SHADER PACKS</div>
    <div class="track"><div class="fill" style="width: 88%"></div></div></div>
  <div class="card"><div class="num">67</div><div class="lab">MCP TOOLS</div>
    <div class="track"><div class="fill" style="width: 46%"></div></div></div>
  <div class="card"><div class="num">10-BIT</div><div class="lab">LINEAR LIGHT</div>
    <div class="track"><div class="fill" style="width: 72%"></div></div></div>
</div>
"#;
    let mut built = comp("funcut.stats.triptych", "Stat cards", 1920, 1080, 5.0);
    let node = with_track(
        html_node("row", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.92), Curve::SPRING_POP),
            Key::new(0.55, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.25, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "row".into(),
        field: SlotField::Text,
        sample: "SHADER PACKS".into(),
        label: "Label 1".into(),
    });
    built.slots.push(Slot {
        node: "row".into(),
        field: SlotField::Text,
        sample: "127".into(),
        label: "Number 1".into(),
    });
    built.slots.push(Slot {
        node: "row".into(),
        field: SlotField::Text,
        sample: "67".into(),
        label: "Number 2".into(),
    });
    built.slots.push(Slot {
        node: "row".into(),
        field: SlotField::Text,
        sample: "MCP TOOLS".into(),
        label: "Label 2".into(),
    });
    built.slots.push(Slot {
        node: "row".into(),
        field: SlotField::Text,
        sample: "10-BIT".into(),
        label: "Number 3".into(),
    });
    built.slots.push(Slot {
        node: "row".into(),
        field: SlotField::Text,
        sample: "LINEAR LIGHT".into(),
        label: "Label 3".into(),
    });
    built
}

/// A wordmark with a rule that draws under it: solid ink and one accent
/// full stop - the gradient-text version was the template smell.
pub fn wordmark_sting() -> Comp {
    let css = format!(
        r#"
.stack {{ position: absolute; inset: 0; display: flex; flex-direction: column;
  align-items: center; justify-content: center; }}
.mark {{ font-family: {DISPLAY}; font-size: 190px; font-weight: 900; letter-spacing: -0.045em;
  color: {INK}; }}
.mark em {{ font-style: normal; color: {ACCENT}; }}
.rule {{ width: 0px; height: 3px; border-radius: 2px; margin: 26px 0 22px 0;
  background: {ACCENT};
  animation: grow 0.9s cubic-bezier(0.16,1,0.3,1) 0.45s both; }}
.sub {{ font-family: {MONO}; font-size: 20px; letter-spacing: 0.42em; color: {MUTED}; }}
@keyframes grow {{ from {{ width: 0px }} to {{ width: 620px }} }}
"#
    );
    let markup = r#"
<div class="stack">
  <div class="mark">FUNCUT<em>.</em></div>
  <div class="rule"></div>
  <div class="sub">MOTION</div>
</div>
"#;
    let mut built = comp("funcut.sting.wordmark", "Wordmark sting", 1920, 1080, 3.5);
    let node = with_track(
        html_node("mark", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(1.12), Curve::EASE_OUT),
            Key::new(0.6, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.3, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "mark".into(),
        field: SlotField::Text,
        sample: "FUNCUT".into(),
        label: "Wordmark".into(),
    });
    built.slots.push(Slot {
        node: "mark".into(),
        field: SlotField::Text,
        sample: "MOTION".into(),
        label: "Subline".into(),
    });
    built
}

/// A pull quote with its attribution.
pub fn pull_quote() -> Comp {
    let css = format!(
        r#"
.sheet {{ position: absolute; left: 180px; width: 1560px; top: 300px;
  display: flex; flex-direction: column; gap: 24px; padding: 44px 48px 48px 48px;
  border-radius: 16px; background: rgba(12,13,16,0.92);
  border: 1px solid rgba(244,241,234,0.10); border-left: 4px solid {ACCENT};
  box-shadow: 0 12px 32px rgba(0,0,0,0.35); }}
.q {{ font-family: {DISPLAY}; font-size: 66px; font-weight: 700; line-height: 1.14;
  letter-spacing: -0.02em; color: {INK}; }}
.a {{ font-family: {MONO}; font-size: 18px; letter-spacing: 0.22em; color: {MUTED}; }}
.marks {{ font-family: {DISPLAY}; font-size: 96px; font-weight: 900; color: {ACCENT}; line-height: 1; }}
"#
    );
    let markup = r#"
<div class="sheet">
  <div class="marks">&ldquo;</div>
  <div class="q">The last ten percent of a picture is the part people remember.</div>
  <div class="a">— FUNCUT, MOTION NOTES</div>
</div>
"#;
    let mut built = comp("funcut.quote.pull", "Pull quote", 1920, 1080, 6.0);
    let node = with_track(
        html_node(
            "sheet",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_IN_OUT),
            Key::new(0.4, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.97), Curve::EASE_OUT),
            Key::new(0.5, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "The last ten percent of a picture is the part people remember.".into(),
        label: "Quote".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "FUNCUT, MOTION NOTES".into(),
        label: "Attribution".into(),
    });
    built
}

/// A chapter chip: a number, a label and a rule - the label carries it,
/// so the label is display-size and the number is quiet gold mono.
pub fn chapter_chip() -> Comp {
    let css = format!(
        r#"
.chap {{ position: absolute; left: 120px; top: 88px; display: flex; flex-direction: column; gap: 12px; }}
.num {{ font-family: {MONO}; font-size: 17px; letter-spacing: 0.3em; color: {ACCENT};
  text-shadow: 0 2px 10px rgba(0,0,0,0.5); }}
.lab {{ font-family: {DISPLAY}; font-size: 46px; font-weight: 800; letter-spacing: -0.015em; color: {INK};
  text-shadow: 0 2px 14px rgba(0,0,0,0.55); }}
.ul {{ width: 0px; height: 3px; background: {ACCENT}; animation: ul 0.8s cubic-bezier(0.16,1,0.3,1) 0.4s both; }}
@keyframes ul {{ from {{ width: 0px }} to {{ width: 260px }} }}
"#
    );
    let markup = r#"
<div class="chap">
  <div class="num">02 / 06</div>
  <div class="lab">LOWER THIRDS</div>
  <div class="ul"></div>
</div>
"#;
    let mut built = comp("funcut.chapter.chip", "Chapter chip", 1920, 1080, 5.0);
    let node = with_track(
        html_node("chap", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "position",
        Track::new(vec![
            Key::eased(0.0, Value::Point { x: 960.0, y: 500.0 }, Curve::EASE_OUT),
            Key::new(0.5, Value::Point { x: 960.0, y: 540.0 }),
        ]),
    );
    let node = with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.3, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "chap".into(),
        field: SlotField::Text,
        sample: "LOWER THIRDS".into(),
        label: "Chapter".into(),
    });
    built.slots.push(Slot {
        node: "chap".into(),
        field: SlotField::Text,
        sample: "02 / 06".into(),
        label: "Number".into(),
    });
    built
}

/// A subscribe card: typographic - a big ask, one accent word with a rule
/// that draws under it, an outlined button that settles once. No glow, no
/// pulse: the ask is the design.
pub fn subscribe_card() -> Comp {
    let css = format!(
        r#"
.wrap {{ position: absolute; inset: 0; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: 34px; }}
.ask {{ font-family: {DISPLAY}; font-size: 88px; font-weight: 800; letter-spacing: -0.035em;
  color: {INK}; }}
.ask em {{ font-style: normal; color: {ACCENT}; }}
.ul {{ width: 260px; height: 5px; border-radius: 3px; background: {ACCENT}; transform-origin: left center;
  animation: draw 0.7s cubic-bezier(0.16,1,0.3,1) 0.55s both; }}
.btn {{ display: flex; flex-direction: row; align-items: center; gap: 14px;
  padding: 18px 36px; border-radius: 999px; background: rgba(12,13,16,0.92);
  border: 1.5px solid {ACCENT}; }}
.dot {{ width: 10px; height: 10px; border-radius: 999px; background: {ACCENT}; }}
.txt {{ font-family: {MONO}; font-size: 21px; font-weight: 700; letter-spacing: 0.2em; color: {INK}; }}
.sub {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.24em; color: {MUTED}; }}
@keyframes draw {{ from {{ transform: scaleX(0) }} to {{ transform: scaleX(1) }} }}
"#
    );
    let markup = r#"
<div class="wrap">
  <div class="ask">One <em>tap</em> away.</div>
  <div class="ul"></div>
  <div class="btn"><div class="dot"></div><div class="txt">SUBSCRIBE</div></div>
  <div class="sub">NEW EPISODE EVERY FRIDAY</div>
</div>
"#;
    let mut built = comp("funcut.cta.subscribe", "Subscribe card", 1920, 1080, 4.5);
    let node = with_track(
        html_node("card", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.25, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.94), Curve::SPRING_POP),
            Key::new(0.5, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "One tap away.".into(),
        label: "Headline".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "NEW EPISODE EVERY FRIDAY".into(),
        label: "Subline".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "SUBSCRIBE".into(),
        label: "Button".into(),
    });
    built
}

/// A terminal window: the prompt, a command, and output that lands line by
/// line on a `steps()` curve.
pub fn terminal_reveal() -> Comp {
    let css = format!(
        r#"
.win {{ position: absolute; left: 160px; top: 250px; width: 1600px;
  display: flex; flex-direction: column; border-radius: 18px; overflow: hidden;
  background: #07090d; border: 1px solid rgba(247,249,255,0.14);
  box-shadow: 0 30px 80px rgba(0,0,0,0.6); }}
.bar {{ display: flex; flex-direction: row; align-items: center; gap: 9px;
  padding: 16px 22px; background: #0d1117; border-bottom: 1px solid rgba(247,249,255,0.08); }}
.light {{ width: 13px; height: 13px; border-radius: 999px; }}
.title {{ font-family: {MONO}; font-size: 15px; letter-spacing: 0.16em; color: #6b7a89; margin-left: 12px; }}
.body {{ display: flex; flex-direction: column; gap: 12px; padding: 30px 34px 38px 34px; }}
.line {{ font-family: {MONO}; font-size: 26px; color: #d7e2ec; white-space: nowrap; }}
.p {{ color: {ACCENT}; }}
.ok {{ color: #4ade80; }}
.dim {{ color: #6b7a89; }}
.l1 {{ animation: type 0.5s steps(24) 0.15s both; }}
.l2 {{ animation: type 0.6s steps(30) 0.75s both; }}
.l3 {{ animation: type 0.5s steps(26) 1.45s both; }}
.l4 {{ animation: type 0.4s steps(18) 2.1s both; }}
@keyframes type {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}
"#
    );
    let markup = r#"
<div class="win">
  <div class="bar">
    <div class="light" style="background: #ff5f57"></div>
    <div class="light" style="background: #febc2e"></div>
    <div class="light" style="background: #28c840"></div>
    <div class="title">funcut — motion</div>
  </div>
  <div class="body">
    <div class="line l1"><span class="p">~ $</span> funcut motion bake lower-third --crf 18</div>
    <div class="line l2 dim">→ geneva 1.6.0 · 1920x1080 · 30 fps · alpha</div>
    <div class="line l3"><span class="ok">✓</span> 180 frames in 6.5 s (27.7 fps)</div>
    <div class="line l4 dim">browserless · linear light · 10-bit</div>
  </div>
</div>
"#;
    let mut built = comp("funcut.code.terminal", "Terminal reveal", 1920, 1080, 4.2);
    let node = with_track(
        html_node("win", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "position",
        Track::new(vec![
            Key::eased(0.0, Value::Point { x: 960.0, y: 610.0 }, Curve::EASE_OUT),
            Key::new(0.45, Value::Point { x: 960.0, y: 540.0 }),
        ]),
    );
    let node = with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "win".into(),
        field: SlotField::Text,
        sample: "funcut — motion".into(),
        label: "Window title".into(),
    });
    built
}

/// A bar chart that builds on: four bars, a baseline, and labels. One
/// accent at falling weights - the eye ranks the bars without a legend.
///
/// The words are slots - the title, the subline, every bar's label - so a
/// ranking fills in without touching markup. The heights are the design's
/// own falling rhythm, which is right whenever the story is a ranking; a
/// chart whose bars must carry real data ("you put in $180k, time put in
/// the rest") rewrites the columns with
/// [`Op::SetMarkup`](crate::authoring::Op::SetMarkup), keeping this
/// stylesheet and its rise animation.
pub fn bar_chart() -> Comp {
    let bars = [
        ("Q1", 96.0, ACCENT, "0.20"),
        ("Q2", 74.0, "rgba(217,164,65,0.68)", "0.32"),
        ("Q3", 58.0, "rgba(217,164,65,0.46)", "0.44"),
        ("Q4", 41.0, "rgba(217,164,65,0.28)", "0.56"),
    ];
    let mut columns = String::new();
    for (label, pct, colour, delay) in bars {
        columns.push_str(&format!(
            r#"<div class="col"><div class="bar" style="height: {pct}%; background: {colour}; animation-delay: {delay}s"></div><div class="lab">{label}</div></div>"#
        ));
    }
    let css = format!(
        r#"
.sheet {{ position: absolute; left: 160px; width: 1600px; top: 180px; height: 720px;
  display: flex; flex-direction: column; gap: 18px; }}
.head {{ display: flex; flex-direction: row; align-items: baseline; gap: 16px; }}
.t {{ font-family: {DISPLAY}; font-size: 46px; font-weight: 800; color: {INK}; }}
.s {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.22em; color: {MUTED}; }}
.plot {{ position: relative; flex: 1; display: flex; flex-direction: row; align-items: flex-end;
  gap: 56px; padding: 0 20px; border-bottom: 2px solid rgba(244,241,234,0.16); }}
.col {{ flex: 1; display: flex; flex-direction: column; justify-content: flex-end; align-items: center; height: 100%; }}
.bar {{ width: 100%; border-radius: 10px 10px 0 0; transform-origin: bottom center;
  animation: rise 1s cubic-bezier(0.16,1,0.3,1) both; }}
.lab {{ font-family: {MONO}; font-size: 17px; letter-spacing: 0.12em; color: {MUTED};
  padding-top: 14px; }}
@keyframes rise {{ from {{ transform: scaleY(0) }} to {{ transform: scaleY(1) }} }}
"#
    );
    let markup = format!(
        r#"<div class="sheet"><div class="head"><div class="t">Where the money went</div><div class="s">SHARE OF TOTAL, %</div></div><div class="plot">{columns}</div></div>"#
    );
    let mut built = comp("funcut.chart.bars", "Bar chart", 1920, 1080, 4.6);
    let node = with_track(
        html_node(
            "sheet",
            &markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Where the money went".into(),
        label: "Chart title".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "SHARE OF TOTAL, %".into(),
        label: "Chart subline".into(),
    });
    for (index, (label, _, _, _)) in bars.iter().enumerate() {
        built.slots.push(Slot {
            node: "sheet".into(),
            field: SlotField::Text,
            sample: (*label).into(),
            label: format!("Bar {} label", index + 1),
        });
    }
    built
}

/// A before/after split: one frame, a wipe, and a label on each side.
pub fn split_compare() -> Comp {
    let css = format!(
        r#"
.frame {{ position: absolute; left: 160px; width: 1600px; top: 200px; height: 680px;
  border-radius: 14px; overflow: hidden; display: flex; flex-direction: row; }}
.side {{ flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 14px; }}
.before {{ background: linear-gradient(150deg, #1c1a17, #26221d); }}
.after {{ background: linear-gradient(150deg, #241d10, #2e2412); }}
.tag {{ font-family: {MONO}; font-size: 18px; letter-spacing: 0.3em; }}
.tag.b {{ color: {MUTED}; }}
.tag.a {{ color: {ACCENT}; }}
.big {{ font-family: {DISPLAY}; font-size: 58px; font-weight: 800; color: {INK}; letter-spacing: -0.02em; }}
.wipe {{ position: absolute; left: 50%; top: 200px; width: 4px; height: 680px; background: {ACCENT};
  animation: wipe 1.4s cubic-bezier(0.16,1,0.3,1) 0.3s both; }}
.knob {{ position: absolute; left: 50%; top: 520px; width: 54px; height: 54px; border-radius: 999px;
  background: {INK}; margin-left: -27px; }}
@keyframes wipe {{ from {{ left: 12%; opacity: 0 }} to {{ left: 50%; opacity: 1 }} }}
"#
    );
    let markup = r#"
<div class="frame">
  <div class="side before"><div class="tag b">BEFORE</div><div class="big">raw cut</div></div>
  <div class="side after"><div class="tag a">AFTER</div><div class="big">graded + motion</div></div>
</div>
<div class="wipe"><div class="knob"></div></div>
"#;
    let mut built = comp(
        "funcut.compare.split",
        "Before / after split",
        1920,
        1080,
        4.5,
    );
    let node = with_track(
        html_node(
            "frame",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(1.03), Curve::EASE_OUT),
            Key::new(0.6, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "frame".into(),
        field: SlotField::Text,
        sample: "graded + motion".into(),
        label: "After label".into(),
    });
    built.slots.push(Slot {
        node: "frame".into(),
        field: SlotField::Text,
        sample: "raw cut".into(),
        label: "Before text".into(),
    });
    built.slots.push(Slot {
        node: "frame".into(),
        field: SlotField::Text,
        sample: "BEFORE".into(),
        label: "Before tag".into(),
    });
    built.slots.push(Slot {
        node: "frame".into(),
        field: SlotField::Text,
        sample: "AFTER".into(),
        label: "After tag".into(),
    });
    built
}

/// A number that counts up, with its label and a rule. Solid ink - the
/// number is the picture, and gradient text was the tell of a template.
pub fn counting_number() -> Comp {
    let css = format!(
        r#"
.wrap {{ position: absolute; inset: 0; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: 8px; }}
.num {{ font-family: {DISPLAY}; font-size: 270px; font-weight: 800; letter-spacing: -0.05em;
  line-height: 0.95; color: {INK}; text-shadow: 0 4px 24px rgba(0,0,0,0.5); }}
.lab {{ font-family: {MONO}; font-size: 20px; letter-spacing: 0.3em; color: {MUTED};
  text-shadow: 0 2px 10px rgba(0,0,0,0.5); }}
.rule {{ width: 260px; height: 3px; border-radius: 2px; background: {ACCENT}; margin-top: 18px;
  transform-origin: left center; animation: grow 1.0s cubic-bezier(0.16,1,0.3,1) 0.35s both; }}
@keyframes grow {{ from {{ transform: scaleX(0) }} to {{ transform: scaleX(1) }} }}
"#
    );
    let markup = r#"
<div class="wrap">
  <div class="num">67</div>
  <div class="lab">TOOLS AN AGENT CAN CALL</div>
  <div class="rule"></div>
</div>
"#;
    let mut built = comp("funcut.ticker.count", "Counting number", 1920, 1080, 4.0);
    let node = with_track(
        html_node("num", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.88), Curve::SPRING_POP),
            Key::new(0.6, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "num".into(),
        field: SlotField::Text,
        sample: "67".into(),
        label: "Number".into(),
    });
    built.slots.push(Slot {
        node: "num".into(),
        field: SlotField::Text,
        sample: "TOOLS AN AGENT CAN CALL".into(),
        label: "Label".into(),
    });
    built
}

/// A backdrop: a mesh gradient that breathes, with a grid of light.
///
/// The comp a timeline puts *under* everything else - opaque, so it replaces a
/// plate rather than sitting on one. Each gradient is its own element: the
/// markup engine draws one background layer per box, and a stack of boxes is
/// also how the motion is kept cheap.
pub fn mesh_backdrop() -> Comp {
    let css = r#"
.base { position: absolute; inset: 0; background: linear-gradient(150deg, #0a0a0c, #0e0d0b 60%, #0a0a0c); }
.g1 { position: absolute; inset: 0; background: radial-gradient(circle at 24% 28%, #2e2410, #00000000);
  animation: drift1 24s ease-in-out infinite; }
.g2 { position: absolute; inset: 0; background: radial-gradient(circle at 76% 30%, #1a1c22, #00000000);
  animation: drift2 30s ease-in-out infinite; }
.g3 { position: absolute; inset: 0; background: radial-gradient(circle at 52% 86%, #241c0e, #00000000);
  animation: drift1 27s ease-in-out infinite; }
.line { position: absolute; background: rgba(244,241,234,0.05); }
.vig { position: absolute; inset: 0;
  background: radial-gradient(ellipse at 50% 50%, rgba(0,0,0,0) 38%, rgba(0,0,0,0.72) 100%); }
@keyframes drift1 { from { transform: translate(0px, 0px) } 50% { transform: translate(70px, 40px) } to { transform: translate(0px, 0px) } }
@keyframes drift2 { from { transform: translate(0px, 0px) } 50% { transform: translate(-80px, -30px) } to { transform: translate(0px, 0px) } }
"#;
    // The grid is drawn as one-pixel boxes rather than a repeating gradient:
    // geneva reads gradient colours but not gradient stop positions, and a
    // hard stop is exactly what a grid line is. Twenty-seven boxes cost less
    // than the soft band a stopless gradient would draw.
    let mut grid = String::new();
    let step = 120.0_f64;
    let mut x = -step;
    while x <= 1920.0 + step {
        grid.push_str(&format!(
            r#"<div class="line" style="left: {x:.0}px; top: -120px; width: 1px; height: 1320px"></div>"#
        ));
        x += step;
    }
    let mut y = -step;
    while y <= 1080.0 + step {
        grid.push_str(&format!(
            r#"<div class="line" style="left: -120px; top: {y:.0}px; width: 2160px; height: 1px"></div>"#
        ));
        y += step;
    }
    let markup = format!(
        "<div class=\"base\"></div><div class=\"g1\"></div><div class=\"g2\"></div><div class=\"g3\"></div>{grid}<div class=\"vig\"></div>"
    );
    let mut built = comp("funcut.backdrop.mesh", "Mesh backdrop", 1920, 1080, 6.0);
    // Opaque: a backdrop is the picture, not an overlay.
    built.background = Rgba::hex("#06070bff");
    built.nodes.push(html_node(
        "mesh",
        &markup,
        css,
        Length::Px(1920.0),
        Length::Px(1080.0),
    ));
    built
}

/// A minimal lower third: no card, no shadow to shout over the footage -
/// a name, a role in mono, and a hairline rule that draws from the left.
/// The documentary read, where the glass card would have been the loudest
/// thing in the frame.
pub fn minimal_lower_third() -> Comp {
    let css = format!(
        r#"
.wrap {{ position: absolute; left: 140px; top: 768px;
  display: flex; flex-direction: column; align-items: flex-start; gap: 14px; }}
.name {{ font-family: {DISPLAY}; font-size: 56px; font-weight: 800;
  letter-spacing: -0.02em; color: {INK}; line-height: 1;
  text-shadow: 0 2px 14px rgba(0,0,0,0.55);
  animation: rise 0.55s cubic-bezier(0.16,1,0.3,1) 0.12s both; }}
.role {{ font-family: {MONO}; font-size: 18px; font-weight: 500;
  letter-spacing: 0.22em; color: {MUTED}; line-height: 1;
  text-shadow: 0 2px 10px rgba(0,0,0,0.5);
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.24s both; }}
.rule {{ width: 0px; height: 2px; background: rgba(244,241,234,0.35);
  animation: draw 0.7s cubic-bezier(0.16,1,0.3,1) 0.34s both; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(10px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes draw {{ from {{ width: 0px }} to {{ width: 380px }} }}
"#
    );
    let markup = r#"
<div class="wrap">
  <div class="name">Grace Hopper</div>
  <div class="role">REAR ADMIRAL · COMPILER PIONEER</div>
  <div class="rule"></div>
</div>
"#;
    let mut built = comp(
        "funcut.lower-third.minimal",
        "Minimal lower third",
        1920,
        1080,
        5.0,
    );
    let node = with_track(
        html_node(
            "third",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "position",
        Track::new(vec![
            Key::eased(0.0, Value::Point { x: 960.0, y: 552.0 }, Curve::EASE_OUT),
            Key::new(0.5, Value::Point { x: 960.0, y: 540.0 }),
        ]),
    );
    let node = with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.28, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "third".into(),
        field: SlotField::Text,
        sample: "Grace Hopper".into(),
        label: "Name".into(),
    });
    built.slots.push(Slot {
        node: "third".into(),
        field: SlotField::Text,
        sample: "REAR ADMIRAL · COMPILER PIONEER".into(),
        label: "Role".into(),
    });
    built
}

/// A numbered chapter: the full-frame read of a section change - a kicker,
/// a giant number, an accent rule that draws under it, a title. The chip
/// is the corner badge; this is the door the viewer walks through.
pub fn numbered_chapter() -> Comp {
    let css = format!(
        r#"
.wrap {{ position: absolute; inset: 0; display: flex; flex-direction: column;
  justify-content: center; align-items: flex-start; padding-left: 220px; gap: 24px; }}
.kicker {{ font-family: {MONO}; font-size: 17px; font-weight: 600;
  letter-spacing: 0.3em; color: {ACCENT};
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.1s both; }}
.num {{ font-family: {DISPLAY}; font-size: 290px; font-weight: 800;
  letter-spacing: -0.045em; color: {INK}; line-height: 0.9;
  animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) 0.2s both; }}
.rule {{ width: 0px; height: 5px; border-radius: 3px; background: {ACCENT};
  animation: draw 0.85s cubic-bezier(0.16,1,0.3,1) 0.5s both; }}
.title {{ font-family: {SANS}; font-size: 40px; font-weight: 400;
  color: {MUTED}; line-height: 1.2;
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.58s both; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(14px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes draw {{ from {{ width: 0px }} to {{ width: 460px }} }}
"#
    );
    let markup = r#"
<div class="wrap">
  <div class="kicker">CHAPTER THREE</div>
  <div class="num">03</div>
  <div class="rule"></div>
  <div class="title">The Machine That Draws</div>
</div>
"#;
    let mut built = comp(
        "funcut.chapter.numbered",
        "Numbered chapter",
        1920,
        1080,
        4.5,
    );
    let node = with_track(
        html_node(
            "chapter",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.25, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.965), Curve::SPRING_POP),
            Key::new(0.55, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "chapter".into(),
        field: SlotField::Text,
        sample: "CHAPTER THREE".into(),
        label: "Kicker".into(),
    });
    built.slots.push(Slot {
        node: "chapter".into(),
        field: SlotField::Text,
        sample: "03".into(),
        label: "Number".into(),
    });
    built.slots.push(Slot {
        node: "chapter".into(),
        field: SlotField::Text,
        sample: "The Machine That Draws".into(),
        label: "Title".into(),
    });
    built
}

/// An agenda: the numbered list of what is coming, one row arriving a
/// step after the one before it, hairlines between. The explainer's
/// table of contents - the promise a chapter chip only points at.
pub fn agenda_list() -> Comp {
    let css = format!(
        r#"
.wrap {{ position: absolute; left: 220px; top: 292px; width: 980px;
  display: flex; flex-direction: column; align-items: stretch; }}
.head {{ font-family: {MONO}; font-size: 17px; font-weight: 600;
  letter-spacing: 0.3em; color: {ACCENT}; padding-bottom: 26px;
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.1s both; }}
.row {{ display: flex; flex-direction: row; align-items: baseline; gap: 36px;
  padding: 25px 4px; border-bottom: 1px solid rgba(244,241,234,0.14); }}
.n {{ font-family: {MONO}; font-size: 20px; font-weight: 600; color: {ACCENT}; }}
.t {{ font-family: {DISPLAY}; font-size: 42px; font-weight: 700;
  letter-spacing: -0.01em; color: {INK}; }}
.r1 {{ animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.20s both; }}
.r2 {{ animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.32s both; }}
.r3 {{ animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.44s both; }}
.r4 {{ animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.56s both; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(12px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = r#"
<div class="wrap">
  <div class="head">WHAT IS COMING</div>
  <div class="row r1"><div class="n">01</div><div class="t">The comp document</div></div>
  <div class="row r2"><div class="n">02</div><div class="t">The looks</div></div>
  <div class="row r3"><div class="n">03</div><div class="t">The critic</div></div>
  <div class="row r4"><div class="n">04</div><div class="t">The farm</div></div>
</div>
"#;
    let mut built = comp("funcut.chapter.agenda", "Agenda list", 1920, 1080, 5.0);
    let node = with_track(
        html_node(
            "agenda",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.3, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "agenda".into(),
        field: SlotField::Text,
        sample: "WHAT IS COMING".into(),
        label: "Heading".into(),
    });
    built.slots.push(Slot {
        node: "agenda".into(),
        field: SlotField::Text,
        sample: "The comp document".into(),
        label: "Row 1".into(),
    });
    built.slots.push(Slot {
        node: "agenda".into(),
        field: SlotField::Text,
        sample: "The looks".into(),
        label: "Row 2".into(),
    });
    built.slots.push(Slot {
        node: "agenda".into(),
        field: SlotField::Text,
        sample: "The critic".into(),
        label: "Row 3".into(),
    });
    built.slots.push(Slot {
        node: "agenda".into(),
        field: SlotField::Text,
        sample: "The farm".into(),
        label: "Row 4".into(),
    });
    built
}

/// An end card: the full-frame close - a kicker, a headline with one
/// accent word, two buttons that settle one after the other, a footer.
/// The subscribe card is the mid-roll ask; this is the last frame the
/// viewer sees, and it offers the next thing rather than repeating the
/// same one louder.
pub fn end_card() -> Comp {
    let css = format!(
        r#"
.wrap {{ position: absolute; inset: 0; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: 34px; }}
.kicker {{ font-family: {MONO}; font-size: 17px; font-weight: 600;
  letter-spacing: 0.3em; color: {ACCENT};
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.1s both; }}
.head {{ font-family: {DISPLAY}; font-size: 92px; font-weight: 800;
  letter-spacing: -0.035em; color: {INK}; line-height: 1;
  animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) 0.2s both; }}
.head em {{ font-style: normal; color: {ACCENT}; }}
.row {{ display: flex; flex-direction: row; align-items: center; gap: 26px; }}
.btn {{ display: flex; flex-direction: row; align-items: center; gap: 14px;
  padding: 18px 34px; border-radius: 999px; background: rgba(12,13,16,0.92);
  border: 1.5px solid {ACCENT}; }}
.ghost {{ display: flex; flex-direction: row; align-items: center; gap: 14px;
  padding: 18px 34px; border-radius: 999px; background: rgba(12,13,16,0.92);
  border: 1.5px solid rgba(244,241,234,0.3); }}
.dot {{ width: 10px; height: 10px; border-radius: 999px; background: {ACCENT}; }}
.txt {{ font-family: {MONO}; font-size: 19px; font-weight: 700;
  letter-spacing: 0.2em; color: {INK}; }}
.b1 {{ animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.5s both; }}
.b2 {{ animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.62s both; }}
.foot {{ font-family: {MONO}; font-size: 15px; letter-spacing: 0.26em;
  color: {MUTED};
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.74s both; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(10px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = r#"
<div class="wrap">
  <div class="kicker">THANKS FOR WATCHING</div>
  <div class="head">See you <em>Friday</em>.</div>
  <div class="row">
    <div class="btn b1"><div class="dot"></div><div class="txt">WATCH NEXT</div></div>
    <div class="ghost b2"><div class="txt">SUBSCRIBE</div></div>
  </div>
  <div class="foot">FUNCUT · EPISODE 12</div>
</div>
"#;
    let mut built = comp("funcut.cta.endcard", "End card", 1920, 1080, 4.5);
    let node = with_track(
        html_node("card", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.25, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.955), Curve::SPRING_POP),
            Key::new(0.5, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "See you Friday.".into(),
        label: "Headline".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "WATCH NEXT".into(),
        label: "Button one".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "SUBSCRIBE".into(),
        label: "Button two".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "FUNCUT · EPISODE 12".into(),
        label: "Footer".into(),
    });
    built
}

/// A code diff: the second code device after the terminal - a file
/// header, a removed line in red, an added line in green, each landing
/// on a `steps()` curve a beat after the one before it. Where the
/// terminal types a session, the diff shows a change: the before/after
/// a reviewer reads.
pub fn code_diff() -> Comp {
    let css = format!(
        r#"
.win {{ position: absolute; left: 260px; top: 320px; width: 1400px;
  display: flex; flex-direction: column; border-radius: 18px; overflow: hidden;
  background: #07090d; border: 1px solid rgba(247,249,255,0.14);
  box-shadow: 0 30px 80px rgba(0,0,0,0.6); }}
.bar {{ display: flex; flex-direction: row; align-items: center; gap: 9px;
  padding: 16px 22px; background: #0d1117; border-bottom: 1px solid rgba(247,249,255,0.08); }}
.light {{ width: 13px; height: 13px; border-radius: 999px; }}
.title {{ font-family: {MONO}; font-size: 15px; letter-spacing: 0.16em;
  color: #6b7a89; margin-left: 12px; }}
.body {{ display: flex; flex-direction: column; gap: 12px; padding: 30px 34px 38px 34px; }}
.line {{ font-family: {MONO}; font-size: 25px; white-space: nowrap; }}
.ctx {{ color: #6b7a89; }}
.del {{ color: #ff7b72; }}
.add {{ color: #4ade80; }}
.meta {{ color: #6b7a89; }}
.l1 {{ animation: land 0.4s steps(18) 0.15s both; }}
.l2 {{ animation: land 0.4s steps(20) 0.65s both; }}
.l3 {{ animation: land 0.4s steps(20) 1.15s both; }}
.l4 {{ animation: land 0.35s steps(14) 1.65s both; }}
.l5 {{ animation: land 0.35s steps(16) 2.1s both; }}
@keyframes land {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}
"#
    );
    let markup = r#"
<div class="win">
  <div class="bar">
    <div class="light" style="background: #ff5f57"></div>
    <div class="light" style="background: #febc2e"></div>
    <div class="light" style="background: #28c840"></div>
    <div class="title">src/plan.rs</div>
  </div>
  <div class="body">
    <div class="line ctx l1">@@ fn whip(seconds: f64) -> Cut {</div>
    <div class="line meta l2">@@@ the sweep the whip owns, in seconds</div>
    <div class="line del l3">-     seconds: 24.0,</div>
    <div class="line add l4">+     seconds: 2.4,</div>
    <div class="line ctx l5">}</div>
  </div>
</div>
"#;
    let mut built = comp("funcut.code.diff", "Code diff", 1920, 1080, 4.5);
    let node = with_track(
        html_node("diff", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.28, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "diff".into(),
        field: SlotField::Text,
        sample: "src/plan.rs".into(),
        label: "File".into(),
    });
    built.slots.push(Slot {
        node: "diff".into(),
        field: SlotField::Text,
        sample: "-     seconds: 24.0,".into(),
        label: "Removed line".into(),
    });
    built.slots.push(Slot {
        node: "diff".into(),
        field: SlotField::Text,
        sample: "+     seconds: 2.4,".into(),
        label: "Added line".into(),
    });
    built
}

/// A progress bar: one number, one fill. The dashboard's favourite
/// motion - a number that lands while a bar grows to it - drawn as a
/// wide hairline track and one accent fill, so the read survives being
/// retimed, re-filled and re-labelled.
pub fn progress_bar() -> Comp {
    let css = format!(
        r#"
.wrap {{ position: absolute; left: 220px; top: 396px;
  display: flex; flex-direction: column; align-items: flex-start; gap: 34px; }}
.num {{ font-family: {DISPLAY}; font-size: 210px; font-weight: 800;
  letter-spacing: -0.04em; color: {INK}; line-height: 0.9; }}
.track {{ width: 1200px; height: 14px; border-radius: 999px;
  background: rgba(244,241,234,0.10); overflow: hidden; }}
.fill {{ width: 0px; height: 14px; border-radius: 999px; background: {ACCENT};
  animation: fill 1.4s cubic-bezier(0.16,1,0.3,1) 0.4s both; }}
.lab {{ font-family: {MONO}; font-size: 18px; font-weight: 500;
  letter-spacing: 0.24em; color: {MUTED};
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.75s both; }}
@keyframes fill {{ from {{ width: 0px }} to {{ width: 864px }} }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(10px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = r#"
<div class="wrap">
  <div class="num">72%</div>
  <div class="track"><div class="fill"></div></div>
  <div class="lab">OF RENDERS FINISH INSIDE CI</div>
</div>
"#;
    let mut built = comp("funcut.chart.progress", "Progress bar", 1920, 1080, 4.0);
    let node = with_track(
        html_node("bar", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.88), Curve::SPRING_POP),
            Key::new(0.6, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "bar".into(),
        field: SlotField::Text,
        sample: "72%".into(),
        label: "Number".into(),
    });
    built.slots.push(Slot {
        node: "bar".into(),
        field: SlotField::Text,
        sample: "OF RENDERS FINISH INSIDE CI".into(),
        label: "Label".into(),
    });
    built
}

// ------------------------------------------------------------ charts, drawn

/// A line chart: the stroke draws itself across the plot, a dot pops where
/// it lands, and the quarters name themselves underneath. The chart a
/// retention curve or a quarterly read is.
pub fn line_chart() -> Comp {
    // (fraction across the plot, fraction up it) per quarter.
    let points: [(f64, f64); 6] = [
        (0.05, 0.58),
        (0.24, 0.50),
        (0.43, 0.62),
        (0.62, 0.34),
        (0.80, 0.42),
        (0.99, 0.16),
    ];
    let (plot_w, plot_h) = (1600.0, 620.0);
    let at = |(fx, fy): (f64, f64)| {
        (
            40.0 + fx * (plot_w - 80.0),
            plot_h - 40.0 - fy * (plot_h - 80.0),
        )
    };
    let stroke = points
        .iter()
        .map(|point| at(*point))
        .map(|(x, y)| format!("{x:.0},{y:.0}"))
        .collect::<Vec<_>>()
        .join(" ");
    let dots = points
        .iter()
        .enumerate()
        .map(|(i, point)| {
            let (x, y) = at(*point);
            format!(
                r#"<circle class="dot" style="animation-delay: {:.2}s" cx="{x:.0}" cy="{y:.0}" r="7"/>"#,
                0.55 + i as f64 * 0.14
            )
        })
        .collect::<String>();
    let labels = points
        .iter()
        .enumerate()
        .map(|(i, point)| {
            let (x, _) = at(*point);
            format!(
                r#"<text class="lab" style="animation-delay: {:.2}s" x="{x:.0}" y="{:.0}">{}</text>"#,
                0.65 + i as f64 * 0.14,
                plot_h - 6.0,
                ["Q1", "Q2", "Q3", "Q4", "Q5", "Q6"][i]
            )
        })
        .collect::<String>();
    let grid = (1..=3)
        .map(|i| {
            let y = 40.0 + (plot_h - 80.0) * (i as f64 / 4.0);
            format!(
                r#"<line x1="40" x2="{:.0}" y1="{y:.0}" y2="{y:.0}"/>"#,
                plot_w - 40.0
            )
        })
        .collect::<String>();
    let css = format!(
        r#"
.sheet {{ position: absolute; left: 160px; top: 180px; width: 1600px; height: 820px;
  display: flex; flex-direction: column; gap: 18px; }}
.head {{ display: flex; flex-direction: row; align-items: baseline; gap: 16px; }}
.t {{ font-family: {DISPLAY}; font-size: 46px; font-weight: 800; color: {INK}; }}
.s {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.22em; color: {MUTED}; }}
.plot {{ flex: 1; }}
.plot svg {{ width: 100%; height: 100%; }}
.grid line {{ stroke: rgba(244,241,234,0.08); stroke-width: 1; }}
.line {{ fill: none; stroke: {INK}; stroke-width: 3; stroke-linejoin: round; stroke-linecap: round;
  stroke-dasharray: 3200; stroke-dashoffset: 3200;
  animation: draw 1.3s cubic-bezier(0.16,1,0.3,1) 0.35s both; }}
.dot {{ fill: {ACCENT}; opacity: 0; transform-box: fill-box; transform-origin: center;
  animation: pop 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
.lab {{ font-family: {MONO}; font-size: 17px; letter-spacing: 0.12em; fill: {MUTED};
  text-anchor: middle; opacity: 0; animation: fade 0.4s ease-out both; }}
@keyframes draw {{ to {{ stroke-dashoffset: 0 }} }}
@keyframes pop {{ from {{ opacity: 0; transform: scale(0.4) }} to {{ opacity: 1; transform: scale(1) }} }}
@keyframes fade {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}
"#
    );
    let markup = format!(
        r#"
<div class="sheet">
  <div class="head"><div class="t">Retention by quarter</div><div class="s">PERCENT RETURNING, %</div></div>
  <div class="plot"><svg viewBox="0 0 1600 620" fill="none">
    <g class="grid">{grid}</g>
    <polyline class="line" points="{stroke}"/>
    {dots}
    {labels}
  </svg></div>
</div>
"#
    );
    let mut built = comp("funcut.chart.line", "Line chart", 1920, 1080, 5.0);
    let node = with_track(
        html_node(
            "sheet",
            &markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.93), Curve::SPRING_POP),
            Key::new(0.55, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Retention by quarter".into(),
        label: "Chart title".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "PERCENT RETURNING, %".into(),
        label: "Chart subline".into(),
    });
    built
}

/// A donut chart: four arcs that fill in staggered turns around one ring,
/// the leading share in the hole, and a legend that names each slice. The
/// chart a breakdown of hours or a share of total is.
pub fn donut_chart() -> Comp {
    // (share, name): the share is percent of the whole ring.
    let slices: [(u32, &str); 4] = [
        (38, "Editing"),
        (26, "Renders"),
        (21, "Planning"),
        (15, "Waiting"),
    ];
    let rings = [
        ACCENT,
        "rgba(217,164,65,0.62)",
        "rgba(217,164,65,0.40)",
        "rgba(217,164,65,0.24)",
    ];
    let mut arcs = String::new();
    let mut turned = 0u32;
    for (i, (share, _)) in slices.iter().enumerate() {
        arcs.push_str(&format!(
            r#"<circle class="seg seg{}" style="animation-delay: {:.2}s" transform="rotate({turned} 460 310)" cx="460" cy="310" r="210" stroke-width="96" pathLength="100" stroke="{}"/>"#,
            i + 1,
            0.30 + i as f64 * 0.12,
            rings[i]
        ));
        turned += share;
    }
    let mut fills = String::new();
    for (i, (share, _)) in slices.iter().enumerate() {
        fills.push_str(&format!(
            "@keyframes fill{} {{ to {{ stroke-dasharray: {} {} }} }}\n",
            i + 1,
            share,
            100 - share
        ));
    }
    let legend = slices
        .iter()
        .enumerate()
        .map(|(i, (share, name))| {
            format!(
                r#"<div class="row" style="animation-delay: {:.2}s"><span class="chip" style="background: {}"></span><span class="name">{name}</span><span class="pct">{}%</span></div>"#,
                0.5 + i as f64 * 0.1,
                rings[i],
                share
            )
        })
        .collect::<String>();
    let css = format!(
        r#"
.sheet {{ position: absolute; left: 160px; top: 180px; width: 1600px; height: 820px;
  display: flex; flex-direction: column; gap: 18px; }}
.head {{ display: flex; flex-direction: row; align-items: baseline; gap: 16px; }}
.t {{ font-family: {DISPLAY}; font-size: 46px; font-weight: 800; color: {INK}; }}
.s {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.22em; color: {MUTED}; }}
.plot {{ flex: 1; position: relative; }}
.plot svg {{ width: 100%; height: 100%; }}
.seg {{ fill: none; stroke-dasharray: 0 100;
  animation-duration: 0.9s; animation-timing-function: cubic-bezier(0.16,1,0.3,1); animation-fill-mode: both; }}
.seg1 {{ animation-name: fill1; }}
.seg2 {{ animation-name: fill2; }}
.seg3 {{ animation-name: fill3; }}
.seg4 {{ animation-name: fill4; }}
.hole {{ font-family: {DISPLAY}; font-weight: 800; fill: {INK}; text-anchor: middle; }}
.hole-sub {{ font-family: {MONO}; font-size: 17px; letter-spacing: 0.24em; fill: {MUTED}; text-anchor: middle; }}
.legend {{ position: absolute; right: 120px; top: 220px; display: flex; flex-direction: column; gap: 30px; }}
.legend .row {{ display: flex; flex-direction: row; align-items: center; gap: 18px;
  opacity: 0; animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
.chip {{ width: 16px; height: 16px; border-radius: 5px; }}
.name {{ font-family: {DISPLAY}; font-size: 30px; font-weight: 700; color: {INK}; width: 220px; }}
.pct {{ font-family: {MONO}; font-size: 24px; color: {MUTED}; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(14px) }} to {{ opacity: 1; transform: translateY(0) }} }}
{fills}
"#
    );
    let markup = format!(
        r#"
<div class="sheet">
  <div class="head"><div class="t">Where the time goes</div><div class="s">HOURS BY STAGE, %</div></div>
  <div class="plot"><svg viewBox="0 0 1600 620" fill="none">
    <g transform="rotate(-90 460 310)">{arcs}</g>
    <text class="hole" x="460" y="330" style="font-size: 96px">38%</text>
    <text class="hole-sub" x="460" y="386">TOP SHARE</text>
  </svg>
  <div class="legend">{legend}</div>
  </div>
</div>
"#
    );
    let mut built = comp("funcut.chart.donut", "Donut chart", 1920, 1080, 5.0);
    let node = with_track(
        html_node(
            "sheet",
            &markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Where the time goes".into(),
        label: "Chart title".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "HOURS BY STAGE, %".into(),
        label: "Chart subline".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "38%".into(),
        label: "Leading share".into(),
    });
    for (share, name) in slices {
        built.slots.push(Slot {
            node: "sheet".into(),
            field: SlotField::Text,
            sample: name.into(),
            label: format!("Legend {}", name),
        });
        built.slots.push(Slot {
            node: "sheet".into(),
            field: SlotField::Text,
            sample: format!("{}%", share),
            label: format!("Legend {} share", name),
        });
    }
    built
}

/// An area chart: the fill and its outline wiped on together from the left,
/// so the shape arrives as one sheet rather than a line that colours in.
/// The chart a cumulative total or a burn-down is.
pub fn area_chart() -> Comp {
    let points: [(f64, f64); 6] = [
        (0.05, 0.55),
        (0.24, 0.48),
        (0.43, 0.60),
        (0.62, 0.34),
        (0.80, 0.44),
        (0.99, 0.14),
    ];
    let (plot_w, plot_h) = (1600.0, 620.0);
    let at = |(fx, fy): (f64, f64)| {
        (
            40.0 + fx * (plot_w - 80.0),
            plot_h - 40.0 - fy * (plot_h - 80.0),
        )
    };
    let line = points
        .iter()
        .map(|point| at(*point))
        .map(|(x, y)| format!("{x:.0},{y:.0}"))
        .collect::<Vec<_>>()
        .join(" ");
    let (_, base_y) = at((0.0, 0.0));
    let (first_x, _) = at((0.05, 0.55));
    let (last_x, _) = at((0.99, 0.14));
    let area = format!(
        "M {first_x:.0},{base_y:.0} {} {last_x:.0},{base_y:.0} Z",
        points
            .iter()
            .map(|point| at(*point))
            .map(|(x, y)| format!("L {x:.0},{y:.0}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let dots = points
        .iter()
        .enumerate()
        .map(|(i, point)| {
            let (x, y) = at(*point);
            format!(
                r#"<circle class="dot" style="animation-delay: {:.2}s" cx="{x:.0}" cy="{y:.0}" r="7"/>"#,
                1.1 + i as f64 * 0.1
            )
        })
        .collect::<String>();
    let grid = (1..=3)
        .map(|i| {
            let y = 40.0 + (plot_h - 80.0) * (i as f64 / 4.0);
            format!(
                r#"<line x1="40" x2="{:.0}" y1="{y:.0}" y2="{y:.0}"/>"#,
                plot_w - 40.0
            )
        })
        .collect::<String>();
    let css = format!(
        r#"
.sheet {{ position: absolute; left: 160px; top: 180px; width: 1600px; height: 820px;
  display: flex; flex-direction: column; gap: 18px; }}
.head {{ display: flex; flex-direction: row; align-items: baseline; gap: 16px; }}
.t {{ font-family: {DISPLAY}; font-size: 46px; font-weight: 800; color: {INK}; }}
.s {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.22em; color: {MUTED}; }}
.plot {{ flex: 1; }}
.plot svg {{ width: 100%; height: 100%; }}
.grid line {{ stroke: rgba(244,241,234,0.08); stroke-width: 1; }}
.wipe {{ transform-box: fill-box; transform-origin: left;
  animation: wipe 1.2s cubic-bezier(0.16,1,0.3,1) 0.3s both; }}
.area {{ fill: rgba(217,164,65,0.14); stroke: none; }}
.line {{ fill: none; stroke: {INK}; stroke-width: 3; stroke-linejoin: round; stroke-linecap: round; }}
.dot {{ fill: {ACCENT}; opacity: 0; transform-box: fill-box; transform-origin: center;
  animation: pop 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
@keyframes wipe {{ from {{ transform: scaleX(0) }} to {{ transform: scaleX(1) }} }}
@keyframes pop {{ from {{ opacity: 0; transform: scale(0.4) }} to {{ opacity: 1; transform: scale(1) }} }}
"#
    );
    let markup = format!(
        r#"
<div class="sheet">
  <div class="head"><div class="t">Minutes per render, per week</div><div class="s">MEAN OVER 30 DAYS</div></div>
  <div class="plot"><svg viewBox="0 0 1600 620" fill="none">
    <defs><clipPath id="reveal"><rect class="wipe" x="0" y="0" width="1600" height="620"/></clipPath></defs>
    <g class="grid">{grid}</g>
    <g clip-path="url(#reveal)">
      <path class="area" d="{area}"/>
      <polyline class="line" points="{line}"/>
    </g>
    {dots}
  </svg></div>
</div>
"#
    );
    let mut built = comp("funcut.chart.area", "Area chart", 1920, 1080, 5.0);
    let node = with_track(
        html_node(
            "sheet",
            &markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Minutes per render, per week".into(),
        label: "Chart title".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "MEAN OVER 30 DAYS".into(),
        label: "Chart subline".into(),
    });
    built
}

/// A bar race: rows ranked live, the leader's bar in full accent and the
/// rest falling away through the ramp. The chart a countdown or a weekly
/// leaderboard is.
pub fn bar_race() -> Comp {
    // (rank, name, percent): one week of seconds saved per job.
    let rows: [(u32, &str, u32); 5] = [
        (1, "Renders", 92),
        (2, "Exports", 74),
        (3, "Bakes", 61),
        (4, "Relinks", 40),
        (5, "Batches", 26),
    ];
    let bars = [
        ACCENT,
        "rgba(217,164,65,0.58)",
        "rgba(217,164,65,0.40)",
        "rgba(217,164,65,0.26)",
        "rgba(217,164,65,0.16)",
    ];
    let board = rows
        .iter()
        .enumerate()
        .map(|(i, (rank, name, pct))| {
            format!(
                r#"<div class="row" style="animation-delay: {:.2}s"><span class="rank">{rank}</span><span class="name">{name}</span><span class="track"><span class="fill" style="width: {pct}%; background: {}; animation-delay: {:.2}s"></span></span><span class="val">{pct}</span></div>"#,
                0.08 + i as f64 * 0.08,
                bars[i],
                0.35 + i as f64 * 0.08
            )
        })
        .collect::<String>();
    let css = format!(
        r#"
.board {{ position: absolute; left: 160px; top: 200px; width: 1600px; }}
.head {{ display: flex; flex-direction: row; align-items: baseline; gap: 16px; margin-bottom: 44px; }}
.t {{ font-family: {DISPLAY}; font-size: 46px; font-weight: 800; color: {INK}; }}
.s {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.22em; color: {MUTED}; }}
.row {{ display: flex; flex-direction: row; align-items: center; gap: 28px; margin-bottom: 30px;
  opacity: 0; animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
.rank {{ font-family: {MONO}; font-size: 30px; font-weight: 700; color: {MUTED}; width: 56px; }}
.name {{ font-family: {DISPLAY}; font-size: 34px; font-weight: 800; color: {INK}; width: 250px; }}
.track {{ flex: 1; height: 42px; border-radius: 21px; background: rgba(244,241,234,0.08); overflow: hidden; }}
.fill {{ display: block; height: 100%; border-radius: 21px; transform-origin: left;
  animation: grow 1.0s cubic-bezier(0.16,1,0.3,1) both; }}
.val {{ font-family: {MONO}; font-size: 24px; color: {MUTED}; width: 96px; text-align: right; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(16px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes grow {{ from {{ transform: scaleX(0) }} to {{ transform: scaleX(1) }} }}
"#
    );
    let markup = format!(
        r#"<div class="board"><div class="head"><div class="t">This week, by seconds saved</div><div class="s">MEAN PER JOB, SECONDS</div></div>{board}</div>"#
    );
    let mut built = comp("funcut.leaderboard.bar-race", "Bar race", 1920, 1080, 5.5);
    let node = with_track(
        html_node(
            "board",
            &markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.94), Curve::SPRING_POP),
            Key::new(0.55, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "This week, by seconds saved".into(),
        label: "Chart title".into(),
    });
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "MEAN PER JOB, SECONDS".into(),
        label: "Chart subline".into(),
    });
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "Renders".into(),
        label: "Leader name".into(),
    });
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "92".into(),
        label: "Leader value".into(),
    });
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "Exports".into(),
        label: "Second name".into(),
    });
    built
}

/// A comparison table: two columns and one verdict, the winning column
/// tinted and its dots in accent. The table a release note or a versus
/// card is.
pub fn comparison_table() -> Comp {
    let features: [(&str, &str, &str); 4] = [
        ("Timeline", "30 fps", "60 fps"),
        ("Trim", "keyed only", "ripples"),
        ("Grade", "8-bit", "10-bit linear"),
        ("Export", "one queue", "farms"),
    ];
    let body = features
        .iter()
        .enumerate()
        .map(|(i, (feature, was, now))| {
            format!(
                r#"<div class="feat" style="animation-delay: {:.2}s"><span class="f">{feature}</span><span class="a">● {was}</span><span class="b">● {now}</span></div>"#,
                0.35 + i as f64 * 0.1
            )
        })
        .collect::<String>();
    let css = format!(
        r#"
.sheet {{ position: absolute; left: 200px; top: 210px; width: 1520px; }}
.head {{ display: grid; grid-template-columns: 1fr 140px 1fr; align-items: center;
  padding-bottom: 26px; border-bottom: 2px solid rgba(244,241,234,0.16); }}
.product {{ font-family: {DISPLAY}; font-size: 44px; font-weight: 800; color: {INK}; }}
.product.b {{ color: {ACCENT}; text-align: right; }}
.vs {{ justify-self: center; font-family: {MONO}; font-size: 15px; letter-spacing: 0.3em;
  color: {MUTED}; border: 1px solid rgba(244,241,234,0.22); border-radius: 999px; padding: 10px 22px 10px 26px; }}
.feat {{ display: grid; grid-template-columns: 1fr 140px 1fr; align-items: center;
  padding: 30px 0; opacity: 0; animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
.f {{ font-family: {MONO}; font-size: 19px; letter-spacing: 0.18em; color: {MUTED}; }}
.a {{ font-family: {DISPLAY}; font-size: 28px; font-weight: 700; color: rgba(244,241,234,0.55); }}
.b {{ font-family: {DISPLAY}; font-size: 28px; font-weight: 700; color: {INK}; text-align: right;
  background: rgba(217,164,65,0.06); border-radius: 14px; padding: 18px 24px; margin: -18px 0; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(16px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = format!(
        r#"<div class="sheet"><div class="head"><div class="product">Cut 24.2</div><div class="vs">VS</div><div class="product b">Cut 25.1</div></div>{body}</div>"#
    );
    let mut built = comp(
        "funcut.table.comparison",
        "Comparison table",
        1920,
        1080,
        5.5,
    );
    let node = with_track(
        html_node(
            "sheet",
            &markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.95), Curve::SPRING_POP),
            Key::new(0.5, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Cut 24.2".into(),
        label: "Product A".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Cut 25.1".into(),
        label: "Product B".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Timeline".into(),
        label: "Feature 1".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Trim".into(),
        label: "Feature 2".into(),
    });
    built
}

// ------------------------------------------------- social, news, devices

/// A social quote card: an avatar, a handle, the claim, and the counts a
/// platform reads back. The card a pull-quote or a testimonial is, cut
/// for a feed.
pub fn social_quote() -> Comp {
    let css = format!(
        r#"
.card {{ position: absolute; left: 520px; top: 150px; width: 880px; padding: 56px 64px 48px 64px;
  background: rgba(12,13,16,0.92); border: 1px solid rgba(244,241,234,0.12); border-radius: 18px;
  opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) both; }}
.who {{ display: flex; flex-direction: row; align-items: center; gap: 26px; margin-bottom: 40px; }}
.avatar {{ width: 96px; height: 96px; border-radius: 999px; background: #14161a;
  border: 1px solid rgba(244,241,234,0.18); display: flex; align-items: center; justify-content: center;
  font-family: {DISPLAY}; font-size: 34px; font-weight: 800; color: {INK}; }}
.id {{ display: flex; flex-direction: column; gap: 8px; }}
.name {{ font-family: {DISPLAY}; font-size: 28px; font-weight: 700; color: {INK}; }}
.handle {{ font-family: {MONO}; font-size: 17px; letter-spacing: 0.08em; color: {ACCENT}; }}
.claim {{ font-family: {DISPLAY}; font-size: 52px; font-weight: 800; letter-spacing: -0.02em;
  line-height: 1.2; color: {INK}; margin-bottom: 44px; }}
.counts {{ display: flex; flex-direction: row; gap: 36px; padding-top: 28px;
  border-top: 1px solid rgba(244,241,234,0.12); }}
.count {{ font-family: {MONO}; font-size: 20px; color: {MUTED}; }}
.count b {{ color: {INK}; font-weight: 600; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(18px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = r#"
<div class="card">
  <div class="who">
    <div class="avatar">GH</div>
    <div class="id"><div class="name">Grace Hopper</div><div class="handle">@gracebuilds</div></div>
  </div>
  <div class="claim">The best error message is the one that never shows.</div>
  <div class="counts">
    <div class="count"><b>1.2k</b> likes</div>
    <div class="count"><b>340</b> reposts</div>
    <div class="count"><b>52</b> replies</div>
  </div>
</div>
"#;
    let mut built = comp("funcut.social.quote", "Social quote card", 1920, 1080, 5.0);
    let node = with_track(
        html_node("card", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.24, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.95), Curve::SPRING_POP),
            Key::new(0.55, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "Grace Hopper".into(),
        label: "Name".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "@gracebuilds".into(),
        label: "Handle".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "The best error message is the one that never shows.".into(),
        label: "Quote".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "1.2k".into(),
        label: "Likes".into(),
    });
    built
}

/// A social stat card: one number at poster size, the label under it, a
/// sparkline that draws, and the delta chip. The card a weekly number or a
/// launch count is.
pub fn social_stat() -> Comp {
    let css = format!(
        r#"
.card {{ position: absolute; left: 600px; top: 130px; width: 720px; height: 820px;
  background: rgba(12,13,16,0.92); border: 1px solid rgba(244,241,234,0.12); border-radius: 22px;
  padding: 52px 56px; display: flex; flex-direction: column;
  opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) both; }}
.chip {{ align-self: flex-start; display: flex; flex-direction: row; align-items: center; gap: 10px;
  font-family: {MONO}; font-size: 15px; letter-spacing: 0.28em; color: {MUTED};
  border: 1px solid rgba(244,241,234,0.18); border-radius: 999px; padding: 10px 20px; }}
.chip i {{ width: 9px; height: 9px; border-radius: 999px; background: {ACCENT}; }}
.num {{ font-family: {DISPLAY}; font-size: 190px; font-weight: 800; letter-spacing: -0.05em;
  line-height: 1; color: {INK}; margin: 60px 0 12px 0; }}
.lab {{ font-family: {MONO}; font-size: 19px; letter-spacing: 0.3em; color: {MUTED}; }}
.spark {{ margin-top: auto; margin-bottom: 36px; }}
.spark svg {{ width: 100%; height: 130px; }}
.rise {{ stroke: {ACCENT}; stroke-width: 3; fill: none; stroke-linejoin: round; stroke-linecap: round;
  stroke-dasharray: 700; stroke-dashoffset: 700; animation: draw 1.2s cubic-bezier(0.16,1,0.3,1) 0.5s both; }}
.delta {{ align-self: flex-start; font-family: {MONO}; font-size: 20px; color: {ACCENT}; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(18px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes draw {{ to {{ stroke-dashoffset: 0 }} }}
"#
    );
    let markup = r#"
<div class="card">
  <div class="chip"><i></i>WEEKLY</div>
  <div class="num">24,180</div>
  <div class="lab">RENDERS THIS WEEK</div>
  <div class="spark"><svg viewBox="0 0 600 130" fill="none">
    <polyline class="rise" points="10,96 90,88 170,92 250,70 330,74 410,52 490,56 570,30"/>
  </svg></div>
  <div class="delta">+18% week over week</div>
</div>
"#;
    let mut built = comp("funcut.social.stat", "Social stat card", 1920, 1080, 4.5);
    let node = with_track(
        html_node("card", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.24, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.94), Curve::SPRING_POP),
            Key::new(0.55, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "24,180".into(),
        label: "Number".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "RENDERS THIS WEEK".into(),
        label: "Label".into(),
    });
    built.slots.push(Slot {
        node: "card".into(),
        field: SlotField::Text,
        sample: "+18% week over week".into(),
        label: "Delta".into(),
    });
    built
}

/// A headline flash: a band across the bottom, a LIVE kicker with a dot
/// that pulses, the headline at poster weight, and a rule that draws under
/// it. The flash a bulletin or a breaking change is.
pub fn headline_flash() -> Comp {
    let css = format!(
        r#"
.band {{ position: absolute; left: 0; right: 0; bottom: 110px; padding: 44px 120px 0 120px;
  opacity: 0; animation: rise 0.55s cubic-bezier(0.16,1,0.3,1) both; }}
.rule {{ height: 3px; background: {ACCENT}; transform-origin: left;
  animation: grow 0.9s cubic-bezier(0.16,1,0.3,1) 0.2s both; }}
.row {{ display: flex; flex-direction: row; align-items: center; gap: 28px; margin: 26px 0 18px 0; }}
.live {{ display: flex; flex-direction: row; align-items: center; gap: 12px;
  font-family: {MONO}; font-size: 18px; font-weight: 700; letter-spacing: 0.34em; color: {ACCENT}; }}
.live i {{ width: 11px; height: 11px; border-radius: 999px; background: {ACCENT};
  animation: pulse 1.6s ease-in-out infinite; }}
.state {{ font-family: {MONO}; font-size: 18px; letter-spacing: 0.3em; color: {MUTED}; }}
.when {{ margin-left: auto; font-family: {MONO}; font-size: 18px; color: {MUTED}; }}
.head {{ font-family: {DISPLAY}; font-size: 84px; font-weight: 800; letter-spacing: -0.03em;
  line-height: 1.05; color: {INK}; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(26px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes grow {{ from {{ transform: scaleX(0) }} to {{ transform: scaleX(1) }} }}
@keyframes pulse {{ 0%, 100% {{ opacity: 1 }} 50% {{ opacity: 0.35 }} }}
"#
    );
    let markup = r#"
<div class="band">
  <div class="rule"></div>
  <div class="row">
    <div class="live"><i></i>LIVE</div>
    <div class="state">DEVELOPING</div>
    <div class="when">22:41 UTC</div>
  </div>
  <div class="head">The render farm now bakes while you sleep.</div>
</div>
"#;
    let mut built = comp(
        "funcut.news.headline-flash",
        "Headline flash",
        1920,
        1080,
        4.5,
    );
    let node = with_track(
        html_node("band", markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "band".into(),
        field: SlotField::Text,
        sample: "The render farm now bakes while you sleep.".into(),
        label: "Headline".into(),
    });
    built.slots.push(Slot {
        node: "band".into(),
        field: SlotField::Text,
        sample: "LIVE".into(),
        label: "Kicker".into(),
    });
    built.slots.push(Slot {
        node: "band".into(),
        field: SlotField::Text,
        sample: "22:41 UTC".into(),
        label: "Timestamp".into(),
    });
    built.slots.push(Slot {
        node: "band".into(),
        field: SlotField::Text,
        sample: "DEVELOPING".into(),
        label: "State".into(),
    });
    built
}

/// A phone frame: the device centred, an island at the top, a status bar,
/// and the screen's own words under a kicker - the frame a product shot or
/// an app tease is, without a photo of a phone.
pub fn device_phone() -> Comp {
    let css = format!(
        r#"
.stage {{ position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }}
.phone {{ width: 430px; height: 880px; border-radius: 56px; background: #0b0c0f;
  border: 1px solid rgba(244,241,234,0.20); padding: 14px; }}
.screen {{ position: relative; width: 100%; height: 100%; border-radius: 44px; background: #101216;
  overflow: hidden; display: flex; flex-direction: column; padding: 64px 34px 44px 34px; gap: 26px; }}
.island {{ position: absolute; top: 22px; left: 50%; margin-left: -61px; width: 122px; height: 34px;
  border-radius: 20px; background: #050507; opacity: 0; animation: fade 0.5s ease-out 0.5s both; }}
.bar {{ display: flex; flex-direction: row; align-items: center; }}
.time {{ font-family: {MONO}; font-size: 17px; color: {INK}; }}
.bicons {{ margin-left: auto; display: flex; flex-direction: row; gap: 7px; }}
.bicons i {{ width: 8px; height: 8px; border-radius: 999px; background: rgba(244,241,234,0.45); }}
.kicker {{ font-family: {MONO}; font-size: 15px; letter-spacing: 0.32em; color: {ACCENT}; margin-top: 26px; }}
.words {{ font-family: {DISPLAY}; font-size: 58px; font-weight: 800; letter-spacing: -0.03em;
  line-height: 1.08; color: {INK}; opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) 0.35s both; }}
.cta {{ margin-top: auto; align-self: flex-start; background: {ACCENT}; color: #101216;
  font-family: {MONO}; font-size: 17px; font-weight: 700; letter-spacing: 0.06em;
  border-radius: 999px; padding: 16px 30px; opacity: 0;
  animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) 0.55s both; }}
.home {{ position: absolute; bottom: 18px; left: 50%; margin-left: -70px; width: 140px; height: 5px;
  border-radius: 3px; background: rgba(244,241,234,0.35); }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(16px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes fade {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}
"#
    );
    let markup = r#"
<div class="stage">
  <div class="phone">
    <div class="screen">
      <div class="island"></div>
      <div class="bar"><div class="time">9:41</div><div class="bicons"><i></i><i></i><i></i></div></div>
      <div class="kicker">CONCAT MOTION</div>
      <div class="words">Bakes while you sleep.</div>
      <div class="cta">Render now</div>
      <div class="home"></div>
    </div>
  </div>
</div>
"#;
    let mut built = comp("funcut.device.phone", "Phone frame", 1920, 1080, 5.0);
    let node = with_track(
        html_node(
            "stage",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.9), Curve::SPRING_POP),
            Key::new(0.6, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "stage".into(),
        field: SlotField::Text,
        sample: "Bakes while you sleep.".into(),
        label: "Heading".into(),
    });
    built.slots.push(Slot {
        node: "stage".into(),
        field: SlotField::Text,
        sample: "9:41".into(),
        label: "Clock".into(),
    });
    built.slots.push(Slot {
        node: "stage".into(),
        field: SlotField::Text,
        sample: "Render now".into(),
        label: "Button".into(),
    });
    built
}

/// A tablet frame: a wide screen, a rail of dots down the left edge, and
/// real words on the right - the frame a dashboard tease or a release note
/// is, at the scale a tablet reads at.
pub fn device_tablet() -> Comp {
    let css = format!(
        r#"
.stage {{ position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }}
.slate {{ width: 1240px; height: 840px; border-radius: 40px; background: #0b0c0f;
  border: 1px solid rgba(244,241,234,0.20); padding: 18px; }}
.glass {{ position: relative; width: 100%; height: 100%; border-radius: 26px; background: #101216;
  overflow: hidden; display: flex; flex-direction: row; }}
.cam {{ position: absolute; top: 9px; left: 50%; margin-left: -5px; width: 10px; height: 10px;
  border-radius: 999px; background: #050507; border: 1px solid rgba(244,241,234,0.14); }}
.rail {{ width: 96px; border-right: 1px solid rgba(244,241,234,0.12); display: flex;
  flex-direction: column; align-items: center; gap: 30px; padding-top: 88px; }}
.rail i {{ width: 14px; height: 14px; border-radius: 999px; background: rgba(244,241,234,0.28);
  opacity: 0; animation: fade 0.4s ease-out both; }}
.rail i:first-child {{ background: {ACCENT}; }}
.body {{ flex: 1; padding: 88px 64px; display: flex; flex-direction: column; gap: 34px; }}
.title {{ font-family: {DISPLAY}; font-size: 64px; font-weight: 800; letter-spacing: -0.03em;
  line-height: 1.06; color: {INK}; opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) 0.3s both; }}
.line {{ font-family: {SANS}; font-size: 27px; line-height: 1.5; color: {MUTED};
  opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) both; }}
.row {{ margin-top: auto; display: flex; flex-direction: row; gap: 22px; }}
.chip {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.14em; color: {INK};
  border: 1px solid rgba(244,241,234,0.22); border-radius: 999px; padding: 15px 30px;
  opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) both; }}
.chip.gold {{ background: {ACCENT}; border-color: {ACCENT}; color: #101216; font-weight: 700; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(16px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes fade {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}
"#
    );
    let markup = r#"
<div class="stage">
  <div class="slate">
    <div class="glass">
      <div class="cam"></div>
      <div class="rail"><i></i><i></i><i></i><i></i><i></i></div>
      <div class="body">
        <div class="title">The whole board, one pass.</div>
        <div class="line" style="animation-delay: 0.45s">Parts that survive a failed run.</div>
        <div class="line" style="animation-delay: 0.6s">A cache that answers before it asks.</div>
        <div class="line" style="animation-delay: 0.75s">One queue for every engine.</div>
        <div class="row">
          <div class="chip gold" style="animation-delay: 0.95s">RENDER</div>
          <div class="chip" style="animation-delay: 1.1s">Plan first</div>
        </div>
      </div>
    </div>
  </div>
</div>
"#;
    let mut built = comp("funcut.device.tablet", "Tablet frame", 1920, 1080, 5.5);
    let node = with_track(
        html_node(
            "stage",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.93), Curve::SPRING_POP),
            Key::new(0.55, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "stage".into(),
        field: SlotField::Text,
        sample: "The whole board, one pass.".into(),
        label: "Title".into(),
    });
    built.slots.push(Slot {
        node: "stage".into(),
        field: SlotField::Text,
        sample: "RENDER".into(),
        label: "Primary chip".into(),
    });
    built
}

/// A laptop frame: a screen with a bezel, a hinge line, and a base that
/// slides home under it - the frame a keynote tease or a desktop story is,
/// drawn rather than photographed.
pub fn device_laptop() -> Comp {
    let css = format!(
        r#"
.stage {{ position: absolute; inset: 0; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: 0; }}
.lid {{ width: 1500px; border-radius: 22px 22px 0 0; background: #0b0c0f;
  border: 1px solid rgba(244,241,234,0.20); border-bottom: none; padding: 18px 18px 12px 18px;
  opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) both; }}
.glass {{ position: relative; width: 100%; height: 830px; border-radius: 10px; background: #101216;
  overflow: hidden; display: flex; flex-direction: column; align-items: center; padding-top: 120px; }}
.kicker {{ font-family: {MONO}; font-size: 16px; letter-spacing: 0.34em; color: {ACCENT}; }}
.title {{ font-family: {DISPLAY}; font-size: 78px; font-weight: 800; letter-spacing: -0.03em;
  color: {INK}; margin-top: 30px; opacity: 0; animation: rise 0.6s cubic-bezier(0.16,1,0.3,1) 0.35s both; }}
.dock {{ position: absolute; bottom: 26px; display: flex; flex-direction: row; gap: 18px; }}
.dock i {{ width: 14px; height: 14px; border-radius: 999px; background: rgba(244,241,234,0.30);
  opacity: 0; animation: fade 0.4s ease-out both; }}
.dock i:nth-child(4) {{ background: {ACCENT}; }}
.base {{ width: 1740px; height: 36px; background: #14161a;
  clip-path: polygon(3% 0, 97% 0, 100% 100%, 0 100%);
  border-top: 1px solid rgba(244,241,234,0.16);
  opacity: 0; animation: settle 0.6s cubic-bezier(0.16,1,0.3,1) 0.15s both; }}
.notch {{ width: 230px; height: 10px; background: #0a0b0e; border-radius: 0 0 8px 8px; margin: 0 auto; }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(22px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes settle {{ from {{ opacity: 0; transform: translateY(14px) }} to {{ opacity: 1; transform: translateY(0) }} }}
@keyframes fade {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}
"#
    );
    let markup = r#"
<div class="stage">
  <div class="lid">
    <div class="glass">
      <div class="kicker">CONCAT 25.1</div>
      <div class="title">One timeline, every engine.</div>
      <div class="dock"><i></i><i></i><i></i><i></i><i></i><i></i><i></i></div>
    </div>
  </div>
  <div class="base"><div class="notch"></div></div>
</div>
"#;
    let mut built = comp("funcut.device.laptop", "Laptop frame", 1920, 1080, 5.5);
    let node = with_track(
        html_node(
            "stage",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.22, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.92), Curve::SPRING_POP),
            Key::new(0.6, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "stage".into(),
        field: SlotField::Text,
        sample: "One timeline, every engine.".into(),
        label: "Heading".into(),
    });
    built.slots.push(Slot {
        node: "stage".into(),
        field: SlotField::Text,
        sample: "CONCAT 25.1".into(),
        label: "Kicker".into(),
    });
    built
}

/// A pin route map: an abstract street grid, a dashed route that draws
/// itself across it, and pins that drop where it bends - start small,
/// destination heavy. The map a journey or a rollout plan is, without a
/// map.
pub fn pin_route() -> Comp {
    let grid_v = (0..15)
        .map(|i| {
            format!(
                r#"<line x1="{}" x2="{}" y1="0" y2="760"/>"#,
                i * 107 + 20,
                i * 107 + 20
            )
        })
        .collect::<String>();
    let grid_h = (0..8)
        .map(|i| {
            format!(
                r#"<line x1="0" x2="1600" y1="{}" y2="{}"/>"#,
                i * 95 + 10,
                i * 95 + 10
            )
        })
        .collect::<String>();
    let css = format!(
        r#"
.map {{ position: absolute; left: 160px; top: 160px; width: 1600px; height: 760px; }}
.map svg {{ width: 100%; height: 100%; }}
.streets line {{ stroke: rgba(244,241,234,0.07); stroke-width: 1; }}
.blocks rect {{ fill: rgba(244,241,234,0.045); }}
.route {{ fill: none; stroke: {ACCENT}; stroke-width: 5; stroke-linecap: round;
  stroke-dasharray: 2600; stroke-dashoffset: 2600;
  animation: draw 1.6s cubic-bezier(0.16,1,0.3,1) 0.3s both; }}
.pin {{ opacity: 0; animation: drop 0.55s cubic-bezier(0.34,1.56,0.64,1) both; }}
.pin circle {{ fill: {ACCENT}; }}
.pin.home circle {{ fill: {INK}; }}
.pin.end circle {{ fill: #101216; stroke: {ACCENT}; stroke-width: 4; }}
.tag {{ position: absolute; font-family: {MONO}; font-size: 16px; letter-spacing: 0.22em;
  color: {INK}; background: rgba(12,13,16,0.92); border: 1px solid rgba(244,241,234,0.18);
  border-radius: 999px; padding: 10px 20px; opacity: 0;
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
@keyframes draw {{ to {{ stroke-dashoffset: 0 }} }}
@keyframes drop {{ from {{ opacity: 0; transform: translateY(-22px) scale(0.8) }} to {{ opacity: 1; transform: translateY(0) scale(1) }} }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(10px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = format!(
        r##"
<div class="map">
  <svg viewBox="0 0 1600 760" fill="none">
    <g class="streets">{grid_v}{grid_h}</g>
    <g class="blocks">
      <rect x="180" y="120" width="220" height="150" rx="14"/>
      <rect x="1150" y="420" width="260" height="170" rx="14"/>
      <rect x="520" y="560" width="180" height="120" rx="14"/>
    </g>
    <path class="route" d="M 140,620 C 420,540 380,300 760,300 S 1180,420 1460,180"/>
    <g class="pin home" style="animation-delay: 0.35s" transform="translate(140,620)">
      <circle r="13"/><path d="M -9,8 L 0,30 L 9,8 Z" fill="#f4f1ea"/>
    </g>
    <g class="pin" style="animation-delay: 0.95s" transform="translate(760,300)">
      <circle r="13"/><path d="M -9,8 L 0,30 L 9,8 Z" fill="#d9a441"/>
    </g>
    <g class="pin end" style="animation-delay: 1.55s" transform="translate(1460,180)">
      <circle r="19"/><path d="M -13,12 L 0,44 L 13,12 Z" fill="#d9a441"/>
    </g>
  </svg>
  <div class="tag" style="left: 6%; top: 86%; animation-delay: 0.5s">START</div>
  <div class="tag" style="left: 44%; top: 30%; animation-delay: 1.1s">BEND</div>
  <div class="tag" style="right: 4%; top: 12%; animation-delay: 1.7s">SHIP</div>
</div>
"##
    );
    let mut built = comp("funcut.map.pin-route", "Pin route map", 1920, 1080, 5.5);
    let node = with_track(
        html_node("map", &markup, &css, Length::Px(1920.0), Length::Px(1080.0)),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.95), Curve::SPRING_POP),
            Key::new(0.5, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "map".into(),
        field: SlotField::Text,
        sample: "START".into(),
        label: "Origin label".into(),
    });
    built.slots.push(Slot {
        node: "map".into(),
        field: SlotField::Text,
        sample: "SHIP".into(),
        label: "Destination label".into(),
    });
    built.slots.push(Slot {
        node: "map".into(),
        field: SlotField::Text,
        sample: "BEND".into(),
        label: "Waypoint label".into(),
    });
    built
}

/// A horizontal timeline: one rule that draws left to right, milestones
/// that pop along it, alternating labels above and below. The timeline a
/// roadmap or a retrospective is.
pub fn horizontal_timeline() -> Comp {
    let marks: [(&str, &str, &str); 5] = [
        ("01", "Open", "the board loads"),
        ("02", "Cut", "transitions own their windows"),
        ("03", "Grade", "ten-bit linear"),
        ("04", "Bake", "farms survive a failure"),
        ("05", "Ship", "one queue, every engine"),
    ];
    let spots = [280, 660, 1020, 1380, 1700];
    let mut dots = String::new();
    let mut labels = String::new();
    for (i, (era, name, note)) in marks.iter().enumerate() {
        let x = spots[i];
        let above = i % 2 == 0;
        dots.push_str(&format!(
            r#"<div class="dot{}" style="left: {x}px; animation-delay: {:.2}s"></div>"#,
            if i == 2 { " gold" } else { "" },
            0.5 + i as f64 * 0.18
        ));
        labels.push_str(&format!(
            r#"<div class="side {}" style="left: {}px; animation-delay: {:.2}s"><div class="era">{era}</div><div class="name">{name}</div><div class="note">{note}</div></div>"#,
            if above { "up" } else { "down" },
            x,
            0.62 + i as f64 * 0.18
        ));
    }
    let css = format!(
        r#"
.board {{ position: absolute; left: 0; right: 0; top: 540px; height: 0; }}
.axis {{ position: absolute; left: 160px; right: 160px; top: -1px; height: 2px;
  background: rgba(244,241,234,0.20); transform-origin: left;
  animation: grow 1.4s cubic-bezier(0.16,1,0.3,1) both; }}
.dot {{ position: absolute; top: -9px; width: 18px; height: 18px; border-radius: 999px;
  margin-left: -9px; background: #101216; border: 2px solid rgba(244,241,234,0.5);
  opacity: 0; animation: pop 0.45s cubic-bezier(0.34,1.56,0.64,1) both; }}
.dot.gold {{ background: {ACCENT}; border-color: {ACCENT}; }}
.side {{ position: absolute; width: 300px; opacity: 0;
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
.side.up {{ bottom: 34px; }}
.side.down {{ top: 34px; }}
.side .era {{ font-family: {MONO}; font-size: 15px; letter-spacing: 0.3em; color: {ACCENT}; margin-bottom: 10px; }}
.side .name {{ font-family: {DISPLAY}; font-size: 40px; font-weight: 800; color: {INK}; letter-spacing: -0.02em; }}
.side .note {{ font-family: {SANS}; font-size: 22px; color: {MUTED}; margin-top: 8px; }}
.side.down {{ text-align: left; }}
@keyframes grow {{ from {{ transform: scaleX(0) }} to {{ transform: scaleX(1) }} }}
@keyframes pop {{ from {{ opacity: 0; transform: scale(0.4) }} to {{ opacity: 1; transform: scale(1) }} }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(14px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = format!(r#"<div class="board"><div class="axis"></div>{dots}{labels}</div>"#);
    let mut built = comp(
        "funcut.timeline.horizontal",
        "Horizontal timeline",
        1920,
        1080,
        5.5,
    );
    let node = with_track(
        html_node(
            "board",
            &markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.97), Curve::SPRING_POP),
            Key::new(0.5, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "Open".into(),
        label: "First milestone".into(),
    });
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "Ship".into(),
        label: "Last milestone".into(),
    });
    built.slots.push(Slot {
        node: "board".into(),
        field: SlotField::Text,
        sample: "one queue, every engine".into(),
        label: "Last note".into(),
    });
    built
}

/// A flowchart: boxes that rise, connectors that draw themselves between
/// them, the path to the outcome in accent. The diagram a pipeline or a
/// decision tree is.
pub fn flowchart() -> Comp {
    let css = format!(
        r#"
.sheet {{ position: absolute; left: 160px; top: 150px; width: 1600px; height: 780px; }}
.sheet svg {{ position: absolute; inset: 0; width: 100%; height: 100%; }}
.wire {{ fill: none; stroke: rgba(244,241,234,0.28); stroke-width: 2;
  stroke-dasharray: 700; stroke-dashoffset: 700;
  animation: draw 0.8s cubic-bezier(0.16,1,0.3,1) both; }}
.wire.gold {{ stroke: {ACCENT}; stroke-width: 3; }}
.box {{ position: absolute; background: rgba(12,13,16,0.92); border: 1px solid rgba(244,241,234,0.16);
  border-radius: 14px; padding: 24px 34px; opacity: 0;
  animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) both; }}
.box .step {{ font-family: {MONO}; font-size: 13px; letter-spacing: 0.3em; color: {MUTED}; margin-bottom: 8px; }}
.box .what {{ font-family: {DISPLAY}; font-size: 34px; font-weight: 800; color: {INK}; }}
.box.end {{ border-color: {ACCENT}; }}
.box.end .what {{ color: {ACCENT}; }}
@keyframes draw {{ to {{ stroke-dashoffset: 0 }} }}
@keyframes rise {{ from {{ opacity: 0; transform: translateY(16px) }} to {{ opacity: 1; transform: translateY(0) }} }}
"#
    );
    let markup = r#"
<div class="sheet">
  <svg viewBox="0 0 1600 780" fill="none">
    <path class="wire" style="animation-delay: 0.55s" d="M 800,96 V 220 M 800,220 H 400 V 320"/>
    <path class="wire" style="animation-delay: 0.55s" d="M 800,220 H 1200 V 320"/>
    <path class="wire gold" style="animation-delay: 1.0s" d="M 400,470 V 560 H 800"/>
    <path class="wire gold" style="animation-delay: 1.0s" d="M 1200,470 V 560 H 800"/>
    <path class="wire" style="animation-delay: 0.3s" d="M 800,96 V 220"/>
  </svg>
  <div class="box" style="left: 660px; top: 30px; animation-delay: 0.1s">
    <div class="step">STEP 00</div><div class="what">Shot</div>
  </div>
  <div class="box" style="left: 260px; top: 320px; animation-delay: 0.35s">
    <div class="step">STEP 01</div><div class="what">Plan</div>
  </div>
  <div class="box" style="left: 1060px; top: 320px; animation-delay: 0.45s">
    <div class="step">STEP 02</div><div class="what">Grade</div>
  </div>
  <div class="box end" style="left: 660px; top: 560px; animation-delay: 0.75s">
    <div class="step">OUTCOME</div><div class="what">Reel</div>
  </div>
</div>
"#;
    let mut built = comp("funcut.diagram.flowchart", "Flowchart", 1920, 1080, 5.5);
    let node = with_track(
        html_node(
            "sheet",
            markup,
            &css,
            Length::Px(1920.0),
            Length::Px(1080.0),
        ),
        "opacity",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.2, Value::number(1.0)),
        ]),
    );
    let node = with_track(
        node,
        "scale",
        Track::new(vec![
            Key::eased(0.0, Value::number(0.96), Curve::SPRING_POP),
            Key::new(0.5, Value::number(1.0)),
        ]),
    );
    built.nodes.push(node);
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Reel".into(),
        label: "Outcome".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Plan".into(),
        label: "Step one".into(),
    });
    built.slots.push(Slot {
        node: "sheet".into(),
        field: SlotField::Text,
        sample: "Grade".into(),
        label: "Step two".into(),
    });
    built
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_builds_and_validates() {
        for preset in ALL {
            let comp = build(preset.id).unwrap_or_else(|| panic!("{} builds", preset.id));
            let problems = comp.validate();
            assert!(problems.is_empty(), "{}: {problems:?}", preset.id);
            assert_eq!(comp.id, preset.id);
            // Every Geneva preset is markup; the caption row is a text node
            // with word timing, and the 3D showcase is native geometry.
            if preset.category == Category::Captions
                || preset.engine == crate::engine::EngineKind::Native
            {
                assert!(!comp.needs_markup(), "{} is native", preset.id);
            } else {
                assert!(comp.needs_markup(), "{} carries markup", preset.id);
            }
        }
    }

    /// A slot that cannot be filled is a trap: it either edits the wrong
    /// words or nothing at all. Every shipped template is filled here, so an
    /// agent never discovers this at 2 a.m.
    #[test]
    fn every_slot_can_be_filled() {
        for preset in ALL {
            let comp = build(preset.id).expect("a preset");
            for slot in &comp.slots {
                let value = "A VERY FILLED SLOT";
                let filled = crate::authoring::apply(
                    &comp,
                    &[crate::authoring::Op::SetSlot {
                        slot: slot.label.clone(),
                        value: value.into(),
                    }],
                )
                .unwrap_or_else(|error| panic!("{}: {:?}: {error}", preset.id, slot.label));
                let node = filled.node(&slot.node).expect("the node");
                match &node.kind {
                    NodeKind::Html { markup, .. } => assert!(
                        markup.contains(value),
                        "{}: the slot {:?} filled nothing: {markup}",
                        preset.id,
                        slot.label
                    ),
                    NodeKind::Text(text) => assert_eq!(text.text, value),
                    _ => panic!(
                        "{}: the slot {:?} is on a node of another kind",
                        preset.id, slot.label
                    ),
                }
            }
        }
    }

    #[test]
    fn an_unknown_preset_is_none_and_not_a_panic() {
        assert!(build("nope").is_none());
    }

    #[test]
    fn the_library_has_a_preset_for_every_category() {
        for category in [
            Category::LowerThird,
            Category::Captions,
            Category::Stats,
            Category::Sting,
            Category::Quote,
            Category::Chapter,
            Category::CallToAction,
            Category::Code,
            Category::Chart,
            Category::Compare,
            Category::Backdrop,
            Category::Leaderboard,
            Category::Table,
            Category::Social,
            Category::News,
            Category::Device,
            Category::Map,
            Category::Timeline,
            Category::Diagram,
        ] {
            assert!(
                ALL.iter().any(|preset| preset.category == category),
                "{category:?}"
            );
        }
    }

    #[test]
    fn a_caption_preset_carries_word_timing_an_engine_can_use() {
        let comp = kinetic_caption();
        let node = comp.node("caption").expect("the caption node");
        match &node.kind {
            NodeKind::Text(text) => {
                assert_eq!(text.words.len(), 6);
                assert!(text.highlight.is_some());
                assert_eq!(crate::comp::Node::current_word(text, 1.0, 0.0), Some(2));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn presets_carry_tracks_that_sample_inside_their_span() {
        let comp = lower_third();
        let node = comp.node("card").expect("the card node");
        let position = node
            .tracks
            .get("position")
            .expect("an entrance track")
            .sample(1.0)
            .expect("samples");
        assert_eq!(
            position,
            Value::Point { x: 960.0, y: 540.0 },
            "the card lands centred"
        );
    }
}
