// SPDX-License-Identifier: AGPL-3.0-or-later
// FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The style: one layout, three personalities.
//!
//! A layout template says WHERE things sit - the stat card's three
//! columns, the lower third's two lines - and the presets are that. A
//! style says HOW the same layout feels: the weight the type carries,
//! how tightly it tracks, how fast it moves and how it settles, and the
//! colour the whole scene is graded. Same card, three moods:
//!
//! ```text
//! calm       500-weight type, open tracking, a slow settle, a cool wash
//! punchy     900-weight type, tight tracking, an overshoot, a hot grade
//! editorial  600-weight type, wide tracking, an even hand, a crisp neutral
//! ```
//!
//! The style is a restyle, not a rewrite: it swaps the weights, the
//! tracking, the timing tokens and the tempo of the comp's own
//! stylesheet in place, and the easing curves of the comp's own
//! keyframes, and leaves the layout - the geometry, the structure, the
//! words - exactly as the template wrote it. The colour it wears comes
//! back as a [`Grade`] the plan bakes when the scene names no grade of
//! its own, so a scene can take the personality's colour or override
//! it with the brand's.
//!
//! Nothing a style does is a guess: `calm` cannot turn a typewriter's
//! `steps()` into a glide or a ticker's `linear` count into a bounce -
//! the mechanical timings keep their meaning, and only the eased
//! segments take the personality. A spring keeps its spring: an author
//! who reached for one meant it.

use crate::comp::{Comp, NodeKind};
use crate::curve::Curve;
use crate::grade::{Grade, Temperature};

/// The three personalities a scene can wear over any layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style {
    /// The unhurried read: lighter type in the open, a slow settle into
    /// place, a cool wash over the plate. The explainer's voice.
    Calm,
    /// The hit: the heaviest type, tracked tight, arriving with an
    /// overshoot on a hot, contrasty grade. The trailer's voice.
    Punchy,
    /// The measured hand: medium type, tracked wide, moving on an even
    /// curve under a crisp neutral grade. The report's voice.
    Editorial,
}

impl Style {
    /// The grade the personality wears, for a scene that names no grade
    /// of its own: the colour treatment half of the style.
    pub fn grade(&self) -> Grade {
        match self {
            Style::Calm => Grade {
                temperature: Temperature::Cool,
                amount: 0.6,
                ..Grade::at(Temperature::Cool)
            },
            Style::Punchy => Grade::at(Temperature::Hot),
            Style::Editorial => Grade {
                temperature: Temperature::Balanced,
                contrast: Some(1.06),
                ..Grade::at(Temperature::Balanced)
            },
        }
    }

    /// The easing the personality gives every eased segment: the
    /// default hand the layout's own curves are replaced with.
    fn curve(&self) -> Curve {
        match self {
            // The long settle the presets themselves favour, kept and
            // stated: out fast, in forever.
            Style::Calm => Curve::EASE_OUT,
            // An overshoot past the landing and back: the arrival you
            // feel in the chest.
            Style::Punchy => Curve::Bezier {
                x1: 0.3,
                y1: 1.6,
                x2: 0.5,
                y2: 1.0,
            },
            // Even in, even out: the hand that turns a page.
            Style::Editorial => Curve::Bezier {
                x1: 0.45,
                y1: 0.0,
                x2: 0.55,
                y2: 1.0,
            },
        }
    }

    /// The weight the type carries.
    fn weight(&self) -> u32 {
        match self {
            Style::Calm => 500,
            Style::Punchy => 900,
            Style::Editorial => 600,
        }
    }

    /// The tracking, as CSS letter-spacing.
    fn tracking(&self) -> &'static str {
        match self {
            Style::Calm => "0.02em",
            Style::Punchy => "-0.04em",
            Style::Editorial => "0.08em",
        }
    }

    /// The leading, as CSS line-height.
    fn leading(&self) -> &'static str {
        match self {
            Style::Calm => "1.6",
            Style::Punchy => "1.08",
            Style::Editorial => "1.45",
        }
    }

    /// The tempo every animation runs at: `1.0` is as written.
    fn tempo(&self) -> f64 {
        match self {
            Style::Calm => 1.4,
            Style::Punchy => 0.72,
            Style::Editorial => 1.0,
        }
    }
}

