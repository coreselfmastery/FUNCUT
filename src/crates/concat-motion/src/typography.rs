// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Kinetic typography: the headline that flexes, the caption that follows
//! the speech, built as one markup node a comp can place.
//!
//! The engine already plays `@keyframes` exactly as written, animates
//! `font-variation-settings`, and times an `animation-delay` to the
//! millisecond. What a comp author did not have was the *builder* that
//! turns those facts into a node: a headline whose words arrive one after
//! another, a variable-weight flex from 100 to 900, a caption whose every
//! word lights when it is spoken. Hand-authoring the spans and their
//! delays is exactly the kind of arithmetic a model gets wrong and a
//! human will not do twice.
//!
//! Everything here emits CSS the engine can play and the
//! static-transform guard can read: every `translate`/`scale` lives
//! inside a `@keyframes` rule, never as a base style.
//!
//! ```no_run
//! # fn main() {
//! use concat_motion::comp::{Node, TextNode, Word};
//! use concat_motion::typography::{Entrance, Kinetic, Stagger};
//!
//! // A headline: three words, eighty milliseconds apart, weight flexing.
//! let kinetic = Kinetic::words()
//!     .delay(0.08)
//!     .entrance(Entrance::BlurIn)
//!     .flexing_weight(100, 900);
//! let style = TextNode { font: "Archivo, Inter".into(), size: 132.0, ..TextNode::default() };
//! let node = concat_motion::typography::headline("Make it move", "headline", &style, &kinetic);
//! assert!(matches!(node.kind, concat_motion::comp::NodeKind::Html { .. }));
//!
//! // A caption: the words a recogniser timed, each lit when it is spoken.
//! let words = vec![
//!     Word { text: "Make".into(), start: 0.0, end: Some(0.45) },
//!     Word { text: "it".into(), start: 0.45, end: Some(0.85) },
//!     Word { text: "move.".into(), start: 0.85, end: None },
//! ];
//! let caption = concat_motion::typography::karaoke(&words, 2.4, "caption", &style, &kinetic);
//! # let _ = caption;
//! # }
//! ```

use serde::{Deserialize, Serialize};

use crate::comp::{Length, Node, NodeKind, Stroke, TextNode, Word};
use crate::curve::Curve;

/// What the stagger walks: whole words, or single characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stagger {
    /// One span per word: the headline rhythm everyone reads first.
    Word,
    /// One span per character: typewriters, counters, mechanical reveals.
    Char,
}

/// The three entrances every motion designer reaches for first, as named
/// presets rather than hand-written keyframes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Entrance {
    /// Rises out of a blur: the soft, filmic arrival.
    BlurIn,
    /// Rises from below: the workhorse of titles and captions.
    SlideUp,
    /// Lands from a smaller scale: the confident, placed arrival.
    ScaleIn,
}

/// A variable-weight flex, as the two ends of `font-variation-settings`
/// the headline travels between. Needs a variable font face carried as a
/// `font` asset - the library's `Archivo-Variable.ttf` and
/// `Inter-Variable.ttf` are exactly that.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct WeightFlex {
    /// The weight it starts at, 100 to 900.
    pub from: u32,
    /// The weight it flexes to, 100 to 900.
    pub to: u32,
}

/// How a piece of kinetic type is built.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Kinetic {
    /// What the stagger walks.
    pub stagger: Stagger,
    /// The delay between one unit and the next, in seconds.
    pub delay: f64,
    /// The named entrance every unit plays.
    pub entrance: Entrance,
    /// How long the entrance takes, in seconds.
    pub seconds: f64,
    /// How the entrance travels.
    pub curve: Curve,
    /// The variable-weight flex the whole line plays, when it plays one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<WeightFlex>,
    /// How long the weight flex takes, in seconds.
    pub flex_seconds: f64,
}

impl Default for Kinetic {
    fn default() -> Self {
        Kinetic {
            stagger: Stagger::Word,
            delay: 0.08,
            entrance: Entrance::SlideUp,
            seconds: 0.5,
            curve: Curve::EMPHASIS,
            weight: None,
            flex_seconds: 0.9,
        }
    }
}

impl Kinetic {
    /// The library's word-staggered default: 80 ms apart, sliding up on
    /// the settle curve.
    pub fn words() -> Kinetic {
        Kinetic::default()
    }

    /// Character-staggered, for typewriters and mechanical reveals.
    pub fn chars() -> Kinetic {
        Kinetic {
            stagger: Stagger::Char,
            delay: 0.045,
            ..Kinetic::default()
        }
    }

