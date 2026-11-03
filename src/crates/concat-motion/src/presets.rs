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
        }
    }
}

/// Every preset, in library order.
pub const ALL: [Preset; 18] = [
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
        _ => None,
    }
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
            // Every preset but the caption row is markup; the caption is a
            // text node with word timing, which both engines draw.
            if preset.category == Category::Captions {
                assert!(!comp.needs_markup());
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
