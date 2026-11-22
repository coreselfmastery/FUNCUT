// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The plan's schemas, generated rather than hand-edited (SCH-02).
//!
//! The canonical edit-plan schema - the document `plan_validate` checks
//! a plan against at runtime, and the template the director skill reads -
//! used to be a hand-maintained JSON file beside a hand-written `json!`
//! summary of itself in `tools.rs`. The two drifted: the compact summary
//! carried `bgm.mood` and `voiceover.whisperModel` months before the
//! canonical did, and neither was backed by the Rust types that actually
//! deserialize a plan's slots.
//!
//! Now the schema is assembled: a template carries the parts only prose
//! can describe, and the `slots` section is generated from
//! `concat_motion::plan`'s types through schemars, so the wire contract
//! cannot drift from what the validator deserializes. The compact
//! `plan_validate` tool schema is projected from the assembled canonical,
//! so it cannot drift from the canonical either. Both artifacts are
//! committed, and the drift test below refuses a commit in which either
//! one no longer matches what the generator produces.
//!
//! Regenerate with `cargo run -p concat-mcp --bin plan-schema` from
//! `src/`; check without writing with `--check`.

use serde_json::{Map, Value, json};

/// The canonical schema's template: every section of the plan document
/// that prose describes by hand, with the `slots` property marked for
/// generation from the types.
const TEMPLATE: &str = include_str!("../schema/edit_plan.template.json");

/// The committed canonical artifact: what `plan_validate` checks a plan
/// against at runtime, and the template the director skill reads. The
/// drift test refuses a commit where this is not what the generator
/// produces.
pub const CANONICAL: &str =
    include_str!("../../../../.claude/skills/funcut-director/templates/edit_plan.schema.json");

/// The committed compact artifact: the `plan_validate` input schema an
/// MCP client sees - the canonical's own shape with the sections a client
/// does not need to hold dropped and the descriptions thinned, so a full
/// plan template stays out of every client's tool list.
pub const COMPACT: &str = include_str!("../schema/plan_validate.schema.json");

/// The canonical edit-plan schema, assembled: the template with its
/// `slots` marker replaced by the schema generated from
/// `concat_motion::plan`'s types.
pub fn canonical() -> Value {
    let mut schema: Value = serde_json::from_str(TEMPLATE).expect("the canonical template is JSON");
    let slots = schema
        .pointer_mut("/properties/slots")
        .expect("the template carries the slots property");
    assert_eq!(
        slots.get("$generate").and_then(Value::as_str),
        Some("plan-slots"),
        "the template's slots marker is the generator's only door"
    );
    *slots = concat_motion::plan::slots_json_schema();
    schema
}

/// The canonical schema as it is committed: pretty-printed, one trailing
/// newline, byte-for-byte what `plan-schema` writes.
pub fn canonical_pretty() -> String {
    let mut text = serde_json::to_string_pretty(&canonical()).expect("a schema is JSON");
    text.push('\n');
    text
}

/// The sections of the plan document the compact projection drops: what
/// an agent calling `plan_validate` does not need to hold, because the
/// canonical schema re-checks the whole plan at runtime anyway.
const COMPACT_DROPS: &[&str] = &[
    "title",
    "niche",
    "captions",
    "export",
    "maxSegmentSeconds",
    "text",
    "slots",
    // The style bible and retention contract are validated at runtime and
    // documented in the canonical schema; the compact plan_validate input
    // schema keeps them accepted but does not spend client context on them.
    "style",
    "retention",
];

/// What the compact projection keeps of a segment: the timeline facts
/// and the visual. The emotion reads, the transitions, the comps and the
/// SFX are the canonical schema's business.
const SEGMENT_KEEP: &[&str] = &["start", "end", "visual"];

/// What the compact projection keeps of a segment's visual.
const VISUAL_KEEP: &[&str] = &[
    "type",
    "query",
    "description",
    "queries",
    "hero",
    "motionMin",
    "shotScale",
    "category",
    "storyboard",
    "clipId",
    "mediaId",
    "file",
];

/// What the compact projection keeps of a section.
const SECTION_KEEP: &[&str] = &["role", "start", "end"];