/// Restyle a comp in place: the typography and the motion take the
/// personality, the layout keeps its own. The colour is not touched
/// here - it comes back from [`Style::grade`] and is baked as any other
/// grade, so a scene's own `grade` can still win.
pub fn restyle(comp: &mut Comp, style: Style) {
    let curve = style.curve();
    for node in &mut comp.nodes {
        // The motion: every eased segment takes the personality's hand.
        // Linear counts, typewriter steps, `linear()` ramps and springs
        // keep what they are - their meaning is mechanical, not mood.
        for track in node.tracks.values_mut() {
            for key in &mut track.keys {
                if matches!(key.curve, Some(Curve::Bezier { .. })) {
                    key.curve = Some(curve.clone());
                }
            }
        }
        // The stylesheet: weights, tracking and leading, timing tokens
        // and tempo, rewritten in place.
        if let NodeKind::Html { css, .. } = &mut node.kind {
            let mut sheet = std::mem::take(css);
            sheet = swap_weights(&sheet, style.weight());
            sheet = swap_tracking(&sheet, style.tracking(), style.leading());
            sheet = swap_timing(&sheet, &curve.css());
            if style.tempo() != 1.0 {
                sheet = retime(&sheet, style.tempo());
            }
            *css = sheet;
        }
    }
}

/// Every `font-weight: <number>` in the stylesheet becomes `weight`.
/// A weight written as a name is left alone: this is a restyle, not a
/// re-education.
fn swap_weights(css: &str, weight: u32) -> String {
    replace_property(css, "font-weight", &weight.to_string(), true)
}

/// Every `letter-spacing` becomes `tracking`, and when the stylesheet
/// never set one the tracking and leading arrive as a rule of their own
/// at the end - the cascade's last word on properties the template left
/// open.
fn swap_tracking(css: &str, tracking: &str, leading: &str) -> String {
    let mut sheet = replace_property(css, "letter-spacing", tracking, false);
    if !sheet.contains("letter-spacing") {
        sheet.push_str(&format!(
            "\n* {{ letter-spacing: {tracking}; line-height: {leading}; }}\n"
        ));
    } else if !sheet.contains("line-height") {
        sheet.push_str(&format!("\n* {{ line-height: {leading}; }}\n"));
    }
    sheet
}

/// Every `property: <value>` declaration takes `value`. With
/// `numbers_only`, a value that is not a plain number - a name, a
/// calculation - is left as the template wrote it.
fn replace_property(css: &str, property: &str, value: &str, numbers_only: bool) -> String {
    let needle = format!("{property}:");
    let boundary = |c: char| c.is_alphanumeric() || c == '-';
    let mut out = String::with_capacity(css.len());
    let mut at = 0;
    while at < css.len() {
        if css[at..].starts_with(&needle) {
            let before_ok = at == 0 || !boundary(css[..at].chars().next_back().unwrap_or(' '));
            if before_ok {
                let value_start = at + needle.len();
                let value_end = css[value_start..]
                    .find([';', '\n', '}'])
                    .map(|end| value_start + end)
                    .unwrap_or(css.len());
                let old = css[value_start..value_end].trim();
                if !numbers_only || old.chars().all(|c| c.is_ascii_digit()) {
                    out.push_str(&format!("{needle} {value}"));
                    at = value_end;
                    continue;
                }
                // A value the restyle does not reach: copied whole, and
                // the search moves past it rather than finding it again.
                out.push_str(&css[at..value_end]);
                at = value_end;
                continue;
            }
        }
        let end = css[at..].chars().next().map(char::len_utf8).unwrap_or(1);
        out.push_str(&css[at..at + end]);
        at += end;
    }
    out
}

/// Every timing function in the stylesheet becomes `timing` - except
/// the mechanical ones: `steps()` and `linear()` keep their meaning, and
/// a bare `linear` is not a mood.
fn swap_timing(css: &str, timing: &str) -> String {
    let mut sheet = css.to_owned();
    // The bezier family first: everything the presets write is either
    // cubic-bezier(...) or a named ease, and the names are prefixes of
    // one another, so the longest go first.
    for name in [
        "cubic-bezier(",
        "ease-in-out",
        "ease-out",
        "ease-in",
        "ease",
    ] {
        sheet = replace_token(&sheet, name, timing);
    }
    sheet
}

