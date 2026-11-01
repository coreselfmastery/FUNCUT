// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! A stylesheet read for the two things the markup engine stays quiet
//! about - or says too late.
//!
//! Geneva animates `transform` - and the individual `translate`,
//! `rotate`, `scale` and `perspective` properties - only inside
//! `@keyframes`. A transform set as a
//! base style is silently skipped: the layer is reported by name and drawn
//! at its default position, and every motion feature built on top of it is
//! then wrong while looking perfectly valid. The engine does not say so;
//! [`crate::comp::Comp::validate`] says so instead, using
//! [`static_transforms`] here, so the comp is refused with the rule and the
//! property in the message rather than rendered wrong.
//!
//! The mirror trap is **weight**: `font-variation-settings` is not drawn
//! at all (the engine's W450, so a variable font's axis is never read)
//! and `font-weight` cannot be animated (E442) - a comp that puts either
//! inside a `@keyframes` rule is refused by the engine only at the bake,
//! after it validated everywhere else, and one that sets
//! `font-variation-settings` as a base style renders at the family's
//! default instance while looking exactly as authored. [`refused_weights`]
//! names both, so the comp is refused here with the fix - a static
//! `font-weight` per element, which the engine snaps to the family's own
//! instances and draws.
//!
//! The same door also carries the **determinism contract**'s half of the
//! markup ([`network_refs`]): a comp that fetches a URL, imports a
//! stylesheet or asks the clock for a value is a comp that cannot promise
//! the same bytes twice, and the contract refuses it here - before the
//! bake, with the token that broke it - instead of rendering a timeline
//! that happens to be different on a machine without a network.
//!
//! The scan is deliberately small: comments out, strings kept whole so a
//! `content: "transform: x"` cannot false-positive, blocks tracked with a
//! stack so `@keyframes` marks everything nested inside it, and every
//! declaration checked for exactly the property names each guard reads. It
//! is a lint over authored CSS, not a parser: a stylesheet this crate
//! cannot read is left to the engine's own `E451` to refuse.

/// One static transform declaration found where the engine will skip it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StaticTransform {
    /// The selector whose rule carries the declaration, as written.
    pub selector: String,
    /// The property: `transform`, `translate`, `rotate`, `scale` or
    /// `perspective`.
    pub property: String,
}

/// The properties that place a layer when written outside `@keyframes`,
/// and are skipped there: the engine reads them only as animation values.
const PLACING_PROPERTIES: [&str; 5] = ["transform", "translate", "rotate", "scale", "perspective"];

/// Every transform-style declaration in `css` that sits outside a
/// `@keyframes` block, in document order.
///
/// ```text
/// .layer { transform: translateX(120px) }          -> named
/// .bar  { transform-origin: top center }           -> clean (origin, not transform)
/// @keyframes rise { from { translate: 0 -14px } }  -> clean (inside keyframes)
/// ```
pub fn static_transforms(css: &str) -> Vec<StaticTransform> {
    let mut found = Vec::new();
    declarations(css, &mut |declaration, stack| {
        let inside_keyframes = stack.last().is_some_and(|(_, keyframes)| *keyframes);
        if inside_keyframes {
            return;
        }
        // The property is what stands before the first colon; a value may
        // carry colons of its own (`url(data:image/png,...)`) and a string in
        // a value may carry anything, and neither is the property.
        let Some((property, _)) = declaration.split_once(':') else {
            return;
        };
        let property = property.trim();
        if PLACING_PROPERTIES.contains(&property) {
            found.push(StaticTransform {
                selector: stack
                    .last()
                    .map(|(selector, _)| selector.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                property: property.to_owned(),
            });
        }
    });
    found
}

/// One weight declaration the engine refuses or does not draw, found
/// where an author would put one on purpose.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RefusedWeight {
    /// The `@keyframes` rule (named) or the base-style rule (its
    /// selector) the declaration sits in, as written.
    pub selector: String,
    /// The property: `font-variation-settings` or `font-weight`.
    pub property: String,
    /// Whether the declaration sits inside a `@keyframes` rule - where
    /// the engine refuses to animate either property (E442) - or is a
    /// base style, where `font-variation-settings` is not drawn at all
    /// (W450) and a static `font-weight` is the one weight that works.
    pub animated: bool,
}

