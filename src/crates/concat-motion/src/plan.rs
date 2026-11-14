// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The plan's footage slots: the openings an edit promises a picture for.
//!
//! A segment says what the timeline plays; a slot says what one opening
//! of that timeline is *for* - the role it plays in the piece, the
//! treatment that fills it, the job its picture does, where it enters
//! against the words and the beat grid, how it leaves, and the rich
//! description a stock search ranks against. A plan carries slots beside
//! its segments; the slots are how a director shops for footage without
//! giving the search a bare keyword and hoping.
//!
//! The wire shape is the plan document's own camelCase (`slotId`,
//! `motionMin`, `durationS`), so a plan written by hand, by an agent or
//! by the editor reads the same words the validator rejects with.
//!
//! ```text
//! slot "hook.1": b-roll, illustrate, enter on_emphasis, exit on_beat
//!   description "a slow aerial drift across the city skyline at dusk"
//!   queries     ["city skyline aerial"]   <- fallback keywords only
//! ```

use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

use crate::route::BeatType;

/// The dramaturgical role the slot plays: the same seven acts the edit's
/// own [`BeatType`] names, because a slot spends a beat. The wire values
/// are the plan's (`"hook"`, `"setup"`, ...), which kebab-case spells
/// the same way a single word is spelled.
pub type SlotRole = BeatType;

/// What fills the slot's screen time.
///
/// `hold`, `graphic` and `black` are treatments the edit draws itself:
/// the A-roll stays, a card or chart carries the span, or the screen
/// goes dark on purpose. None of them search for stock. Only `broll`
/// shops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(inline))]
#[serde(rename_all = "snake_case")]
pub enum Treatment {
    /// The A-roll holds: the picture stays where it is.
    Hold,
    /// Stock footage covers the span.
    Broll,
    /// The edit draws the span itself - a chart, a card, a caption.
    Graphic,
    /// Black, on purpose - the dip, the breath, the act break.
    Black,
}

impl Treatment {
    /// The name the plan carries and the validator answers with.
    pub const fn name(self) -> &'static str {
        match self {
            Treatment::Hold => "hold",
            Treatment::Broll => "broll",
            Treatment::Graphic => "graphic",
            Treatment::Black => "black",
        }
    }
}

/// The work the slot's picture does for the edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(inline))]
#[serde(rename_all = "snake_case")]
pub enum SlotJob {
    /// Show what the words describe.
    Illustrate,
    /// Show the opposite the words sit against.
    Contrast,
    /// Land a beat - the punctuation mark the cut makes.
    Punctuate,
    /// Bridge two stretches the edit joins.
    Transition,
    /// Give the viewer a breath.
    Breathe,
}

/// When the slot's picture enters, relative to the words and the beat
/// grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(inline))]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// On the emphasis the words carry.
    OnEmphasis,
    /// Just before the line starts, so the picture is there to meet it.
    BeforeLine,
    /// On a beat the grid names.
    OnBeat,
    /// After the line has landed - the picture follows the words.
    AfterLine,
}

/// How the slot's screen time ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(inline))]
#[serde(rename_all = "snake_case")]
pub enum Exit {
    /// Before the next emphasis the words carry, so the picture leaves
    /// before it outstays its welcome.
    BeforeNextEmphasis,
    /// On a beat the grid names.
    OnBeat,
    /// Where the source clip itself cuts - the join hides in the source.
    SourceCut,
    /// Early, before the words are done with it: the picture owes the
    /// edit less than the span asks.
    Early,
}

/// A concise, film-specific reason for a slot's visual choice: what
/// this beat must communicate, the medium that carries it, and why that
/// medium is stronger than the plausible alternatives. Required in
/// practice for major beats; it must explain intent rather than repeat
/// a template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(inline))]
#[serde(rename_all = "camelCase")]
pub struct VisualRationale {
    /// What this beat must communicate or make the viewer feel.
    #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
    pub job: String,
    /// The selected visual medium.
    pub medium: Medium,
    /// Why this medium is stronger than the plausible alternatives for
    /// this beat.
    #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
    pub reason: String,
    /// The film-specific visual treatment: composition, palette, type,
    /// movement, texture, or camera language.
    #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
    pub style: String,
    /// Why and how the visual enters or leaves in relation to story,
    /// speech, and rhythm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition: Option<String>,
    /// A meaningful alternative if the selected asset, model, or renderer
    /// is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// The medium a slot's visual rationale selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(inline))]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    /// Stock or imported footage.
    Footage,
    /// A motion comp.
    Motion,
    /// A chart.
    Chart,
    /// Type on screen.
    Typography,
    /// Generated imagery.
    Generated,
    /// The A-roll holds.
    Hold,
    /// More than one medium together.
    Hybrid,
}