/// A schema node with its documentation stripped and its constraints
/// kept: the compact projection's shape. A key named `description` is a
/// documentation keyword wherever it appears as a schema's own key - but
/// a *property* named `description` (a visual's semantic shot
/// description, a brief's fields) lives inside a `properties` block,
/// where the name is a field's name and only its schema is stripped.
fn bare(node: &Value) -> Value {
    let Value::Object(fields) = node else {
        return node.clone();
    };
    let mut out = Map::new();
    for (key, value) in fields {
        match key.as_str() {
            "description" | "$schema" | "$id" | "title" => {}
            "properties" => {
                let mut properties = Map::new();
                if let Some(fields) = value.as_object() {
                    for (name, schema) in fields {
                        properties.insert(name.clone(), bare(schema));
                    }
                }
                out.insert(key.clone(), Value::Object(properties));
            }
            "items" | "additionalProperties" => {
                out.insert(key.clone(), bare(value));
            }
            "oneOf" | "anyOf" | "allOf" => {
                let alternatives = value
                    .as_array()
                    .map(|list| list.iter().map(bare).collect::<Vec<_>>());
                out.insert(key.clone(), Value::Array(alternatives.unwrap_or_default()));
            }
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(out)
}

/// Keep only the named properties of an object schema's `properties`
/// block; everything else about the node (its required list, its own
/// constraints) stays as the canonical has it.
fn keep_only(node: &mut Value, fields: &[&str]) {
    if let Some(properties) = node.get_mut("properties").and_then(Value::as_object_mut) {
        properties.retain(|name, _| fields.contains(&name.as_str()));
    }
}

/// The compact `plan_validate` input schema, projected from the
/// assembled canonical: the same required list, the same constraints on
/// what stays, the dropped sections gone, and the descriptions thinned
/// to the five a client actually needs to read.
pub fn compact() -> Value {
    let mut plan = bare(&canonical());
    {
        let object = plan
            .as_object_mut()
            .expect("the canonical root is an object schema");
        object.insert(
            "description".to_owned(),
            json!("Canonical edit plan; validated at runtime."),
        );
        let properties = object
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .expect("the canonical root has properties");
        for name in COMPACT_DROPS {
            properties.remove(*name);
        }
        keep_only(
            properties
                .get_mut("segments")
                .and_then(|segments| segments.get_mut("items"))
                .expect("segments carry items"),
            SEGMENT_KEEP,
        );
        keep_only(
            properties
                .get_mut("segments")
                .and_then(|segments| segments.get_mut("items"))
                .and_then(|items| items.get_mut("properties"))
                .and_then(|properties| properties.get_mut("visual"))
                .expect("a segment carries a visual"),
            VISUAL_KEEP,
        );
        keep_only(
            properties
                .get_mut("sections")
                .and_then(|sections| sections.get_mut("items"))
                .expect("sections carry items"),
            SECTION_KEEP,
        );
        // The five descriptions a client needs: the audio knobs (their
        // defaults are a contract), the reused-clip id, the brief and
        // the decision log (their gates refuse plans), and the
        // storyboard's door.
        if let Some(audio) = properties.get_mut("audio").and_then(Value::as_object_mut) {
            audio.insert(
                "description".to_owned(),
                json!("Voiceover and bed. Defaults: whisperModel base.en, TTS model kokoro-int8, bgm.duckDb -12; the bed shops with query, falling back to mood."),
            );
        }
        if let Some(audio_clip_id) = properties
            .get_mut("audioClipId")
            .and_then(Value::as_object_mut)
        {
            audio_clip_id.insert(
                "description".to_owned(),
                json!("Existing audio clip id to reuse; its timing must match the plan."),
            );
        }
        if let Some(brief) = properties
            .get_mut("creativeBrief")
            .and_then(Value::as_object_mut)
        {
            brief.insert(
                "description".to_owned(),
                json!("Brief: audience, platform, goal, thesis, curiosity gap, payoff, emotional arc, style, tone, brand kit, constraints, avoidances, monetized flag. Missing brief returns blockers. Monetized true arms the licence gates: OpenVid stock and MobyGratis beds are refused."),
            );
        }
        if let Some(signal_log) = properties
            .get_mut("signalLog")
            .and_then(Value::as_object_mut)
        {
            signal_log.insert(
                "description".to_owned(),
                json!("The decision log: for each cut, caption, graphic, BGM and transition choice, which voice_signals signals drove it. An entry is {at: seconds, decision: cut|caption|graphic|bgm|transition|pacing, signals: [names from the profile's decisionPolicy], confidence: 0-1, note: the basis}. An entry with no confidence, confidence 0, or no note is refused; a cut entry must land on a segment boundary; and a plan with a voiceover and no signalLog is not creativeReady."),
            );
        }
    }
    if let Some(storyboard) = plan
        .pointer_mut("/properties/segments/items/properties/visual/properties/storyboard")
        .and_then(Value::as_object_mut)
    {
        storyboard.insert(
            "description".to_owned(),
            json!("Complete motion_reel storyboard for text/title segments."),
        );
    }
    json!({
        "type": "object",
        "properties": { "plan": plan },
        "required": ["plan"],
        "additionalProperties": false,
    })
}

/// The compact schema as it is committed: pretty-printed, one trailing
/// newline, byte-for-byte what `plan-schema` writes.
pub fn compact_pretty() -> String {
    let mut text = serde_json::to_string_pretty(&compact()).expect("a schema is JSON");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drift test: the committed artifacts are what the types and
    /// the template generate. A hand edit to either committed schema -
    /// or a change to the plan types the artifacts were not regenerated
    /// for - fails here, naming the command that reconciles them.
    #[test]
    fn the_committed_plan_schemas_are_what_the_types_generate() {
        assert_eq!(
            canonical_pretty(),
            CANONICAL,
            "the committed canonical edit-plan schema drifted from the generator - run `cargo run -p concat-mcp --bin plan-schema` from src/"
        );
        assert_eq!(
            compact_pretty(),
            COMPACT,
            "the committed plan_validate schema drifted from the generator - run `cargo run -p concat-mcp --bin plan-schema` from src/"
        );
    }

    /// The drift test is not vacuous: a hand edit of either committed
    /// schema - a constraint nudged, an enum variant slipped in - no
    /// longer equals what the generator produces.
    #[test]
    fn a_hand_edit_of_the_committed_schema_fails_the_drift_test() {
        let mut hand_edited: Value =
            serde_json::from_str(CANONICAL).expect("the committed canonical parses");
        hand_edited["properties"]["segments"]["maxItems"] = json!(499);
        let hand_edited = serde_json::to_string_pretty(&hand_edited).expect("pretty");
        assert_ne!(
            hand_edited,
            canonical_pretty(),
            "a nudged constraint must not equal the generated canonical"
        );

        let mut hand_edited: Value =
            serde_json::from_str(COMPACT).expect("the committed compact parses");
        hand_edited["properties"]["plan"]["properties"]["aspect"]["enum"]
            .as_array_mut()
            .expect("the aspect enum")
            .pop();
        let hand_edited = serde_json::to_string_pretty(&hand_edited).expect("pretty");
        assert_ne!(
            hand_edited,
            compact_pretty(),
            "a slipped-out enum variant must not equal the generated compact"
        );
    }

    /// A plan the executor runs passes the generated canonical schema,
    /// slots included: the director's own example, and the same example
    /// carrying a slot, both fit what the types generated.
    #[test]
    fn the_generated_canonical_schema_accepts_the_director_s_example() {
        let schema = canonical();
        let plan: Value = serde_json::from_str(include_str!(
            "../../../../.claude/skills/funcut-director/templates/edit_plan.example.json"
        ))
        .expect("valid example JSON");
        crate::schema::check(&schema, &plan, &schema, "plan")
            .expect("the example fits the generated canonical schema");

        // The example carries no slots; a plan that does must fit the
        // generated slots section, and the slot must round-trip through
        // the very types the section was generated from.
        let mut with_slots = plan;
        with_slots["slots"] = json!([{
            "slotId": "hook.1",
            "role": "hook",
            "treatment": "broll",
            "job": "illustrate",
            "entry": "on_emphasis",
            "exit": "on_beat",
            "description": "a slow aerial drift across the city skyline at dusk, camera gliding left",
            "queries": ["city skyline aerial"],
            "hero": false,
            "motionMin": 1.0,
            "durationS": 2.5
        }]);
        crate::schema::check(&schema, &with_slots, &schema, "plan")
            .expect("a plan with slots fits the generated schema");
        let slot: concat_motion::plan::Slot =
            serde_json::from_value(with_slots["slots"][0].clone())
                .expect("the slot the schema accepted deserializes");
        assert!(slot.validate().is_ok());

        // A slot the types refuse is refused by the schema first: the
        // treatments are the plan's own four words, not whatever a hand
        // edit left behind.
        let mut shopped = with_slots;
        shopped["slots"][0]["treatment"] = json!("shopping");
        assert!(
            crate::schema::check(&schema, &shopped, &schema, "plan").is_err(),
            "an invented treatment does not fit the generated schema"
        );
    }

    /// The compact projection stays a summary an agent can hold: the
    /// dropped sections stay dropped, the kept constraints stay kept,
    /// and the wrapper is the one field the tool takes.
    #[test]
    fn the_compact_projection_keeps_the_surface_small_and_true() {
        let compact = compact();
        assert_eq!(compact["type"], "object");
        assert_eq!(compact["required"], json!(["plan"]));
        assert_eq!(compact["additionalProperties"], false);
        let plan = &compact["properties"]["plan"];
        for dropped in COMPACT_DROPS {
            assert!(
                plan["properties"].get(dropped).is_none(),
                "`{dropped}` stays out of the compact projection"
            );
        }
        for kept in ["aspect", "mode", "sections", "segments", "audio", "grade"] {
            assert!(
                plan["properties"].get(kept).is_some(),
                "`{kept}` stays in the compact projection"
            );
        }
        // The compact keeps the canonical's required list and the
        // segment ceiling, and drops the canonical's own dressing.
        assert_eq!(
            plan["required"],
            canonical()["required"],
            "the compact keeps the canonical's required list"
        );
        assert_eq!(
            plan["properties"]["segments"]["maxItems"], 500,
            "the segment ceiling rides the projection"
        );
        assert!(
            plan.get("$schema").is_none() && plan.get("title").is_none(),
            "the canonical's dressing stays on the canonical"
        );
        // The constraints a client can rely on survive the projection;
        // the prose does not. The description's 40-character floor moved
        // to the runtime rule - an empty visual.description with a 40+
        // spoken line is a valid plan the schema must not reject - so the
        // constraint the compact still carries is the query's.
        let visual = &plan["properties"]["segments"]["items"]["properties"]["visual"];
        assert_eq!(visual["properties"]["query"]["minLength"], 1);
        assert!(
            visual["properties"]["description"]
                .get("minLength")
                .is_none(),
            "the description's floor is the runtime rule, with its spoken fallback"
        );
        assert!(visual["properties"].get("preset").is_none());
    }
}