/// Replace whole occurrences of `token` in `text` with `to`, where a
/// whole occurrence is bounded by anything that is not a letter, a
/// digit, a dash or an open paren on either side - so `ease` never
/// matches the `ease` inside `ease-out` (already replaced) or inside
/// `release`, and `cubic-bezier(` is matched with its paren.
fn replace_token(text: &str, token: &str, to: &str) -> String {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '(';
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if text[at..].starts_with(token) {
            let before_ok = at == 0 || !is_word(bytes[at - 1] as char);
            // The paren of `cubic-bezier(` is part of the token, so the
            // boundary after it is the letter that must not follow -
            // which it cannot, because the token ends in the paren.
            let after = at + token.len();
            let after_ok = after >= bytes.len()
                || if token.ends_with('(') {
                    true
                } else {
                    !is_word(bytes[after] as char)
                };
            if before_ok && after_ok {
                out.push_str(to);
                at = after;
                // A cubic-bezier takes arguments to a closing paren:
                // they are replaced with the new timing whole.
                if token.ends_with('(') {
                    while at < bytes.len() && bytes[at] != b')' {
                        at += 1;
                    }
                    if at < bytes.len() {
                        at += 1;
                    }
                }
                continue;
            }
        }
        let end = text[at..].chars().next().map(char::len_utf8).unwrap_or(1);
        out.push_str(&text[at..at + end]);
        at += end;
    }
    out
}

