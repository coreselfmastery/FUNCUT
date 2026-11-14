// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Tool input schemas, cut out of the API's own.
//!
//! [`concat_api::request_schema`] is the schema of every request, made by
//! `schemars` from the types: one alternative per method, and every type
//! a request carries under `$defs`. A tool that is a method takes that
//! method's alternative with `method` taken out; a tool that is one edit
//! command takes that command's alternative with `op` taken out and the
//! project's `path` put in. Nothing here describes a field: a field's
//! type, range and description are what its Rust type and doc comment
//! say, so a tool's schema changes when - and only when - the API does.
//!
//! References are inlined, because not every MCP client follows `$ref`;
//! the one that cannot be, the edit command a batch nests inside itself,
//! stays a reference with its definition carried along under `$defs`.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

/// The API's request schema, ready to cut tools out of.
pub struct Schemas {
    /// Every type a request carries, by name.
    defs: Map<String, Value>,
    /// Each method's alternative.
    methods: Vec<Value>,
}

impl Schemas {
    /// Generates the API's schema and indexes it.
    pub fn generate() -> Schemas {
        Schemas::from_root(concat_api::request_schema())
    }

    fn from_root(mut root: Value) -> Schemas {
        let defs = match root.get_mut("$defs").map(Value::take) {
            Some(Value::Object(defs)) => defs,
            _ => Map::new(),
        };
        let methods = alternatives(&root);
        Schemas { defs, methods }
    }

    /// The arguments of `method`, as a self-contained schema: the
    /// method's params, without `method`. `None` for a method the API does
    /// not have.
    pub fn method(&self, method: &str) -> Option<Value> {
        let variant = self
            .methods
            .iter()
            .find(|variant| tag_of(variant, "method") == Some(method))?;
        Some(self.finish(without(variant.clone(), "method")))
    }

    /// The arguments of the edit command `op`, plus the project's `path`
    /// it is applied to. `None` for a command the API does not have.
    pub fn command(&self, op: &str) -> Option<Value> {
        let variant = self
            .commands()
            .into_iter()
            .find(|variant| tag_of(variant, "op") == Some(op))?;
        let mut schema = without(variant, "op");
        add_required(&mut schema, "path", self.path_property());
        Some(self.finish(schema))
    }

    /// One type's schema by its Rust name - a `TextStyle`, say - inlined,
    /// for a tool that builds its own arguments around it.
    pub fn def(&self, name: &str) -> Option<Value> {
        let def = self.defs.get(name)?.clone();
        let mut kept = BTreeSet::new();
        let inlined = inline(def, &self.defs, &mut vec![name.to_owned()], &mut kept);
        // A type that nests itself would need its definitions carried; none
        // a tool builds around does, and `finish` is for whole schemas.
        Some(inlined)
    }

    /// Every edit command's alternative.
    fn commands(&self) -> Vec<Value> {
        self.defs
            .get("Command")
            .map(alternatives)
            .unwrap_or_default()
    }

    /// The `path` every project method takes, as the API describes it.
    fn path_property(&self) -> Value {
        self.method("project.get")
            .and_then(|schema| schema.pointer("/properties/path").cloned())
            .unwrap_or_else(|| json!({ "type": "string", "description": "The project folder." }))
    }

    /// Inlines every reference that can be, and carries the definitions of
    /// those that cannot. The carried ones - the whole edit vocabulary,
    /// for a tool that takes any command - are kept as the API writes
    /// them, each type once and referred to, with every description cut
    /// to its first sentence: complete, and a few thousand tokens of an
    /// agent's context rather than twenty.
    fn finish(&self, schema: Value) -> Value {
        let mut kept = BTreeSet::new();
        let mut schema = inline(schema, &self.defs, &mut Vec::new(), &mut kept);
        if schema.get("type").is_none() {
            schema["type"] = json!("object");
        }
        // The whole object is the tool's contract: a key it does not
        // name is a misspelling - `style.text` where the words go,
        // `trackId` on a command whose lane is `track` - and the request
        // layer reads one as silently absent, which is how an edit lands
        // half-read. The checker ([`crate::tools::Toolbox::call`]) refuses
        // one at the door; a nested style still tolerates its own old
        // names, which is what TextStyle's serde aliases are for.
        if let Some(object) = schema.as_object_mut()
            && !object.contains_key("additionalProperties")
        {
            object.insert("additionalProperties".to_owned(), json!(false));
        }
        if !kept.is_empty() {
            let mut defs = Map::new();
            let mut pending: Vec<String> = kept.into_iter().collect();
            while let Some(name) = pending.pop() {
                if defs.contains_key(&name) {
                    continue;
                }
                let Some(def) = self.defs.get(&name) else {
                    continue;
                };
                let body = compact(def.clone());
                pending.extend(
                    references(&body)
                        .into_iter()
                        .filter(|other| !defs.contains_key(other)),
                );
                defs.insert(name, body);
            }
            schema["$defs"] = Value::Object(defs);
        }
        schema
    }
}