    /// A typewriter: character by character, each held and jumped by
    /// `steps(8)` - the reveal arriving in clacks rather than a slide.
    /// This is the easing library's `Curve::TYPEWRITER` spent where it
    /// was born to be.
    pub fn typewriter() -> Kinetic {
        Kinetic {
            stagger: Stagger::Char,
            delay: 0.045,
            seconds: 0.4,
            curve: Curve::TYPEWRITER,
            ..Kinetic::default()
        }
    }

    /// Set the delay between units.
    pub fn delay(mut self, seconds: f64) -> Kinetic {
        self.delay = seconds.max(0.0);
        self
    }

    /// Set the named entrance.
    pub fn entrance(mut self, entrance: Entrance) -> Kinetic {
        self.entrance = entrance;
        self
    }

    /// Set how the entrance travels.
    pub fn curved(mut self, curve: Curve) -> Kinetic {
        self.curve = curve;
        self
    }

    /// Flex the variable font's weight from `from` to `to` over the line.
    pub fn flexing_weight(mut self, from: u32, to: u32) -> Kinetic {
        self.weight = Some(WeightFlex {
            from: from.clamp(100, 900),
            to: to.clamp(100, 900),
        });
        self
    }
}

/// A headline as one markup node: the units staggered `kinetic.delay`
/// apart on `kinetic.entrance`, the whole line optionally flexing its
/// variable weight.
///
/// The node's box is content-sized; place it with its
/// [`crate::comp::Transform`] the way any text node is placed.
pub fn headline(id: &str, text: &str, style: &TextNode, kinetic: &Kinetic) -> Node {
    let units = units_of(text, kinetic.stagger);
    let spans = staggered_spans(&units, kinetic, "kt-in");
    let markup = format!(
        "<div class=\"kt\"{}>{}</div>",
        flex_attribute(kinetic),
        spans
    );
    let mut css = stylesheet(style) + &entrance_keyframes(kinetic);
    css.push_str(&flex_keyframes(kinetic));
    html_node(id, &markup, &css)
}

/// A word-synced caption as one markup node: every word a span whose one
/// animation appears it, holds it lit while it is spoken, and settles it
/// back - the karaoke pattern, timed from the recogniser's own word
/// times instead of an author's guess.
///
/// `span` is how long the caption node runs, which is what an open-ended
/// last word (`end: None`) is closed at.
pub fn karaoke(words: &[Word], span: f64, id: &str, style: &TextNode, kinetic: &Kinetic) -> Node {
    let mut spans = String::new();
    for word in words {
        // An open-ended word holds until the caption's own end, the same
        // rule the timeline's text source applies.
        let end = word.end.unwrap_or(span.max(word.start + 0.1));
        let spoken = (end - word.start).max(0.05);
        // The word arrives at its spoken moment, lit, and holds the lit
        // colour for most of its own length - the karaoke highlight is a
        // colour trip inside the one animation, not an effect stacked on
        // top of the entrance.
        spans.push_str(&format!(
            "<span style=\"animation: kk-word {:.3}s {} {:.3}s both\">{}</span>",
            spoken,
            kinetic.curve.css(),
            word.start.max(0.0),
            escape(&word.text)
        ));
    }
    let markup = format!(
        "<div class=\"kt kk\"{}>{}</div>",
        flex_attribute(kinetic),
        spans
    );
    let mut css = stylesheet(style) + &karaoke_keyframes(style) + &entrance_keyframes(kinetic);
    css.push_str(&flex_keyframes(kinetic));
    html_node(id, &markup, &css)
}

// ------------------------------------------------------------- the builders

/// The units a stagger walks: the words of the text, or its characters.
fn units_of(text: &str, stagger: Stagger) -> Vec<String> {
    match stagger {
        Stagger::Word => text
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        Stagger::Char => text
            .chars()
            .filter(|character| !character.is_whitespace())
            .map(|character| character.to_string())
            .collect::<Vec<_>>(),
    }
}

/// One span per unit, each carrying its own animation with its own
/// delay: `i * delay`, written inline so nothing depends on `calc()`,
/// custom properties or cascade order the engine may not share.
fn staggered_spans(units: &[String], kinetic: &Kinetic, rule: &str) -> String {
    let mut spans = String::new();
    for (index, unit) in units.iter().enumerate() {
        spans.push_str(&format!(
            "<span style=\"animation: {} {:.3}s {} {:.3}s both\">{}</span>",
            rule,
            kinetic.seconds,
            kinetic.curve.css(),
            index as f64 * kinetic.delay,
            escape(unit)
        ));
    }
    spans
}