/// Every `<number>s` on an `animation:` line is scaled by `tempo`: the
/// durations and the delays alike, because a rhythm that slows its
/// entrances but not its waits is two rhythms.
fn retime(css: &str, tempo: f64) -> String {
    let mut out = String::with_capacity(css.len());
    for line in css.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("animation:") || trimmed.contains(" animation:") {
            out.push_str(&scale_seconds(line, tempo));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Every `<number>s` in `text`, scaled by `tempo`. `ms` values are left
/// alone - a millisecond is a different unit with its own meaning - and
/// two decimals is finer than any rhythm.
fn scale_seconds(text: &str, tempo: f64) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_digit() {
            let start = at;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            if at < bytes.len()
                && bytes[at] == b'.'
                && at + 1 < bytes.len()
                && bytes[at + 1].is_ascii_digit()
            {
                at += 1;
                while at < bytes.len() && bytes[at].is_ascii_digit() {
                    at += 1;
                }
            }
            let is_seconds = at < bytes.len() && bytes[at] == b's' && {
                let after = at + 1;
                after >= bytes.len() || !(bytes[after] as char).is_ascii_alphanumeric()
            };
            let before_ok = start == 0 || !(bytes[start - 1] as char).is_ascii_alphanumeric();
            if is_seconds && before_ok {
                let value: f64 = text[start..at].parse().unwrap_or_default();
                let scaled = (value * tempo * 100.0).round() / 100.0;
                out.push_str(&format!("{scaled}s"));
                at += 1;
                continue;
            }
            out.push_str(&text[start..at]);
            continue;
        }
        let end = text[at..].chars().next().map(char::len_utf8).unwrap_or(1);
        out.push_str(&text[at..at + end]);
        at += end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, Node, Rgba};
    use crate::track::{Key, Track, Value};
    use std::collections::BTreeMap;

    fn html_comp(css: &str, tracks: BTreeMap<String, Track>) -> Comp {
        Comp {
            format: FORMAT,
            id: "test.style".into(),
            name: "Style".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 4.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: BTreeMap::new(),
            curves: BTreeMap::new(),
            nodes: vec![Node {
                id: "card".into(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Default::default(),
                opacity: 1.0,
                blend: crate::comp::Blend::Normal,
                mask: None,
                effects: Vec::new(),
                tracks,
                kind: NodeKind::Html {
                    markup: "<div class=\"card\">42</div>".into(),
                    css: css.into(),
                    width: crate::comp::Length::Percent(100.0),
                    height: crate::comp::Length::Percent(100.0),
                },
            }],
            slots: Vec::new(),
        }
    }

    #[test]
    fn the_personalities_read_and_write_as_their_own_names() {
        for name in ["calm", "punchy", "editorial"] {
            let style: Style = serde_json::from_str(&format!("\"{name}\"")).unwrap();
            assert_eq!(
                serde_json::to_string(&style).expect("writes"),
                format!("\"{name}\"")
            );
        }
        assert!(serde_json::from_str::<Style>("\"loud\"").is_err());
    }

    #[test]
    fn the_weights_and_tracking_and_grade_are_the_personalities_own() {
        assert_eq!(Style::Calm.weight(), 500);
        assert_eq!(Style::Punchy.weight(), 900);
        assert_eq!(Style::Editorial.weight(), 600);
        // The colour half: cool for calm, hot for punchy, and the
        // editorial's own crisp neutral - a balanced grade with a
        // contrast move and no cast at all.
        assert_eq!(Style::Calm.grade().temperature, Temperature::Cool);
        assert_eq!(Style::Calm.grade().amount, 0.6);
        assert_eq!(Style::Punchy.grade().temperature, Temperature::Hot);
        assert_eq!(Style::Punchy.grade().amount, 1.0);
        assert_eq!(Style::Editorial.grade().temperature, Temperature::Balanced);
        assert_eq!(Style::Editorial.grade().contrast, Some(1.06));
        assert_eq!(Style::Editorial.grade().tint, None);
    }

    #[test]
    fn the_weights_are_swapped_in_place() {
        let css = ".mark { font-weight: 900; letter-spacing: -0.045em }\n\
                   .sub { font-weight: 400; letter-spacing: 0.42em }";
        let swapped = swap_weights(css, 500);
        assert!(swapped.contains("font-weight: 500"), "{swapped}");
        assert_eq!(swapped.matches("font-weight: 500").count(), 2, "{swapped}");
        // A named weight is left alone.
        let named = swap_weights(".a { font-weight: bold }", 500);
        assert!(named.contains("bold"), "{named}");
    }

    #[test]
    fn the_tracking_is_swapped_and_arrives_when_absent() {
        let css = ".mark { font-weight: 900; letter-spacing: -0.045em }";
        let swapped = swap_tracking(css, "0.08em", "1.45");
        assert!(swapped.contains("letter-spacing: 0.08em"), "{swapped}");
        assert!(!swapped.contains("-0.045em"), "{swapped}");
        // A stylesheet with no tracking of its own gets the rule at the
        // end, where the cascade gives it the last word.
        let bare = swap_tracking(".mark { font-weight: 900 }", "0.02em", "1.6");
        assert!(
            bare.contains("* { letter-spacing: 0.02em; line-height: 1.6; }"),
            "{bare}"
        );
    }

    #[test]
    fn the_timing_tokens_take_the_personality_but_keep_their_meaning() {
        let css = ".a { animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.18s both }\n\
                   .b { animation: tick 0.3s steps(24) both }\n\
                   .c { animation: fade 0.4s ease-in-out both }\n\
                   .d { animation: hold 1s linear both }";
        let swapped = swap_timing(css, "cubic-bezier(0.3, 1.6, 0.5, 1)");
        assert!(
            swapped.contains("animation: rise 0.5s cubic-bezier(0.3, 1.6, 0.5, 1) 0.18s both"),
            "{swapped}"
        );
        assert!(
            swapped.contains("animation: fade 0.4s cubic-bezier(0.3, 1.6, 0.5, 1) both"),
            "{swapped}"
        );
        // The mechanical timings keep their meaning.
        assert!(swapped.contains("steps(24)"), "{swapped}");
        assert!(swapped.contains("1s linear"), "{swapped}");
        // And a word that merely contains `ease` is untouched.
        let release = swap_timing(".e { content: release }", "cubic-bezier(1, 1, 1, 1)");
        assert!(release.contains("release"), "{release}");
    }

    #[test]
    fn the_tempo_scales_durations_and_delays_alike() {
        let css = ".a { animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) 0.18s both }";
        let slowed = retime(css, 1.4);
        assert!(
            slowed.contains("0.7s cubic-bezier(0.16,1,0.3,1) 0.25s"),
            "{slowed}"
        );
        let quickened = retime(css, 0.72);
        assert!(
            quickened.contains("0.36s cubic-bezier(0.16,1,0.3,1) 0.13s"),
            "{quickened}"
        );
        // Milliseconds are a different unit with their own meaning.
        let ms = retime(".a { transition: width 250ms }", 1.4);
        assert!(ms.contains("250ms"), "{ms}");
        // Lines that are not animations are untouched.
        let other = retime(".a { width: 12s }", 1.4);
        assert!(other.contains("12s"), "{other}");
    }

    #[test]
    fn the_eased_keys_take_the_hand_and_the_mechanical_ones_keep_theirs() {
        let mut tracks = BTreeMap::new();
        tracks.insert(
            "scale".into(),
            Track::new(vec![
                Key::eased(0.0, Value::number(0.8), Curve::EASE_OUT),
                Key::new(1.0, Value::number(1.0)),
            ]),
        );
        tracks.insert(
            "opacity".into(),
            Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::TYPEWRITER),
                Key::new(1.0, Value::number(1.0)),
            ]),
        );
        tracks.insert(
            "position".into(),
            Track::new(vec![Key::eased(0.0, Value::number(0.0), Curve::SPRING_POP)]),
        );
        tracks.insert(
            "blur".into(),
            Track::new(vec![Key::new(0.0, Value::number(4.0))]),
        );
        let mut comp = html_comp(".a { font-weight: 700 }", tracks);
        restyle(&mut comp, Style::Punchy);
        let node = &comp.nodes[0];
        let scale = &node.tracks["scale"].keys[0].curve;
        assert!(
            matches!(scale, Some(Curve::Bezier { y1, .. }) if (*y1 - 1.6).abs() < 1e-9),
            "{scale:?}"
        );
        // The typewriter, the spring and the curve-less key keep theirs.
        assert!(
            matches!(
                &node.tracks["opacity"].keys[0].curve,
                Some(Curve::Steps { .. })
            ),
            "the typewriter keeps its steps"
        );
        assert!(
            matches!(
                &node.tracks["position"].keys[0].curve,
                Some(Curve::Spring(_))
            ),
            "the spring keeps its spring"
        );
        assert!(node.tracks["blur"].keys[0].curve.is_none());
    }

    #[test]
    fn the_stylesheet_comes_out_wearing_the_personality() {
        let css = ".mark { font-family: Archivo; font-size: 190px; font-weight: 900;\n\
                   letter-spacing: -0.045em; color: #f4f1ea }\n\
                   .rule { width: 0px; animation: grow 0.9s cubic-bezier(0.16,1,0.3,1) 0.45s both }\n";
        let mut comp = html_comp(css, BTreeMap::new());
        restyle(&mut comp, Style::Editorial);
        let NodeKind::Html { css, .. } = &comp.nodes[0].kind else {
            panic!("the node is html");
        };
        // The layout keeps its geometry and its words; the type and the
        // motion take the personality.
        assert!(css.contains("font-size: 190px"), "{css}");
        assert!(css.contains("font-weight: 600"), "{css}");
        assert!(css.contains("letter-spacing: 0.08em"), "{css}");
        assert!(css.contains("* { line-height: 1.45; }"), "{css}");
        // Editorial's tempo is as written: no retiming happened.
        assert!(css.contains("0.9s"), "{css}");
        // And the even hand replaced the settle.
        assert!(css.contains("cubic-bezier(0.45, 0, 0.55, 1)"), "{css}");
    }

    #[test]
    fn calm_slows_the_rhythm_and_punchy_hurries_it() {
        let sheet = ".rule { animation: grow 0.9s cubic-bezier(0.16,1,0.3,1) 0.45s both }";
        // Two comps from the same sheet: a restyle is a copy's to keep,
        // and the second one starts from the template as written.
        let mut calm = html_comp(sheet, BTreeMap::new());
        restyle(&mut calm, Style::Calm);
        let NodeKind::Html { css, .. } = &calm.nodes[0].kind else {
            panic!("html")
        };
        assert!(css.contains("1.26s"), "{css}");
        assert!(css.contains("0.63s"), "{css}");
        let mut punchy = html_comp(sheet, BTreeMap::new());
        restyle(&mut punchy, Style::Punchy);
        let NodeKind::Html { css, .. } = &punchy.nodes[0].kind else {
            panic!("html")
        };
        assert!(css.contains("0.65s"), "{css}");
        assert!(css.contains("0.32s"), "{css}");
        // And punchy's arrival overshoots.
        assert!(css.contains("cubic-bezier(0.3, 1.6, 0.5, 1)"), "{css}");
    }

    #[test]
    fn the_three_personalities_make_three_different_sheets() {
        let css = ".mark { font-weight: 700; letter-spacing: -0.02em;\n\
                   animation: rise 0.5s cubic-bezier(0.16,1,0.3,1) both }";
        let mut sheets = Vec::new();
        for style in [Style::Calm, Style::Punchy, Style::Editorial] {
            let mut comp = html_comp(css, BTreeMap::new());
            restyle(&mut comp, style);
            let NodeKind::Html { css, .. } = &comp.nodes[0].kind else {
                panic!("html")
            };
            sheets.push(css.clone());
        }
        assert_ne!(sheets[0], sheets[1]);
        assert_ne!(sheets[1], sheets[2]);
        assert_ne!(sheets[0], sheets[2]);
    }
}
