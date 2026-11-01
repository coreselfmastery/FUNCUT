// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! A stylesheet read for the one thing the markup engine stays quiet about.
//!
//! Geneva animates `transform` - and the individual `translate`, `rotate`
//! and `scale` properties - only inside `@keyframes`. A transform set as a
//! base style is silently skipped: the layer is reported by name and drawn
//! at its default position, and every motion feature built on top of it is
//! then wrong while looking perfectly valid. The engine does not say so;
//! [`crate::comp::Comp::validate`] says so instead, using
//! [`static_transforms`] here, so the comp is refused with the rule and the
//! property in the message rather than rendered wrong.
//!
//! The scan is deliberately small: comments out, strings kept whole so a
//! `content: "transform: x"` cannot false-positive, blocks tracked with a
//! stack so `@keyframes` marks everything nested inside it, and every
//! declaration checked for exactly the four property names. It is a lint
//! over authored CSS, not a parser: a stylesheet this crate cannot read
//! is left to the engine's own `E451` to refuse.

/// One static transform declaration found where the engine will skip it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StaticTransform {
    /// The selector whose rule carries the declaration, as written.
    pub selector: String,
    /// The property: `transform`, `translate`, `rotate` or `scale`.
    pub property: String,
}

/// The properties that place a layer when written outside `@keyframes`,
/// and are skipped there: the engine reads them only as animation values.
const PLACING_PROPERTIES: [&str; 4] = ["transform", "translate", "rotate", "scale"];

/// Every transform-style declaration in `css` that sits outside a
/// `@keyframes` block, in document order.
///
/// ```text
/// .layer { transform: translateX(120px) }          -> named
/// .bar  { transform-origin: top center }           -> clean (origin, not transform)
/// @keyframes rise { from { translate: 0 -14px } }  -> clean (inside keyframes)
/// ```
pub fn static_transforms(css: &str) -> Vec<StaticTransform> {
    let text = without_comments(css);
    let mut found = Vec::new();
    // One entry per open block: the selector it opened with, and whether
    // it is a @keyframes context - which everything nested inside it is.
    let mut stack: Vec<(String, bool)> = Vec::new();
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
                check_declaration(&mut token, &stack, &mut found);
                stack.pop();
                token.clear();
            }
            ';' => {
                check_declaration(&mut token, &stack, &mut found);
                token.clear();
            }
            _ => token.push(character),
        }
    }
    found
}

/// Whether a declaration token sets a placing property, and the block it
/// sits in is not a `@keyframes` one. `token` is drained either way: a
/// declaration is checked once.
fn check_declaration(
    token: &mut String,
    stack: &[(String, bool)],
    found: &mut Vec<StaticTransform>,
) {
    let declaration = std::mem::take(token);
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

    #[test]
    fn a_transform_inside_keyframes_is_clean() {
        let css = "@keyframes rise { from { transform: translateY(14px) } \
                   to { transform: translateY(0) } }";
        assert!(static_transforms(css).is_empty());
    }

    #[test]
    fn the_individual_placing_properties_are_named_too() {
        let css = ".a { translate: -100% 0; } .b { rotate: 90deg; } .c { scale: 1.2; }";
        let hits = static_transforms(css);
        assert_eq!(hits.len(), 3);
        let properties: Vec<&str> = hits.iter().map(|hit| hit.property.as_str()).collect();
        assert_eq!(properties, ["translate", "rotate", "scale"]);
    }

    #[test]
    fn transform_origin_is_not_a_transform() {
        assert!(static_transforms(".bar { transform-origin: top center; }").is_empty());
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
}