/// Every `#/$defs/` name `schema` refers to.
fn references(schema: &Value) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut stack = vec![schema];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(object) => {
                if let Some(Value::String(reference)) = object.get("$ref")
                    && let Some(name) = reference.strip_prefix("#/$defs/")
                {
                    found.insert(name.to_owned());
                }
                stack.extend(object.values());
            }
            Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    found
}

/// A definition made short: descriptions to their first sentence, and the
/// defaults that are whole objects - a style's every field - left out,
/// the field's own description saying what absent means.
fn compact(schema: Value) -> Value {
    match schema {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter(|(key, value)| {
                    !(key == "default" && (value.is_object() || value.is_array()))
                })
                .map(|(key, value)| match (key.as_str(), value) {
                    ("description", Value::String(text)) => {
                        (key, Value::String(first_sentence(&text)))
                    }
                    (_, value) => (key, compact(value)),
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(compact).collect()),
        other => other,
    }
}

/// The first sentence of a doc comment, on one line.
fn first_sentence(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.find(". ") {
        Some(end) => flat[..=end].to_owned(),
        None => flat,
    }
}

/// The alternatives of a tagged enum's schema: `oneOf`, or `anyOf`.
fn alternatives(schema: &Value) -> Vec<Value> {
    ["oneOf", "anyOf"]
        .iter()
        .find_map(|key| schema.get(*key).and_then(Value::as_array))
        .cloned()
        .unwrap_or_default()
}

/// The constant an alternative's tag property holds.
fn tag_of<'a>(variant: &'a Value, tag: &str) -> Option<&'a str> {
    let property = variant.pointer(&format!("/properties/{tag}"))?;
    property.get("const").and_then(Value::as_str).or_else(|| {
        property
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(Value::as_str)
    })
}

/// An alternative without its tag: the property and its requirement.
fn without(mut variant: Value, tag: &str) -> Value {
    if let Some(properties) = variant.get_mut("properties").and_then(Value::as_object_mut) {
        properties.remove(tag);
    }
    if let Some(required) = variant.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|name| name.as_str() != Some(tag));
        if required.is_empty() {
            variant
                .as_object_mut()
                .map(|object| object.remove("required"));
        }
    }
    if variant.get("properties").is_none() {
        variant["properties"] = json!({});
    }
    variant
}

/// Adds a required property to an object schema.
fn add_required(schema: &mut Value, name: &str, property: Value) {
    schema["properties"][name] = property;
    let mut names = vec![json!(name)];
    if let Some(Value::Array(rest)) = schema.get_mut("required").map(Value::take) {
        names.extend(
            rest.into_iter()
                .filter(|other| other.as_str() != Some(name)),
        );
    }
    schema["required"] = Value::Array(names);
}

/// The types that stay references, defined once under `$defs`: the edit
/// command, which nests inside itself and is large enough that a tool
/// taking one should not carry it twice.
const SHARED: &[&str] = &["Command"];

/// `schema` with every `#/$defs/` reference replaced by its definition,
/// except one to a [`SHARED`] type or inside its own definition - which
/// stays, and is noted in `kept` - and with the `$schema` and `$defs` of a
/// root dropped.
fn inline(
    schema: Value,
    defs: &Map<String, Value>,
    stack: &mut Vec<String>,
    kept: &mut BTreeSet<String>,
) -> Value {
    match schema {
        Value::Object(mut object) => {
            object.remove("$schema");
            object.remove("$defs");
            if let Some(Value::String(reference)) = object.get("$ref")
                && let Some(name) = reference.strip_prefix("#/$defs/")
            {
                let name = name.to_owned();
                if SHARED.contains(&name.as_str())
                    || stack.contains(&name)
                    || !defs.contains_key(&name)
                {
                    kept.insert(name);
                    return Value::Object(object);
                }
                object.remove("$ref");
                stack.push(name.clone());
                let body = inline(defs[&name].clone(), defs, stack, kept);
                stack.pop();
                // What sat beside the reference - a field's own
                // description, its default - wins over the type's.
                let mut merged = match body {
                    Value::Object(body) => body,
                    other => return other,
                };
                for (key, value) in object {
                    let value = inline(value, defs, stack, kept);
                    merged.insert(key, value);
                }
                return Value::Object(merged);
            }
            Value::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key, inline(value, defs, stack, kept)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| inline(item, defs, stack, kept))
                .collect(),
        ),
        other => other,
    }
}