/// One opening the edit promises a picture for.
///
/// The `description` is the contract with the search: a rich
/// noun-plus-adjective string of twelve to twenty-five words that CLIP
/// ranks the candidates against. The `queries` are the fallback keyword
/// phrases only - what BM25 gets when the description's shortlist comes
/// back thin. A treatment the edit draws itself takes no queries at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(inline))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    /// The slot's own id, unique in the plan: `"hook.1"`, `"reveal.hero"`.
    /// The per-slot notes the critic sends back name it.
    #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
    pub slot_id: String,
    /// The dramaturgical role the slot plays - the same seven acts a
    /// segment's `act` names, because a slot spends a beat.
    pub role: SlotRole,
    /// What fills the slot's screen time. Only `broll` shops for stock;
    /// `hold`, `graphic` and `black` are treatments the edit draws itself
    /// and take no queries.
    pub treatment: Treatment,
    /// The work the slot's picture does for the edit.
    pub job: SlotJob,
    /// When the picture enters, against the words and the beat grid.
    pub entry: Entry,
    /// How the picture leaves.
    pub exit: Exit,
    /// The rich noun+adjective string the stock search ranks against,
    /// twelve to twenty-five words: what is in frame, what it is doing,
    /// what it feels like. The validator refuses anything under 40
    /// characters - it is what CLIP ranks against.
    #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
    pub description: String,
    /// The fallback keyword phrases only, for the BM25 pass. A treatment
    /// the edit draws itself (hold, graphic, black) takes none; broll
    /// takes at least one.
    #[cfg_attr(feature = "schema", schemars(inner(length(min = 1))))]
    pub queries: Vec<String>,
    /// Whether this slot is the shot the thumbnail and the first glance
    /// live on; a hero slot searches wider and ranks longer.
    pub hero: bool,
    /// The least motion the slot's clip may carry, on the library's own
    /// motion scale: a `breathe` slot wants stillness, a `punctuate`
    /// slot wants the opposite.
    #[cfg_attr(feature = "schema", schemars(range(min = 0)))]
    pub motion_min: f32,
    /// How long the slot's screen time runs, in seconds.
    pub duration_s: f32,
    /// A concise, film-specific reason for this visual choice. Required
    /// in practice for major beats; it must explain intent rather than
    /// repeat a template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_rationale: Option<VisualRationale>,
}

impl Slot {
    /// The shortest description the search can rank with, in characters.
    /// Forty characters is roughly the twelve words the brief asks for;
    /// anything shorter is a keyword wearing a description's clothes.
    pub const MIN_DESCRIPTION_CHARS: usize = 40;

    /// The per-slot contracts: the description carries enough picture to
    /// rank against; a treatment the edit draws itself takes no queries;
    /// b-roll takes at least one. The plan-level share of b-roll slots is
    /// the validator's, not one slot's.
    ///
    /// The error is the words the plan validator answers with: a slot
    /// that fails this fails `plan_validate`.
    pub fn validate(&self) -> Result<(), String> {
        let chars = self.description.chars().count();
        if chars < Self::MIN_DESCRIPTION_CHARS {
            return Err(format!(
                "slot `{}` description is {chars} characters; it is what CLIP ranks against and must be at least {}",
                self.slot_id,
                Self::MIN_DESCRIPTION_CHARS
            ));
        }
        match self.treatment {
            Treatment::Broll => {
                if self.queries.iter().all(|query| query.trim().is_empty()) {
                    return Err(format!(
                        "slot `{}` is b-roll; it takes at least one non-empty query",
                        self.slot_id
                    ));
                }
            }
            hold_or_drawn => {
                if self.queries.iter().any(|query| !query.trim().is_empty()) {
                    return Err(format!(
                        "slot `{}` is a {} treatment; the edit draws it itself and takes no search queries",
                        self.slot_id,
                        hold_or_drawn.name()
                    ));
                }
            }
        }
        Ok(())
    }
}

// The plan's `slots` array as a schema root: what the canonical
// edit-plan schema splices in for `slots`, generated from `PlanSlots`
// so the wire contract cannot drift from the types the validator
// deserializes (SCH-02). The rustdoc below is the schema's own
// description - the words an agent reads - so it stays the canonical
// text and nothing else.
///
/// The footage slots the edit spends: one opening at a time, with the
/// role it plays, the treatment that fills it, and the rich description
/// a stock search ranks against. Optional - a plan without slots is a
/// plan the segments alone describe. The b-roll slots must match the
/// b-roll segments one for one, and a segment that names its slot by
/// slotId must find it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PlanSlots(
    /// At most sixty openings: the plan is a shopping list, not a
    /// catalogue.
    #[cfg_attr(feature = "schema", schemars(length(max = 60)))]
    pub Vec<Slot>,
);