/// The wrapper's variable-weight attribute, when there is a flex: the
/// whole line flexes together while its units stagger in beneath it.
fn flex_attribute(kinetic: &Kinetic) -> String {
    match kinetic.weight {
        Some(_) => format!(
            " style=\"animation: kt-flex {:.3}s {} both\"",
            kinetic.flex_seconds,
            kinetic.curve.css()
        ),
        None => String::new(),
    }
}

/// The variable-weight flex as its `@keyframes` rule:
/// `font-variation-settings` animated from one weight to the other, the
/// one property a variable font carries that a static face cannot.
fn flex_keyframes(kinetic: &Kinetic) -> String {
    match kinetic.weight {
        Some(WeightFlex { from, to }) => format!(
            "@keyframes kt-flex {{ from {{ font-variation-settings: 'wght' {from} }} \
             to {{ font-variation-settings: 'wght' {to} }} }}\n"
        ),
        None => String::new(),
    }
}

/// The shared base stylesheet: the line's own type, in the style a text
/// node would have drawn it with.
fn stylesheet(style: &TextNode) -> String {
    let mut css = format!(
        ".kt {{ display: flex; flex-wrap: wrap; align-items: baseline;\n\
         \x20 gap: 0.26em; font-family: {}; font-size: {:.1}px; font-weight: {};\n\
         \x20 letter-spacing: {:.2}px; line-height: {:.2}; color: {}; text-align: {}; }}\n",
        font_stack(style),
        style.size,
        style.weight,
        style.letter_spacing,
        style.line_height,
        style.color.to_hex(),
        match style.align {
            crate::comp::TextAlign::Left => "left",
            crate::comp::TextAlign::Center => "center",
            crate::comp::TextAlign::Right => "right",
        },
    );
    if let Some(shadow) = &style.shadow {
        css.push_str(&format!(
            ".kt {{ text-shadow: {}px {}px {}px {}; }}\n",
            shadow.x,
            shadow.y,
            shadow.blur,
            shadow.color.to_hex()
        ));
    }
    if let Some(Stroke { color, width }) = &style.outline {
        // A text stroke, the CSS way: the glyphs painted with their own
        // colour behind the plate colour.
        css.push_str(&format!(
            ".kt span {{ -webkit-text-stroke: {:.1}px {}; }}\n",
            width,
            color.to_hex()
        ));
    }
    css
}

/// The entrance a stagger plays, as its `@keyframes` rule. The transform
/// is the individual `translate`/`scale`, inside the rule where the
/// engine animates it - the one place it belongs.
fn entrance_keyframes(kinetic: &Kinetic) -> String {
    let body = match kinetic.entrance {
        Entrance::BlurIn => {
            "from { opacity: 0; filter: blur(14px); translate: 0 0.3em }\n\
                             to { opacity: 1; filter: blur(0px); translate: 0 0 }"
        }
        Entrance::SlideUp => {
            "from { opacity: 0; translate: 0 0.4em }\n\
                              to { opacity: 1; translate: 0 0 }"
        }
        Entrance::ScaleIn => {
            "from { opacity: 0; scale: 0.84 }\n\
                              to { opacity: 1; scale: 1 }"
        }
    };
    format!("@keyframes kt-in {{ {body} }}\n")
}

/// The karaoke word's one animation: it appears lit at its spoken moment,
/// holds the lit colour for most of its own length, and settles back to
/// the base colour - the highlight is a data-driven colour trip, not an
/// effect stacked on top.
fn karaoke_keyframes(style: &TextNode) -> String {
    let spoken = style
        .highlight
        .as_ref()
        .and_then(|highlight| highlight.color)
        .map(|color| color.to_hex())
        .unwrap_or_else(|| crate::presets::ACCENT.to_owned());
    let pop = style
        .highlight
        .as_ref()
        .and_then(|highlight| highlight.weight)
        .unwrap_or(style.weight.max(400) + 200);
    format!(
        "@keyframes kk-word {{\n\
         \x20 0% {{ opacity: 0; translate: 0 0.3em; color: {spoken}; font-weight: {pop} }}\n\
         \x20 12% {{ opacity: 1; translate: 0 0; color: {spoken}; font-weight: {pop} }}\n\
         \x20 75% {{ color: {spoken}; font-weight: {pop} }}\n\
         \x20 100% {{ opacity: 1; color: {base}; font-weight: {base_weight} }}\n\
         }}\n",
        base = style.color.to_hex(),
        base_weight = style.weight,
    )
}