/// The properties that carry a weight. `font-weight` is drawn as a base
/// style (snapped to the family's own instances) but refused inside
/// `@keyframes`; `font-variation-settings` is not drawn anywhere (W450)
/// and refused inside `@keyframes` too (E442).
const WEIGHT_PROPERTIES: [&str; 2] = ["font-variation-settings", "font-weight"];

/// Every weight declaration the engine cannot honour, in document order:
///
/// ```text
/// @keyframes flex { from { font-variation-settings: 'wght' 100 } }  -> named (E442)
/// @keyframes pop  { to { font-weight: 900 } }                      -> named (E442)
/// .layer { font-variation-settings: 'wght' 650 }                    -> named (W450)
/// .layer { font-weight: 700 }                                       -> clean (drawn, snapped)
/// ```
pub fn refused_weights(css: &str) -> Vec<RefusedWeight> {
    let mut found = Vec::new();
    declarations(css, &mut |declaration, stack| {
        let inside_keyframes = stack.last().is_some_and(|(_, keyframes)| *keyframes);
        let Some((property, _)) = declaration.split_once(':') else {
            return;
        };
        let property = property.trim();
        if !WEIGHT_PROPERTIES.contains(&property) {
            return;
        }
        // `font-weight` as a base style is the one weight the engine
        // draws; everything else here is refused or not drawn.
        if !inside_keyframes && property == "font-weight" {
            return;
        }
        // Inside `@keyframes` the report names the rule, not the offset:
        // the fix is about the property, and `from`/`to` is where it
        // already is.
        let selector = if inside_keyframes {
            stack
                .iter()
                .rev()
                .find(|(selector, keyframes)| {
                    *keyframes
                        && selector
                            .trim_start()
                            .to_ascii_lowercase()
                            .starts_with("@keyframes")
                })
                .map(|(selector, _)| selector.trim().to_owned())
                .unwrap_or_default()
        } else {
            stack
                .last()
                .map(|(selector, _)| selector.trim().to_owned())
                .unwrap_or_default()
        };
        found.push(RefusedWeight {
            selector,
            property: property.to_owned(),
            animated: inside_keyframes,
        });
    });
    // One report per rule and property: a `@keyframes` rule that sets the
    // same property at `from` and at `to` is one problem with one fix,
    // not two.
    found.dedup_by(|a, b| a.selector == b.selector && a.property == b.property);
    found
}

/// One open block of a stylesheet, as the declaration walker holds it:
/// the selector it opened with, and whether it is a `@keyframes`
/// context - which everything nested inside one is.
type Block = (String, bool);