/// The share of the slots that go shopping for stock, 0.0 to 1.0.
///
/// This is an informational measurement only. There is intentionally no
/// universal target: the right balance depends on the film's story, source
/// material, evidence density, and chosen visual language.
pub fn broll_share(slots: &[Slot]) -> f32 {
    if slots.is_empty() {
        return 0.0;
    }
    slots
        .iter()
        .filter(|slot| slot.treatment == Treatment::Broll)
        .count() as f32
        / slots.len() as f32
}

/// The plan's `slots` array as a schema root: what the canonical
/// edit-plan schema splices in for `slots`, generated from
/// [`PlanSlots`] so the wire contract cannot drift from the types the
/// validator deserializes (SCH-02).
///
/// The returned value is the *property* schema - `{"type": "array", ...}`
/// with the `$schema` dialect marker stripped, ready to drop in as the
/// value of `properties.slots`.
#[cfg(feature = "schema")]
pub fn slots_json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(PlanSlots);
    let mut value = serde_json::to_value(&schema).expect("a schema is JSON");
    if let Some(object) = value.as_object_mut() {
        // The dialect marker and the newtype's own name stay on the
        // root: the canonical carries its own header, and the property
        // is `slots`, not `PlanSlots`.
        object.remove("$schema");
        object.remove("title");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A description that clears the ranking floor: nouns, adjectives,
    /// fifty-two characters.
    fn rich_description() -> String {
        "a slow aerial drift across the city skyline at dusk".to_owned()
    }

    /// A well-formed b-roll slot, for the tests to bend one field at a
    /// time.
    fn broll_slot() -> Slot {
        Slot {
            slot_id: "hook.1".into(),
            role: BeatType::Hook,
            treatment: Treatment::Broll,
            job: SlotJob::Illustrate,
            entry: Entry::OnEmphasis,
            exit: Exit::OnBeat,
            description: rich_description(),
            queries: vec!["city skyline aerial".into()],
            hero: false,
            motion_min: 1.0,
            duration_s: 2.5,
            visual_rationale: None,
        }
    }

    /// The done-when: a slot with an empty - or merely thin - description
    /// is rejected, and the rejection says what the description is for.
    #[test]
    fn a_slot_with_an_empty_description_is_rejected() {
        let mut slot = broll_slot();
        slot.description = String::new();
        let error = slot.validate().unwrap_err();
        assert!(error.contains("at least 40"), "{error}");

        slot.description = "city dusk".into();
        let error = slot.validate().unwrap_err();
        assert!(error.contains("9 characters"), "{error}");

        // And one clearing the floor passes clean.
        assert!(broll_slot().validate().is_ok());
    }

    /// A treatment the edit draws itself takes no queries; b-roll takes
    /// at least one.
    #[test]
    fn hold_takes_no_queries_and_broll_takes_one() {
        let mut slot = broll_slot();
        slot.treatment = Treatment::Hold;
        slot.queries = vec!["city skyline".into()];
        let error = slot.validate().unwrap_err();
        assert!(error.contains("no search queries"), "{error}");

        slot.treatment = Treatment::Broll;
        slot.queries = Vec::new();
        let error = slot.validate().unwrap_err();
        assert!(error.contains("at least one non-empty query"), "{error}");

        // A drawn treatment with an empty query list passes.
        slot.treatment = Treatment::Graphic;
        slot.queries = Vec::new();
        assert!(slot.validate().is_ok());
        // As does a hold with whitespace where a query would be: nothing
        // searchable was promised.
        slot.treatment = Treatment::Black;
        slot.queries = vec!["   ".into()];
        assert!(slot.validate().is_ok());
    }

    /// The share arithmetic the plan validator keeps its band with.
    #[test]
    fn the_broll_share_counts_the_shopping_slots() {
        let mut slots = vec![broll_slot(); 10];
        assert_eq!(broll_share(&slots), 1.0, "all shopping");
        for slot in &mut slots[3..] {
            slot.treatment = Treatment::Hold;
            slot.queries = Vec::new();
            assert!(slot.validate().is_ok());
        }
        let share = broll_share(&slots);
        assert!((share - 0.3).abs() < 1e-6, "{share}");
        // Thirty percent is the band's floor: valid, no share warning
        // past it, the validator's business from here.
        assert!(broll_share(&[]) == 0.0, "no slots, no shopping");
    }

    /// The wire shape is the plan's own camelCase, and the enums are the
    /// plan's own words.
    #[test]
    fn a_slot_round_trips_through_the_plan_s_json() {
        let slot = broll_slot();
        let json = serde_json::to_value(&slot).expect("serializes");
        assert_eq!(json["slotId"], "hook.1");
        assert_eq!(json["role"], "hook");
        assert_eq!(json["treatment"], "broll");
        assert_eq!(json["job"], "illustrate");
        assert_eq!(json["entry"], "on_emphasis");
        assert_eq!(json["exit"], "on_beat");
        assert_eq!(json["motionMin"], 1.0);
        assert_eq!(json["durationS"], 2.5);
        assert_eq!(json["hero"], false);
        let back: Slot = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, slot);

        // And the drawn treatments spell themselves the way the validator
        // answers: no kebab, no camel, the plan's own snake_case.
        let mut slot = broll_slot();
        slot.treatment = Treatment::Black;
        slot.exit = Exit::BeforeNextEmphasis;
        slot.job = SlotJob::Breathe;
        slot.entry = Entry::AfterLine;
        let json = serde_json::to_value(&slot).expect("serializes");
        assert_eq!(json["treatment"], "black");
        assert_eq!(json["exit"], "before_next_emphasis");
        assert_eq!(json["job"], "breathe");
        assert_eq!(json["entry"], "after_line");
        // The rationale rides its own camelCase keys, absent when None.
        let mut slot = broll_slot();
        slot.visual_rationale = Some(VisualRationale {
            job: "Land the reveal".into(),
            medium: Medium::Footage,
            reason: "The real machine says what no card could".into(),
            style: "Slow push, cool key light, shallow depth".into(),
            transition: Some("enters on the turn's emphasis".into()),
            fallback: None,
        });
        let json = serde_json::to_value(&slot).expect("serializes");
        assert_eq!(json["visualRationale"]["medium"], "footage");
        assert!(json["visualRationale"].get("fallback").is_none());
        let back: Slot = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, slot);
    }

    /// The wire shape the schema generator answers with is the types'
    /// own: the treatment enum, the 40-character floor's field, and the
    /// slot count ceiling the plan's schema carries.
    #[cfg(all(test, feature = "schema"))]
    #[test]
    fn the_slots_schema_is_generated_from_the_types() {
        let schema = slots_json_schema();
        assert_eq!(
            schema["type"], "array",
            "the property is an array: {schema}"
        );
        assert_eq!(
            schema["maxItems"], 60,
            "the plan carries at most sixty slots: {schema}"
        );
        let items = &schema["items"];
        assert_eq!(items["type"], "object");
        assert_eq!(
            items["additionalProperties"], false,
            "a slot takes no unknown fields: {items}"
        );
        let properties = items["properties"].as_object().expect("slot properties");
        for field in [
            "slotId",
            "role",
            "treatment",
            "job",
            "entry",
            "exit",
            "description",
            "queries",
            "hero",
            "motionMin",
            "durationS",
            "visualRationale",
        ] {
            assert!(properties.contains_key(field), "`{field}` is in the schema");
        }
        // The generated schema spells an enum as one const per variant
        // (each carrying its own doc), not as a bare `enum` array: the
        // values are the same words the wire carries.
        let treatment_words = |schema: &serde_json::Value| -> Vec<String> {
            schema["oneOf"]
                .as_array()
                .expect("the treatments are a oneOf of consts")
                .iter()
                .map(|variant| variant["const"].as_str().expect("a const").to_owned())
                .collect()
        };
        assert_eq!(
            treatment_words(&items["properties"]["treatment"]),
            ["hold", "broll", "graphic", "black"],
            "the treatments are the plan's own words"
        );
        assert_eq!(
            treatment_words(&items["properties"]["role"]),
            ["hook", "setup", "tension", "turn", "reveal", "proof", "cta"],
            "the roles are the seven acts"
        );
        // The schema's own floor is the wire's, not the validator's:
        // minLength 1 here, the 40-character refusal in `validate`.
        assert_eq!(items["properties"]["description"]["minLength"], 1);
        // A slot the schema accepts round-trips through the types.
        let slot = broll_slot();
        let wire = serde_json::to_value(&slot).expect("serializes");
        let back: Slot = serde_json::from_value(wire).expect("deserializes");
        assert_eq!(back, slot);
        // And the schema the plan carries accepts the same wire shape:
        // the array wraps the slot, the slot fits its items schema.
        let plan_wire = serde_json::to_value(PlanSlots(vec![slot])).expect("serializes");
        assert_eq!(plan_wire.as_array().map(Vec::len), Some(1));
    }
}
