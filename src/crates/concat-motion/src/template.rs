// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The comp template preprocessor: `{% for %}`, `{% if %}` and `{{ value }}`
//! over a comp document *before* it is parsed, so a shelf of comps can share
//! one parameterised source.
//!
//! A comp file is JSON; a template is the same JSON with Jinja placeholders
//! where a project's own words belong. One rule the whole crate keeps:
//! preprocessing happens *before* the parse, never after - whatever
//! [`render`] returns is an ordinary comp document, byte for byte the one an
//! author could have written by hand, so every engine downstream of the
//! parse stays innocent of templating.
//!
//! Variables are handed in by the caller - an MCP tool's `vars` field, the
//! agent's `--vars` flag, a storyboard's own `vars` map - as a JSON object.
//! The contract is deliberate on both sides:
//!
//! * **no variables, no placeholders**: the text passes through unchanged,
//!   so a plain comp pays nothing and nothing about it can surprise anyone.
//! * **no variables, but placeholders**: refused. A template without its
//!   variables would render its own placeholders as literal text, which is
//!   a comp that looks fine and says the wrong thing - the loud refusal is
//!   the cheap end of that trade.
//! * **variables, no placeholders**: unchanged. Handing variables to a plain
//!   comp is a no-op, the same way an untaken slot is.
//! * **variables and placeholders**: rendered, with minijinja's own errors
//!   carried through in full.

use crate::comp::Comp;

/// Whether a document carries Jinja placeholders at all.
fn templated(text: &str) -> bool {
    text.contains("{{") || text.contains("{%") || text.contains("{#")
}

/// Render a comp document's text through the template preprocessor.
///
/// `vars` is the JSON object the placeholders read; `None` means the caller
/// has no variables, which is the plain-comp case. See the [module
/// docs](self) for the full contract.
pub fn render(text: &str, vars: Option<&serde_json::Value>) -> Result<String, String> {
    let Some(vars) = vars.filter(|vars| !vars.is_null()) else {
        if templated(text) {
            return Err(
                "the document is a template and no variables were given: pass `vars`, or \
                 strip the placeholders"
                    .into(),
            );
        }
        return Ok(text.to_string());
    };
    if !templated(text) {
        return Ok(text.to_string());
    }
    let environment = minijinja::Environment::new();
    let context = minijinja::Value::from_serialize(vars);
    environment
        .render_str(text, context)
        .map_err(|error| format!("the template does not render: {error}"))
}

/// Read a comp document from disk and render it through the preprocessor in
/// one step: the seam every file-reading caller shares, so a comp, an agent
/// and a storyboard all preprocess the same way.
pub fn load(path: &std::path::Path, vars: Option<&serde_json::Value>) -> Result<Comp, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let rendered = render(&text, vars).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&rendered).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain comp with no variables passes through byte for byte.
    #[test]
    fn a_plain_comp_passes_through() {
        let text = r#"{"id": "funcut.plain", "width": 1920, "height": 1080, "duration": 4}"#;
        assert_eq!(render(text, None).unwrap(), text);
        assert_eq!(render(text, Some(&serde_json::json!({}))).unwrap(), text);
    }

    /// Variables that nobody reads are a no-op, the same way an untaken
    /// slot is.
    #[test]
    fn unread_variables_change_nothing() {
        let plain = r#"{"id": "funcut.plain", "words": "plain"}"#;
        assert_eq!(
            render(plain, Some(&serde_json::json!({"title": "Demo"}))).unwrap(),
            plain
        );
    }

    /// A template with its variables renders, and the result is an ordinary
    /// comp a caller can parse.
    #[test]
    fn a_template_renders_into_a_comp() {
        let text = r#"{"id": "funcut.quote", "width": 1920, "height": 1080,
  "duration": {{ seconds }},
  "nodes": [{ "id": "words", "kind": "text", "text": "{{ title }}" }]}"#;
        let rendered = render(
            text,
            Some(&serde_json::json!({"seconds": 4.5, "title": "Ship it"})),
        )
        .unwrap();
        let comp: Comp = serde_json::from_str(&rendered).unwrap();
        assert_eq!(comp.duration, 4.5);
        assert_eq!((comp.width, comp.height), (1920, 1080));
        let crate::comp::NodeKind::Text(text) = &comp.nodes[0].kind else {
            panic!("a text node, found {:?}", comp.nodes[0].kind);
        };
        assert_eq!(text.text, "Ship it");
    }

    /// `{% for %}` generates a node per item, the way a shelf of comps
    /// shares one source.
    #[test]
    fn a_for_loop_generates_nodes() {
        let text = r#"{"id": "funcut.rows", "width": 1920, "height": 1080, "duration": 3,
  "nodes": [
    {% for row in rows -%}
    { "id": "row{{ loop.index }}", "kind": "rect", "width": {"px": 100}, "height": {"px": {{ row }}} }{{ "," if not loop.last }}
    {%- endfor %}
  ]}"#;
        let rendered = render(text, Some(&serde_json::json!({"rows": [40, 80, 120]}))).unwrap();
        let comp: Comp = serde_json::from_str(&rendered).unwrap();
        assert_eq!(comp.nodes.len(), 3);
        assert_eq!(comp.nodes[2].id, "row3");
    }

    /// `{% if %}` picks a variant, the way one source serves two aspect
    /// ratios.
    #[test]
    fn an_if_picks_a_variant() {
        let text = r#"{% if wide %}"width": 1920, "height": 1080{% else %}"width": 1080, "height": 1920{% endif %}"#;
        let rendered = render(text, Some(&serde_json::json!({"wide": true}))).unwrap();
        assert!(rendered.contains("1920"));
        let rendered = render(text, Some(&serde_json::json!({"wide": false}))).unwrap();
        assert!(rendered.contains("1080, \"height\": 1920"));
    }

    /// A template handed no variables is refused loudly rather than
    /// rendered with its own placeholders as literal text.
    #[test]
    fn a_template_without_variables_is_refused() {
        let text = r#"{"id": "funcut.quote", "duration": {{ seconds }}}"#;
        assert!(render(text, None).is_err());
        assert!(render(text, Some(&serde_json::Value::Null)).is_err());
    }

    /// A broken template says so with minijinja's own error, line number
    /// and all.
    #[test]
    fn a_broken_template_names_its_problem() {
        let text = r#"{"id": "{% for %}"#;
        let problem = render(text, Some(&serde_json::json!({}))).unwrap_err();
        assert!(problem.contains("template"), "{problem}");
    }
}