/// Walk every declaration in the stylesheet - comments out, strings kept
/// whole - handing each one to `visit` with the block stack it sits in:
/// one entry per open block, the selector it opened with and whether it
/// is a `@keyframes` context, which everything nested inside one is. A
/// declaration is `property: value`, up to a `;` or the block's closing
/// `}`; the token is drained before `visit` runs, so a visitor cannot
/// read the next one by accident.
fn declarations(css: &str, visit: &mut dyn FnMut(&str, &[Block])) {
    let text = without_comments(css);
    // One entry per open block: the selector it opened with, and whether
    // it is a @keyframes context - which everything nested inside it is.
    let mut stack: Vec<Block> = Vec::new();
    let mut token = String::new();
    let mut in_string: Option<char> = None;
    for character in text.chars() {
        if let Some(quote) = in_string {
            token.push(character);
            if character == quote {
                in_string = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => {
                in_string = Some(character);
                token.push(character);
            }
            '{' => {
                let selector = std::mem::take(&mut token);
                let opens_keyframes = selector
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("@keyframes");
                let keyframes =
                    opens_keyframes || stack.last().is_some_and(|(_, keyframes)| *keyframes);
                stack.push((selector.trim().to_owned(), keyframes));
            }
            '}' => {
                let declaration = std::mem::take(&mut token);
                if !declaration.trim().is_empty() {
                    visit(&declaration, &stack);
                }
                stack.pop();
            }
            ';' => {
                let declaration = std::mem::take(&mut token);
                if !declaration.trim().is_empty() {
                    visit(&declaration, &stack);
                }
            }
            _ => token.push(character),
        }
    }
}

/// The stylesheet with `/* ... */` comments removed, so a commented-out
/// transform is not named: what is not drawn cannot be skipped.
fn without_comments(css: &str) -> String {
    let characters: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(css.len());
    let mut at = 0;
    while at < characters.len() {
        let character = characters[at];
        if character == '/' && at + 1 < characters.len() && characters[at + 1] == '*' {
            at += 2;
            while at + 1 < characters.len() && !(characters[at] == '*' && characters[at + 1] == '/')
            {
                at += 1;
            }
            at = (at + 2).min(characters.len());
            out.push(' ');
        } else {
            out.push(character);
            at += 1;
        }
    }
    out
}

/// One reference the determinism contract refuses, and the token that
/// broke it: a fetch the render would make, or a value the clock or an
/// unseeded random would be asked for - either way, a render that is not
/// the same bytes twice.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NetworkRef {
    /// The kind of refusal, in the words the message carries:
    /// `a mid-render fetch`, `a stylesheet import`, `an unseeded random`
    /// or `the wall clock`.
    pub kind: &'static str,
    /// The token as it was found, trimmed: `url(https://…/logo.png)`,
    /// `@import "…"`, `Math.random(`, `Date.now(`, `new Date(`.
    pub text: String,
}