/// A small JSON Schema checker: enough of the 2020-12 vocabulary used by
/// the MCP schemas here - types, properties, required, items, array/string
/// limits, enums and constants, combinators and local references - for
/// tool round-trip tests and local plan validation.
pub(crate) fn check(schema: &Value, value: &Value, root: &Value, at: &str) -> Result<(), String> {
    let fail = |why: String| Err(format!("{at}: {why}"));
    let Some(object) = schema.as_object() else {
        return match schema {
            Value::Bool(false) => fail("nothing is allowed here".to_owned()),
            _ => Ok(()),
        };
    };
    if let Some(Value::String(reference)) = object.get("$ref") {
        let name = reference.trim_start_matches("#/$defs/");
        let Some(def) = root.pointer(&format!("/$defs/{name}")) else {
            return fail(format!("no definition {name}"));
        };
        check(def, value, root, at)?;
    }
    if let Some(kind) = object.get("type") {
        let kinds: Vec<&str> = match kind {
            Value::String(kind) => vec![kind.as_str()],
            Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let fits = |kind: &str| match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            "number" => value.is_number(),
            "integer" => {
                value.as_i64().is_some()
                    || value.as_u64().is_some()
                    || value
                        .as_f64()
                        .is_some_and(|number| number.is_finite() && number.fract() == 0.0)
            }
            _ => true,
        };
        if !kinds.iter().any(|kind| fits(kind)) {
            return fail(format!("{value} is not {kinds:?}"));
        }
    }
    if let Some(constant) = object.get("const")
        && constant != value
    {
        return fail(format!("{value} is not {constant}"));
    }
    if let Some(Value::Array(allowed)) = object.get("enum")
        && !allowed.contains(value)
    {
        return fail(format!("{value} is not one of {allowed:?}"));
    }
    if let Some(number) = value.as_f64() {
        if let Some(minimum) = object.get("minimum").and_then(Value::as_f64)
            && number < minimum
        {
            return fail(format!("{number} is under {minimum}"));
        }
        if let Some(maximum) = object.get("maximum").and_then(Value::as_f64)
            && number > maximum
        {
            return fail(format!("{number} is over {maximum}"));
        }
    }
    if let Value::String(text) = value {
        if let Some(minimum) = object.get("minLength").and_then(Value::as_u64)
            && text.chars().count() < minimum as usize
        {
            return fail(format!("string is shorter than {minimum} characters"));
        }
        if let Some(maximum) = object.get("maxLength").and_then(Value::as_u64)
            && text.chars().count() > maximum as usize
        {
            return fail(format!("string is longer than {maximum} characters"));
        }
    }
    if let Value::Array(items) = value {
        if let Some(minimum) = object.get("minItems").and_then(Value::as_u64)
            && items.len() < minimum as usize
        {
            return fail(format!("array has fewer than {minimum} items"));
        }
        if let Some(maximum) = object.get("maxItems").and_then(Value::as_u64)
            && items.len() > maximum as usize
        {
            return fail(format!("array has more than {maximum} items"));
        }
    }
    if let Value::Object(fields) = value {
        let properties = object.get("properties").and_then(Value::as_object);
        if let Some(Value::Array(required)) = object.get("required") {
            for name in required.iter().filter_map(Value::as_str) {
                if !fields.contains_key(name) {
                    return fail(format!("{name} is required"));
                }
            }
        }
        for (name, field) in fields {
            match properties.and_then(|properties| properties.get(name)) {
                Some(property) => check(property, field, root, &format!("{at}.{name}"))?,
                None => {
                    if let Some(extra) = object.get("additionalProperties") {
                        check(extra, field, root, &format!("{at}.{name}"))?;
                    }
                }
            }
        }
    }
    if let (Value::Array(items), Some(item)) = (value, object.get("items")) {
        for (index, element) in items.iter().enumerate() {
            check(item, element, root, &format!("{at}[{index}]"))?;
        }
    }
    if let Some(Value::Array(all)) = object.get("allOf") {
        for part in all {
            check(part, value, root, at)?;
        }
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(choices)) = object.get(key) {
            let fitting = choices
                .iter()
                .filter(|choice| check(choice, value, root, at).is_ok())
                .count();
            let ok = if key == "oneOf" {
                fitting == 1
            } else {
                fitting >= 1
            };
            if !ok {
                let why: Vec<String> = choices
                    .iter()
                    .filter_map(|choice| check(choice, value, root, at).err())
                    .take(3)
                    .collect();
                return fail(format!("{fitting} alternatives of {key} fit: {why:?}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_is_cut_out_without_its_tag_and_with_its_references_inlined() {
        let schemas = Schemas::generate();
        let open = schemas.method("project.open").expect("a method");
        assert_eq!(open["type"], "object");
        assert!(open["properties"].get("method").is_none());
        assert_eq!(open["required"], json!(["path"]));
        assert!(schemas.method("project.nope").is_none());
        let set_video = schemas.method("project.setVideo").expect("a method");
        assert!(
            set_video
                .pointer("/properties/video/properties/width")
                .is_some(),
            "VideoSettings inlined: {set_video}"
        );
        assert!(!set_video.to_string().contains("$ref"));
    }

    #[test]
    fn a_command_carries_the_project_path() {
        let schemas = Schemas::generate();
        let trim = schemas.command("trimClip").expect("a command");
        assert_eq!(
            trim.pointer("/properties/path/type"),
            Some(&json!("string"))
        );
        assert!(trim["properties"].get("op").is_none());
        let required = trim["required"].as_array().expect("required");
        assert!(required.contains(&json!("path")) && required.contains(&json!("clipId")));
        assert!(schemas.command("doTheThing").is_none());
    }

    #[test]
    fn the_tool_list_fits_an_agents_context() {
        // Every tool's schema may sit in an agent's context for the whole
        // session. Two budgets hold it: the WHOLE catalogue's, and - the
        // one an agent actually meets - the director's default surface's.
        // The history this guard carries: 172k at the model tools, 180k
        // at the motion five, 253,800 at 95 tools, 263,742 at 99,
        // 268,680 at 100, and 320,624 at 133 - a growth the descriptions
        // themselves were two-thirds of, at 75,611 characters of prose,
        // 569 a tool, while the schemas carried the parameters. The day
        // this comment was written for, the descriptions were cut to one
        // line each (75,611 -> under 9,000) and the thirty-three
        // consumption-layer tools were grouped behind five family doors
        // (133 -> 105), and the cap was re-measured on the result.
        // Descriptions stay one line: 200 characters is the ceiling a
        // tool's WHAT-and-WHEN owes the list; the parameters, their
        // units and their traps live in the schema, where an agent reads
        // them when it calls.
        //
        // The whole-catalogue cap rose once since, 240,000 -> 242,000 at
        // 109 tools: the catalogue gained four real tools (the footage
        // layer's two, timeline_seek, motion_import) after the cap was
        // measured at 105, and the growth is function, not prose. It
        // rose again, 242,000 -> 243,000, for the export surface's own
        // growth: the VAAPI hardware choice and the queue's timeline
        // picker sit in the export schema the way every option does -
        // 242,070 measured over 109 tools on the merged tree, function
        // again, no prose. And once more, 243,000 -> 246,000 at 110: the
        // footage planner's own door (footage_plan_suggest) opened the
        // engine the continuity gate gates, one schema of ~2,050
        // characters - function again. And 246,000 -> 247,000, still at
        // 110: export.status learned to be asked by what a job was
        // started on (`path`, beside the job's own id) - the ask an
        // unattended run makes, its program written before the job was
        // minted - and both waits carry the field; function. And once
        // more, 247,000 -> 248,000, still at 110: the plan schema's
        // audio block says the runner's defaults aloud - the
        // transcriber's base.en, the voice's kokoro-int8, the bed's
        // -12 duck, every knob a plan names when it wants another
        // answer - the block bone-dry, its one hint a maxLength the
        // runtime already enforced; schema the catalogue carries for
        // the agent that plans, function not prose. The
        // director's default surface - what `concat --mcp` serves an
        // agent that names no list, and the budget that matters by
        // default - keeps its own, tighter cap below.
        let toolbox = crate::tools::Toolbox::new();
        let sizes: Vec<(String, usize)> = toolbox
            .list()
            .iter()
            .map(|tool| (tool["name"].to_string(), tool.to_string().len()))
            .collect();
        let total: usize = sizes.iter().map(|(_, size)| size).sum();
        eprintln!(
            "CONTEXT whole catalogue: {total} over {} tools",
            sizes.len()
        );
        assert!(total < 248_000, "{total}: {sizes:?}");
        for (name, size) in &sizes {
            assert!(*size < 64_000, "{name}: {size}");
        }
        // One line, every description: the bloat this guard buried.
        for tool in toolbox.list() {
            let name = tool["name"].as_str().unwrap_or_default();
            let description = tool["description"].as_str().unwrap_or_default();
            assert!(
                description.len() <= 200 && !description.contains('\n'),
                "{name}'s description is {}/200 characters or spans lines",
                description.len()
            );
        }
        // The director's default surface: what `concat --mcp` serves an
        // agent that names no list. It must stay a surface an agent can
        // hold - the specialist tools (the farm, the deep motion reads,
        // the debugging aids) are the whole-catalogue budget's business.
        let director: usize = toolbox
            .list()
            .iter()
            .filter(|tool| {
                crate::protocol::DIRECTOR_ALLOWLIST
                    .contains(&tool["name"].as_str().unwrap_or_default())
            })
            .map(|tool| tool.to_string().len())
            .sum();
        eprintln!(
            "CONTEXT director surface: {director} over {} tools",
            crate::protocol::DIRECTOR_ALLOWLIST.len()
        );
        // 230,000 -> 232,000 with the footage planner's door: the
        // director shops for coverage with it, so it belongs on the
        // surface, and the surface's budget carries its schema. And
        // 232,000 -> 233,000 with export.status's `path`: the director
        // waits on its exports and analyses, and the waits take the ask
        // by what the job was started on. And 233,000 -> 234,000 with
        // the plan schema's audio defaults: the director plans the take
        // and the bed, and the block that names their knobs rides
        // plan_validate onto the surface. And 234,000 -> 235,000 with
        // edit.apply's `idempotencyKey`: the director's placements name
        // themselves, so a resumed run cannot double what a finished one
        // already placed (IDEM) - one field on the one edit tool.
        assert!(
            director < 235_000,
            "the director's default surface is {director} characters - too much for an agent to hold"
        );
    }

    #[test]
    fn checker_observes_array_bounds_and_additional_properties() {
        let array_schema = json!({
            "type": "array",
            "minItems": 1,
            "maxItems": 2,
            "items": { "type": "string" }
        });
        assert!(check(&array_schema, &json!([]), &array_schema, "array").is_err());
        assert!(
            check(
                &array_schema,
                &json!(["a", "b", "c"]),
                &array_schema,
                "array"
            )
            .is_err()
        );
        assert!(check(&array_schema, &json!(["a"]), &array_schema, "array").is_ok());

        let object_schema = json!({
            "type": "object",
            "properties": { "known": { "type": "string" } },
            "additionalProperties": false
        });
        assert!(
            check(
                &object_schema,
                &json!({ "unknown": 1 }),
                &object_schema,
                "object"
            )
            .is_err()
        );
        assert!(
            check(
                &object_schema,
                &json!({ "known": "yes" }),
                &object_schema,
                "object"
            )
            .is_ok()
        );
        let open_schema = json!({
            "type": "object",
            "properties": { "known": { "type": "string" } }
        });
        assert!(
            check(
                &open_schema,
                &json!({ "unknown": 1 }),
                &open_schema,
                "object"
            )
            .is_ok()
        );

        let integer_schema = json!({ "type": "integer" });
        assert!(check(&integer_schema, &json!(3.0), &integer_schema, "integer").is_ok());
        assert!(check(&integer_schema, &json!(3.5), &integer_schema, "integer").is_err());
    }

    #[test]
    fn a_batch_keeps_the_one_reference_it_must_and_carries_its_definition() {
        let schemas = Schemas::generate();
        let batch = schemas.command("batch").expect("a command");
        assert_eq!(
            batch.pointer("/properties/commands/items/$ref"),
            Some(&json!("#/$defs/Command"))
        );
        let command = batch.pointer("/$defs/Command").expect("carried");
        assert!(
            command
                .get("oneOf")
                .or_else(|| command.get("anyOf"))
                .is_some()
        );
        let sample = json!({
            "path": "/p",
            "commands": [
                { "op": "addTrack" },
                { "op": "batch", "commands": [{ "op": "addTimeline" }] }
            ]
        });
        check(&batch, &sample, &batch, "batch").expect("fits");
    }
}