/// A markup node whose box is its content.
fn html_node(id: &str, markup: &str, css: &str) -> Node {
    Node {
        id: id.into(),
        parent: None,
        start: 0.0,
        duration: 0.0,
        transform: crate::comp::Transform::default(),
        opacity: 1.0,
        blend: crate::comp::Blend::Normal,
        mask: None,
        effects: Vec::new(),
        tracks: std::collections::BTreeMap::new(),
        kind: NodeKind::Html {
            markup: markup.into(),
            css: css.into(),
            width: Length::Auto,
            height: Length::Auto,
        },
    }
}

/// The font stack a text node names, with the library's fallbacks behind
/// it so a machine without the face still draws something in its place.
fn font_stack(style: &TextNode) -> String {
    if style.font.is_empty() {
        crate::presets::SANS.to_owned()
    } else {
        style.font.clone()
    }
}

/// Text as HTML: a transcript's words can carry the three characters HTML
/// reads.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{NodeKind, Rgba, TextAlign};

    fn style() -> TextNode {
        TextNode {
            text: String::new(),
            words: Vec::new(),
            highlight: Some(crate::comp::TextStyle {
                color: Some(Rgba::hex("#d9a441")),
                fill: None,
                weight: Some(800),
                scale: None,
            }),
            font: "Archivo, Inter".into(),
            size: 96.0,
            weight: 700,
            italic: false,
            letter_spacing: -1.0,
            line_height: 1.1,
            align: TextAlign::Center,
            max_width: None,
            padding: 0.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            radius: 0.0,
            color: Rgba::hex("#f4f1ea"),
            fill: None,
            outline: None,
            shadow: None,
        }
    }

    fn css_of(node: &Node) -> String {
        match &node.kind {
            NodeKind::Html { css, .. } => css.clone(),
            other => panic!("expected markup, got {other:?}"),
        }
    }

    fn markup_of(node: &Node) -> String {
        match &node.kind {
            NodeKind::Html { markup, .. } => markup.clone(),
            other => panic!("expected markup, got {other:?}"),
        }
    }

    /// The done-when of the stagger: three words, eighty milliseconds
    /// apart, each delay written on its own span.
    #[test]
    fn a_headline_staggers_its_words_eighty_milliseconds_apart() {
        let kinetic = Kinetic::words().delay(0.08);
        let node = headline("headline", "Make it move", &style(), &kinetic);
        let markup = markup_of(&node);
        assert_eq!(markup.matches("<span").count(), 3, "{markup}");
        assert!(markup.contains("0.000s both"), "{markup}");
        assert!(markup.contains("0.080s both"), "{markup}");
        assert!(markup.contains("0.160s both"), "{markup}");
    }

    /// Character stagger walks the characters, not the words.
    #[test]
    fn a_char_stagger_walks_the_characters() {
        let node = headline("headline", "Go", &style(), &Kinetic::chars());
        assert_eq!(markup_of(&node).matches("<span").count(), 2);
    }

    /// The variable-weight flex: `font-variation-settings` animated
    /// inside `@keyframes`, on the wrapper the units stagger beneath.
    #[test]
    fn a_headline_flexes_its_variable_weight_from_100_to_900() {
        let kinetic = Kinetic::words().flexing_weight(100, 900);
        let node = headline("headline", "Make", &style(), &kinetic);
        let css = css_of(&node);
        assert!(css.contains("@keyframes kt-flex"), "{css}");
        assert!(css.contains("font-variation-settings: 'wght' 100"), "{css}");
        assert!(css.contains("font-variation-settings: 'wght' 900"), "{css}");
        let markup = markup_of(&node);
        assert!(
            markup.contains("animation: kt-flex"),
            "the wrapper carries the flex: {markup}"
        );
    }

    /// All three entrances emit their own keyframes, and every transform
    /// they carry is inside the rule - the static-transform guard is run
    /// over the generated stylesheet in its own test below.
    #[test]
    fn every_entrance_emits_its_keyframes() {
        for entrance in [Entrance::BlurIn, Entrance::SlideUp, Entrance::ScaleIn] {
            let kinetic = Kinetic::words().entrance(entrance);
            let node = headline("headline", "Word", &style(), &kinetic);
            let css = css_of(&node);
            assert!(
                css.contains("@keyframes kt-in"),
                "{}: {css}",
                entrance_debug(entrance)
            );
            let expected = match entrance {
                Entrance::BlurIn => "filter: blur(14px)",
                Entrance::SlideUp => "translate: 0 0.4em",
                Entrance::ScaleIn => "scale: 0.84",
            };
            assert!(css.contains(expected), "{:?}: {css}", entrance);
        }
    }

    /// The karaoke caption: every word a span whose animation is delayed
    /// to the word's own spoken moment and sized to its own length - the
    /// Whisper timestamps wired straight into the CSS.
    #[test]
    fn a_karaoke_caption_lights_each_word_when_it_is_spoken() {
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
                end: None,
            },
        ];
        let kinetic = Kinetic::words();
        let node = karaoke(&words, 1.6, "caption", &style(), &kinetic);
        let markup = markup_of(&node);
        // Each word appears exactly when it is spoken, held for exactly
        // its own length (the last word closes at the caption's span).
        assert!(markup.contains("animation: kk-word 0.450s"), "{markup}");
        assert!(markup.contains("0.000s both"), "{markup}");
        assert!(markup.contains("animation: kk-word 0.400s"), "{markup}");
        assert!(markup.contains("0.450s both"), "{markup}");
        assert!(markup.contains("animation: kk-word 0.750s"), "{markup}");
        assert!(markup.contains("0.850s both"), "{markup}");
        // And the highlight is the style's own: accent colour, weight 800.
        let css = css_of(&node);
        assert!(css.contains("#d9a441"), "{css}");
        assert!(css.contains("font-weight: 800"), "{css}");
    }

    /// A transcript's words can carry the characters HTML reads.
    #[test]
    fn a_transcripts_ampersand_survives_into_the_markup() {
        let words = vec![Word {
            text: "R&D".into(),
            start: 0.0,
            end: Some(0.4),
        }];
        let node = karaoke(&words, 1.0, "caption", &style(), &Kinetic::words());
        assert!(markup_of(&node).contains("R&amp;D"));
    }

    /// Everything this module writes passes its own guard: no static
    /// transform anywhere in the generated stylesheets.
    #[test]
    fn the_generated_stylesheets_carry_no_static_transform() {
        let kinetic = Kinetic::words().flexing_weight(100, 900);
        let headline_node = headline("headline", "Make it move", &style(), &kinetic);
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
        ];
        let caption_node = karaoke(&words, 1.5, "caption", &style(), &kinetic);
        for (name, node) in [("headline", &headline_node), ("caption", &caption_node)] {
            let css = css_of(node);
            assert!(
                crate::css::static_transforms(&css).is_empty(),
                "{name}: a static transform in {css}"
            );
            let markup = markup_of(node);
            for block in markup_blocks(&markup) {
                assert!(
                    crate::css::static_transforms(&block).is_empty(),
                    "{name}: a static transform in the markup"
                );
            }
        }
    }

    /// The whole scene the step is done by: a variable-weight headline,
    /// three staggered words, a karaoke caption from a transcript - one
    /// comp that validates clean and maps onto a timeline.
    #[test]
    fn the_designed_moment_validates_and_maps() {
        let mut comp = crate::comp::Comp {
            format: crate::comp::FORMAT,
            id: "funcut.typography.moment".into(),
            name: "The designed moment".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 3.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: std::collections::BTreeMap::new(),
            curves: std::collections::BTreeMap::new(),
            nodes: Vec::new(),
            slots: Vec::new(),
        };
        let kinetic = Kinetic::words().delay(0.08).entrance(Entrance::BlurIn);
        let mut headline_node = headline("headline", "Make it move", &style(), &kinetic);
        headline_node.transform.position = Some(crate::comp::Point::new(960.0, 420.0));
        comp.nodes.push(headline_node);
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
        let mut caption_node = karaoke(&words, 2.4, "caption", &style(), &kinetic);
        caption_node.transform.position = Some(crate::comp::Point::new(960.0, 900.0));
        comp.nodes.push(caption_node);
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());
        let (document, warnings) = crate::geneva::Geneva::at("geneva")
            .timeline(&comp, &crate::engine::BakeRequest::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        let layers = document["layers"].as_array().expect("layers");
        assert_eq!(layers.len(), 2, "one layer per node");
    }

    fn markup_blocks(markup: &str) -> Vec<String> {
        vec![markup.to_owned()]
    }

    fn entrance_debug(entrance: Entrance) -> String {
        format!("{entrance:?}")
    }
}