/// Every reference in `text` - a stylesheet or a markup document, either
/// scans the same - that the determinism contract refuses:
///
/// - `url(http://…)`, `url(https://…)` and `url(//…)`: a fetch the render
///   would make, whose answer is the network's to give. `data:` URLs are
///   inline bytes and stage-free: welcome. Relative paths are the staged
///   assets' own names: welcome too.
/// - `@import`, of anything: a whole stylesheet read at render time that
///   the document did not stage, local or remote.
/// - `src=`/`href=` attributes pointing off-machine (`<img src="https://…">`).
/// - `Math.random(`, `Date.now(`, `new Date(`: a value the render would
///   ask for that no seed and no clock can answer the same way twice.
///
/// The scan is case-insensitive and comment-blind, because a fetch that
/// is commented out is not a fetch and a capital `URL(` still is one.
pub fn network_refs(text: &str) -> Vec<NetworkRef> {
    let text = without_comments(text);
    let lowercase = text.to_ascii_lowercase();
    let mut found = Vec::new();
    // url(...) whose target is off-machine. Byte offsets in the lowercase
    // copy line up with the original, so a find in one finds in both.
    let mut at = 0;
    while let Some(open) = lowercase[at..].find("url(") {
        let open = at + open;
        let after = open + 4;
        let close = lowercase[after..].find(')').map(|offset| after + offset);
        let Some(close) = close else { break };
        let inner = lowercase[after..close].trim();
        let target = inner.trim_start_matches(['"', '\'']);
        let off_machine = target.starts_with("http:")
            || target.starts_with("https://")
            || target.starts_with("//");
        if off_machine {
            found.push(NetworkRef {
                kind: "a mid-render fetch",
                text: text[open..close + 1].trim().to_owned(),
            });
        }
        at = close + 1;
    }
    for (needle, kind) in [
        ("@import", "a stylesheet import"),
        ("math.random(", "an unseeded random"),
        ("date.now(", "the wall clock"),
        ("new date(", "the wall clock"),
    ] {
        let mut at = 0;
        while let Some(hit) = lowercase[at..].find(needle) {
            let hit = at + hit;
            let end = (hit + needle.len()).min(text.len());
            found.push(NetworkRef {
                kind,
                text: text[hit..end].trim().to_owned(),
            });
            at = end;
        }
    }
    // src= and href= attributes pointing off-machine: an <img>, <source>,
    // <script>, <link> that fetches what the document did not stage.
    for attribute in ["src=", "href="] {
        let mut at = 0;
        while let Some(hit) = lowercase[at..].find(attribute) {
            let value_at = at + hit + attribute.len();
            let rest = &text[value_at..];
            let quoted = rest.trim_start().starts_with('"') || rest.trim_start().starts_with('\'');
            let value = if quoted {
                let quote = rest.trim_start().chars().next().unwrap_or('"');
                rest.trim_start()[1..]
                    .split(quote)
                    .next()
                    .unwrap_or_default()
            } else {
                rest.split([' ', '>', '\n', '\t'])
                    .next()
                    .unwrap_or_default()
            };
            let target = value.trim();
            if target.starts_with("http:")
                || target.starts_with("https://")
                || target.starts_with("//")
            {
                found.push(NetworkRef {
                    kind: "a mid-render fetch",
                    text: format!("{attribute}\"{target}\""),
                });
            }
            at = value_at + value.len().max(1);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transform_as_a_base_style_is_named_with_its_rule() {
        let hits = static_transforms(".layer { transform: translateX(120px); }");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].selector, ".layer");
        assert_eq!(hits[0].property, "transform");
    }

    /// The weight traps, both directions: a weight inside `@keyframes`
    /// (the engine's E442) and a `font-variation-settings` as a base
    /// style (W450 - not drawn, so the variable axis is never read).
    /// A rule that sets the same property at `from` and at `to` is one
    /// report, not two: one problem, one fix.
    #[test]
    fn a_weight_inside_keyframes_is_named_with_its_rule() {
        let css = "@keyframes flex { from { font-variation-settings: 'wght' 100 } \
                   to { font-variation-settings: 'wght' 900 } }";
        let hits = refused_weights(css);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].selector, "@keyframes flex", "{hits:?}");
        assert_eq!(hits[0].property, "font-variation-settings");
        assert!(hits[0].animated);
    }

    #[test]
    fn two_rules_are_two_reports() {
        let css =
            "@keyframes a { to { font-weight: 900 } } @keyframes b { to { font-weight: 100 } }";
        let hits = refused_weights(css);
        assert_eq!(hits.len(), 2, "{hits:?}");
        let selectors: Vec<&str> = hits.iter().map(|hit| hit.selector.as_str()).collect();
        assert_eq!(selectors, ["@keyframes a", "@keyframes b"]);
    }

    #[test]
    fn an_animated_font_weight_is_named_too() {
        let css = "@keyframes pop { from { opacity: 0 } to { opacity: 1; font-weight: 900 } }";
        let hits = refused_weights(css);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].property, "font-weight");
        assert!(hits[0].animated);
        assert_eq!(hits[0].selector, "@keyframes pop", "{hits:?}");
    }

    #[test]
    fn a_base_style_variation_setting_is_named_but_a_font_weight_is_not() {
        let css = ".a { font-variation-settings: 'wght' 650; } .b { font-weight: 700; }";
        let hits = refused_weights(css);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].property, "font-variation-settings");
        assert!(!hits[0].animated);
        assert_eq!(hits[0].selector, ".a", "{hits:?}");
    }

    #[test]
    fn a_weight_inside_a_string_is_not_named() {
        assert!(refused_weights(".a::after { content: \"font-weight: x\"; }").is_empty());
    }

    #[test]
    fn a_commented_out_weight_is_not_named() {
        assert!(
            refused_weights(".a { /* font-variation-settings: 'wght' 700 */ opacity: 1; }")
                .is_empty()
        );
    }

    #[test]
    fn a_transform_inside_keyframes_is_clean() {
        let css = "@keyframes rise { from { transform: translateY(14px) } \
                   to { transform: translateY(0) } }";
        assert!(static_transforms(css).is_empty());
    }

    #[test]
    fn the_individual_placing_properties_are_named_too() {
        let css = ".a { translate: -100% 0; } .b { rotate: 90deg; } .c { scale: 1.2; } \
                  .d { perspective: 900px; }";
        let hits = static_transforms(css);
        assert_eq!(hits.len(), 4);
        let properties: Vec<&str> = hits.iter().map(|hit| hit.property.as_str()).collect();
        assert_eq!(properties, ["translate", "rotate", "scale", "perspective"]);
    }

    #[test]
    fn transform_origin_is_not_a_transform() {
        assert!(static_transforms(".bar { transform-origin: top center; }").is_empty());
    }

    #[test]
    fn perspective_origin_is_not_a_perspective() {
        assert!(static_transforms(".stage { perspective-origin: top left; }").is_empty());
    }

    #[test]
    fn a_commented_out_transform_is_not_named() {
        assert!(
            static_transforms(".a { /* transform: translateX(10px) */ opacity: 1; }").is_empty()
        );
    }

    #[test]
    fn a_transform_inside_a_string_is_not_named() {
        assert!(static_transforms(".a::after { content: \"transform: x\"; }").is_empty());
    }

    #[test]
    fn nested_blocks_inside_keyframes_stay_clean_at_any_depth() {
        let css = "@keyframes slide {\n  from { translate: -100% 0 }\n  50% { scale: 1.1 }\n\
                   to { translate: 0 0; scale: 1 }\n}";
        assert!(static_transforms(css).is_empty());
    }

    #[test]
    fn a_transform_inside_a_media_query_is_still_a_base_style() {
        let css = "@media (max-width: 600px) { .layer { transform: scale(0.8) } }";
        let hits = static_transforms(css);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].property, "transform");
    }

    #[test]
    fn the_last_declaration_without_a_semicolon_is_still_checked() {
        assert_eq!(
            static_transforms(".layer { transform: translateX(120px) }").len(),
            1
        );
    }

    #[test]
    fn the_librarys_own_stylesheets_are_clean() {
        for id in crate::presets::ALL.iter().map(|preset| preset.id) {
            let comp = crate::presets::build(id).expect("the preset builds");
            for node in &comp.nodes {
                if let crate::comp::NodeKind::Html { css, .. } = &node.kind {
                    assert!(
                        static_transforms(css).is_empty(),
                        "{id}: the node {:?} carries a static transform",
                        node.id
                    );
                }
            }
        }
    }

    /// The determinism contract's half: every kind of refusuable reference
    /// is found, named and trimmed, and the references the contract
    /// welcomes (a staged file, a `data:` URL) are left alone.
    #[test]
    fn a_fetching_stylesheet_is_named_token_by_token() {
        let css = ".logo { background: url(https://cdn.example.com/logo.png); } \
                   .local { background: url(assets/logo.png); } \
                   .inline { background: url(data:image/png;base64,AAAA); }";
        let hits = network_refs(css);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].kind, "a mid-render fetch");
        assert_eq!(hits[0].text, "url(https://cdn.example.com/logo.png)");
    }

    /// `@import` is refused whatever it imports - local or remote, the
    /// stylesheet was never staged, so the render reads a file the
    /// document does not own. An import *by URL* is named twice over - it
    /// is both an import and a fetch - and twice is honest.
    #[test]
    fn an_import_is_refused_whatever_it_imports() {
        let hits =
            network_refs("@import \"theme.css\"; @import url('https://cdn.example.com/x.css');");
        assert_eq!(hits.len(), 3, "{hits:?}");
        let kinds: Vec<&str> = hits.iter().map(|hit| hit.kind).collect();
        assert!(kinds.contains(&"a stylesheet import"));
        assert!(kinds.contains(&"a mid-render fetch"));
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| **kind == "a stylesheet import")
                .count(),
            2,
            "both imports are named: {kinds:?}"
        );
    }

    /// The clock and the unseeded random: values a document cannot promise
    /// to reproduce, in markup or in a stylesheet, found in either case.
    #[test]
    fn the_clock_and_the_unseeded_random_are_refused() {
        let markup = "<script>const jitter = Math.random(); const now = Date.now();</script>";
        let hits = network_refs(markup);
        assert_eq!(hits.len(), 2, "{hits:?}");
        let kinds: Vec<&str> = hits.iter().map(|hit| hit.kind).collect();
        assert_eq!(kinds, ["an unseeded random", "the wall clock"]);
        // And the constructor form of the clock reads the same.
        assert_eq!(network_refs("const t = new Date();").len(), 1);
    }

    /// An `<img>` (or a `<script>`, or a `<link>`) pointing off-machine is
    /// a fetch the render would make; one pointing at a staged name is
    /// the asset the comp meant to name.
    #[test]
    fn a_remote_src_attribute_is_a_fetch_a_staged_one_is_not() {
        let markup = "<div class=\"row\"><img src=\"https://cdn.example.com/face.png\">\
                      <img src=\"assets/face.png\"></div>";
        let hits = network_refs(markup);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].text, "src=\"https://cdn.example.com/face.png\"");
    }

    /// A commented-out fetch is not a fetch, and a capitalised one still
    /// is: the scan reads the sheet the way the engine would.
    #[test]
    fn a_commented_fetch_is_not_a_fetch_and_case_does_not_hide_one() {
        assert!(
            network_refs(".a { background: /* url(https://x.example.com/p.png) */ #fff; }")
                .is_empty()
        );
        assert_eq!(
            network_refs(".a { background: URL(https://x.example.com/p.png); }").len(),
            1
        );
    }

    /// The library's own comps hold the contract: a preset that fetched or
    /// asked the clock would render differently per machine, and the whole
    /// cache - bake, reel, farm, golden frames - is built on the promise
    /// that none of them does.
    #[test]
    fn the_librarys_own_comps_carry_no_network_or_clock_references() {
        for preset in crate::presets::ALL {
            let comp = crate::presets::build(preset.id).expect("the preset builds");
            for node in &comp.nodes {
                if let crate::comp::NodeKind::Html { markup, css, .. } = &node.kind {
                    assert!(
                        crate::css::network_refs(css).is_empty(),
                        "{}: the node {:?} stylesheet carries a refused reference",
                        preset.id,
                        node.id
                    );
                    assert!(
                        crate::css::network_refs(markup).is_empty(),
                        "{}: the node {:?} markup carries a refused reference",
                        preset.id,
                        node.id
                    );
                }
            }
        }
    }

    /// The comp is refused with a pointer into the document - the agent's
    /// edit is one hop away, not a render failure to decode.
    #[test]
    fn a_comp_that_fetches_is_refused_with_its_pointer() {
        let mut comp = crate::presets::build("funcut.chapter.chip").expect("the preset");
        let node = comp.nodes.first_mut().expect("a node");
        if let crate::comp::NodeKind::Html { markup, .. } = &mut node.kind {
            *markup =
                "<div class=\"chip\"><img src=\"https://cdn.example.com/logo.png\"></div>".into();
        }
        let problems = comp.validate();
        assert!(
            problems
                .iter()
                .any(|problem| problem.path == "/nodes/0/markup"
                    && problem.message.contains("determinism")
                    && problem.message.contains("https://cdn.example.com")),
            "the refusal names the promise and the token: {problems:?}"
        );
    }
}
