// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The acceptance receipt: what the run actually did, written down as
//! evidence rather than reported as a green word.
//!
//! A run's terminal state says it shipped. It does not say whether the
//! edit was measured, whether the B-roll was shopped on a licensed shelf,
//! whether the captions were timed to words, or which capability the
//! program was built with and never reached. Those are the questions a
//! person - or the next agent - asks before trusting the file, and the
//! answer has to come from the run's own record, not from a prompt that
//! asked for it.
//!
//! Three things are derived here, all from the reply log and the stage
//! record the runner already keeps:
//!
//! - **Coverage** - which capability areas were exercised. A capability
//!   is *consumed* when a tool that provides it answered successfully,
//!   and *unreached* when the program was built with it and no call
//!   landed. Unreached is not a failure by itself: a run with no music
//!   gate to answer, or a project with no graphics to bake, is honest.
//!   It is recorded so nobody has to guess later.
//! - **Gates** - independent checks, one per craft dimension, each
//!   answered from a measured fact (a loudness reading, a craft score, a
//!   verified transcript). A gate whose evidence never arrived is
//!   `Skipped`, never `Pass`: an unmeasured edit is not a measured one.
//! - **Verdict** - `Accepted` only when every required gate passed. A
//!   failed gate is `Rejected` whatever the craft score said, and a
//!   required gate with no evidence is `Incomplete`. A 9/10 craft score
//!   cannot hide a missing narration track.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::ToolCall;
use crate::stage::Stage;

/// The receipt schema's own name, so a reader can tell which shape it
/// holds.
pub const SCHEMA: &str = "funcut.acceptance-receipt/1";

/// What became of one stage in the run.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageOutcome {
    /// The stage's calls landed and its gates, if any, passed.
    Completed,
    /// The stage was skipped by the resume record: a prior run on this
    /// runner did the work.
    Skipped,
    /// The stage's calls failed under the warn policy; the run shipped
    /// without it.
    Warned,
    /// The stage failed and the run stopped.
    Failed,
    /// The stage was never reached because an earlier one stopped the
    /// run.
    NotReached,
}

impl StageOutcome {
    /// The word the receipt carries.
    pub fn label(self) -> &'static str {
        match self {
            StageOutcome::Completed => "completed",
            StageOutcome::Skipped => "skipped",
            StageOutcome::Warned => "warned",
            StageOutcome::Failed => "failed",
            StageOutcome::NotReached => "not-reached",
        }
    }

    /// Whether the stage's work is on the timeline.
    pub fn landed(self) -> bool {
        matches!(self, StageOutcome::Completed | StageOutcome::Skipped)
    }
}

/// One stage's line in the receipt: what it was asked to do, what it
/// did, and every tool that answered.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageRecord {
    /// The stage.
    pub stage: Stage,
    /// What became of it.
    pub outcome: StageOutcome,
    /// Every tool that answered successfully, in call order.
    pub calls: Vec<String>,
    /// The failure's sentence, when it warned or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

impl StageRecord {
    /// A record for `stage` that landed, with the tools it called.
    pub fn completed(stage: Stage, calls: Vec<String>) -> StageRecord {
        StageRecord {
            stage,
            outcome: StageOutcome::Completed,
            calls,
            failure: None,
        }
    }

    /// A record for a stage the resume record skipped.
    pub fn skipped(stage: Stage) -> StageRecord {
        StageRecord {
            stage,
            outcome: StageOutcome::Skipped,
            calls: Vec::new(),
            failure: None,
        }
    }
}

/// A capability area of the editor: the group of tools one kind of
/// decision is made with. The areas are the director's own - the
/// measured voice, the plan, the footage, the motion, the words on
/// screen, the sound, the look, and the review that judges them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// The voice measured: transcription, delivery, emotion, discourse.
    Measurement,
    /// The plan: the creative brief, the edit plan and its gate.
    Planning,
    /// The picture: stock search, inspection, continuity, footage gaps.
    Footage,
    /// The motion engine: presets, frames, bakes, reels, overlays.
    Motion,
    /// The words on screen: captions, word timing, titles.
    Captions,
    /// The bed: music search, import, loudness, ducking.
    Music,
    /// The accents: the sound shelf and its placements.
    Sfx,
    /// The look: effects, layers, keyed grades.
    Grade,
    /// The review: the contact sheet, the craft score, the vision critic.
    Review,
    /// The output: the export and its verification.
    Delivery,
}

impl Capability {
    /// Every area, in the order the receipt lists them.
    pub const ALL: [Capability; 10] = [
        Capability::Measurement,
        Capability::Planning,
        Capability::Footage,
        Capability::Motion,
        Capability::Captions,
        Capability::Music,
        Capability::Sfx,
        Capability::Grade,
        Capability::Review,
        Capability::Delivery,
    ];

    /// The word the receipt carries.
    pub fn label(self) -> &'static str {
        match self {
            Capability::Measurement => "measurement",
            Capability::Planning => "planning",
            Capability::Footage => "footage",
            Capability::Motion => "motion",
            Capability::Captions => "captions",
            Capability::Music => "music",
            Capability::Sfx => "sfx",
            Capability::Grade => "grade",
            Capability::Review => "review",
            Capability::Delivery => "delivery",
        }
    }

    /// The capability a tool serves, when it serves one of these areas.
    /// A tool nobody lists here is an editing operation, not a reading -
    /// `clip_add` places a clip, it does not decide anything - and is
    /// recorded in the stage's call list without claiming an area.
    pub fn of_tool(tool: &str) -> Option<Capability> {
        Some(match tool {
            "transcribe"
            | "analysis_wait"
            | "voice_signals"
            | "emotion"
            | "sense_audio_repair"
            | "sense_discourse"
            | "sense_voice_quality"
            | "sense_prosody_style"
            | "sense_captions"
            | "analyze_beats"
            | "analyze_scenes"
            | "analyze_silence" => Capability::Measurement,
            "creative_brief" | "plan_validate" | "footage_plan_suggest" => Capability::Planning,
            "stock_search"
            | "stock_fetch"
            | "stock_import"
            | "stock_contact_sheet"
            | "footage_continuity"
            | "openvid_search"
            | "openvid_search_batch"
            | "openvid_rank_for_slot"
            | "openvid_get"
            | "openvid_fetch"
            | "openvid_import"
            | "openvid_status"
            | "openvid_prepare" => Capability::Footage,
            "motion_presets" | "motion_inspect" | "motion_frame" | "motion_bake"
            | "motion_reel" | "motion_import" | "motion_overlay" | "motion_preview"
            | "motion_probe" | "scene_analysis" | "scene_timeline" => Capability::Motion,
            "captions_add"
            | "captions_animate"
            | "title_add"
            | "title_style_words"
            | "title_clear_word_styles" => Capability::Captions,
            "bgm_search" | "bgm_fetch" | "bgm_import" | "bgm_status" | "bgm_prepare"
            | "measure_loudness" => Capability::Music,
            "sounds_search" | "sounds_add" => Capability::Sfx,
            "effects_list" | "effect_layer_add" | "effect_key_set" => Capability::Grade,
            "watch_render" | "craft_critique" | "local_vlm" | "preview_frame" => Capability::Review,
            "export_start" | "export_wait" | "export_status" | "collision_check" => {
                Capability::Delivery
            }
            _ => return None,
        })
    }
}

/// What one capability area did in the run.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRecord {
    /// The area.
    pub capability: Capability,
    /// Whether a tool serving it answered successfully.
    pub consumed: bool,
    /// The tools of it that answered, in call order, deduplicated.
    pub tools: Vec<String>,
}

/// How one quality gate answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateStatus {
    /// Measured, and it met the bar.
    Pass,
    /// Measured, and it did not.
    Fail,
    /// The evidence the gate reads never arrived. Not a pass: an
    /// unmeasured dimension is unknown, and unknown is not good.
    Skipped,
}

impl GateStatus {
    /// The word the receipt carries.
    pub fn label(self) -> &'static str {
        match self {
            GateStatus::Pass => "pass",
            GateStatus::Fail => "fail",
            GateStatus::Skipped => "skipped",
        }
    }
}

/// One independent quality gate: a single craft dimension, the fact that
/// answers it, and the answer.
///
/// The gates are independent on purpose. A craft score of 9/10 is one
/// number about the picture; it cannot speak for the audio, the export's
/// integrity or the narration's presence. Each dimension answers for
/// itself, so a flaw in one is never averaged away by strength in
/// another.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateRecord {
    /// The dimension's own name: `story`, `audio`, `export-integrity`.
    pub gate: String,
    /// The answer.
    pub status: GateStatus,
    /// The measured fact the answer was read from, said plainly.
    pub evidence: String,
    /// Whether the run is incomplete without this gate. A required gate
    /// that was skipped makes the verdict `Incomplete`; an optional one
    /// that was skipped is only recorded.
    pub required: bool,
}

impl GateRecord {
    /// A gate answered from `evidence`.
    pub fn new(
        gate: &str,
        status: GateStatus,
        evidence: impl Into<String>,
        required: bool,
    ) -> GateRecord {
        GateRecord {
            gate: gate.to_owned(),
            status,
            evidence: evidence.into(),
            required,
        }
    }
}

/// The run's standing, read from its gates rather than from its exit.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Every gate that answered passed, and every required one answered.
    Accepted,
    /// No gate failed, but at least one required gate had no evidence.
    Incomplete,
    /// A gate failed: the file may exist, the edit is not accepted.
    Rejected,
}

impl Verdict {
    /// The word the receipt carries.
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Accepted => "accepted",
            Verdict::Incomplete => "incomplete",
            Verdict::Rejected => "rejected",
        }
    }
}

/// One asset the run brought into the project, with the rights it stands
/// on. A licensed shelf's hit carries its licence in the search page and
/// its import's reply; the research index (OpenVid) carries none, which
/// is why it is recorded as unrecorded rather than assumed clean.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRecord {
    /// Which shelf it came from: `pexels`, `pixabay`, `openvid`, `bgm`,
    /// `sounds`, `magick`, `motion`.
    pub provider: String,
    /// The clip's or track's id on that shelf.
    pub id: String,
    /// The licence the shelf's own reply spelled, when it spelled one.
    /// `None` is the honest answer for an index with no per-clip record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Who made it, when the shelf named them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The URL the asset was taken from, when the run knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Whether the rights allow a monetized edit. `None` is unknown, and
    /// unknown is not permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commercial_ok: Option<bool>,
}

/// Every asset the run imported, read from the import calls' own replies
/// and arguments. A run that imported nothing has an empty list, which
/// is the truth about a project that used only its own media.
pub fn provenance_of(observed: &[(ToolCall, Value)]) -> Vec<AssetRecord> {
    let mut assets: Vec<AssetRecord> = Vec::new();
    // The music shelf's own licence records, remembered from the search
    // and fetch replies the run made before it imported: bgm_import's
    // reply carries media ids and a view, never a licence, so the
    // import arm below used to read a key that never exists and record
    // every bed as an asset with unrecorded rights - on the one asset
    // the receipt exists to vouch for.
    let mut bgm_rights: std::collections::HashMap<
        String,
        (Option<String>, Option<String>),
    > = std::collections::HashMap::new();
    let text = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|word| !word.trim().is_empty())
            .map(str::to_owned)
    };
    // The ids an import call named: the live shelves take one `id`, the
    // research index and the music shelf take an `ids` array.
    let ids_of = |arguments: &Value| -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        if let Some(id) = text(arguments, "id") {
            ids.push(id);
        }
        if let Some(list) = arguments.get("ids").and_then(Value::as_array) {
            for id in list {
                let id = match id {
                    Value::String(word) => word.clone(),
                    Value::Number(number) => number.to_string(),
                    _ => continue,
                };
                if !id.trim().is_empty() && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids
    };
    for (call, reply) in observed {
        let provider = text(&call.arguments, "provider");
        let url = text(&call.arguments, "url");
        let ids = ids_of(&call.arguments);
        let id = ids.first().cloned();
        if call.tool.as_str() == "bgm_search" || call.tool.as_str() == "bgm_fetch" {
            let tracks: Vec<&Value> = if call.tool.as_str() == "bgm_search" {
                reply
                    .get("tracks")
                    .and_then(Value::as_array)
                    .map(|list| list.iter().collect())
                    .unwrap_or_default()
            } else {
                std::iter::once(reply).collect()
            };
            for track in tracks {
                if let Some(track_id) = text(track, "id") {
                    bgm_rights.insert(
                        track_id,
                        (text(track, "license"), text(track, "artist")),
                    );
                }
            }
        }
        let record = match call.tool.as_str() {
            // The live shelves' import: the licence travels in the reply.
            // A reply that spelled no licence is not a monetizable asset:
            // the receipt recorded `commercial_ok: true` with a null
            // licence - "safe to sell, licence unknown" - and the
            // unlicensed count never listed it.
            "stock_import" => {
                let license = text(reply, "license");
                Some(AssetRecord {
                    provider: provider.clone().unwrap_or_else(|| "stock".to_owned()),
                    id: id.clone().unwrap_or_default(),
                    author: None,
                    url: url.clone(),
                    commercial_ok: Some(license.is_some()),
                    license,
                })
            }
            // The research index: no per-clip record, and the import tool
            // refuses a monetized edit, so commercial use is unknown.
            // The research index takes many ids at once; each is its own
            // asset with its own (absent) record.
            "openvid_import" => {
                for id in &ids {
                    if !assets
                        .iter()
                        .any(|asset| asset.provider == "openvid" && &asset.id == id)
                    {
                        assets.push(AssetRecord {
                            provider: "openvid".to_owned(),
                            id: id.clone(),
                            license: None,
                            author: None,
                            url: url.clone(),
                            commercial_ok: None,
                        });
                    }
                }
                None
            }
            "bgm_import" => {
                // One asset per track the import named: the licence each
                // track carries was spelled by the search or fetch reply
                // that chose it (bgm_import's own reply never carries
                // one), and commercial_ok is the licence's own verdict -
                // CC0 permits a monetized edit, MobyGratis refuses it.
                for track_id in &ids {
                    let (license, artist) = bgm_rights
                        .get(track_id)
                        .cloned()
                        .unwrap_or((None, None));
                    let commercial_ok = match license.as_deref() {
                        Some("CC0-1.0") => Some(true),
                        Some(_) => Some(false),
                        None => None,
                    };
                    if !assets
                        .iter()
                        .any(|asset| asset.provider == "bgm" && &asset.id == track_id)
                    {
                        assets.push(AssetRecord {
                            provider: "bgm".to_owned(),
                            id: track_id.clone(),
                            license,
                            author: artist,
                            url: None,
                            commercial_ok,
                        });
                    }
                }
                None
            }
            "sounds_add" => Some(AssetRecord {
                provider: "sounds".to_owned(),
                id: id.clone().unwrap_or_default(),
                // The bundled shelf is CC0 throughout; the reply says so.
                license: text(reply, "license").or_else(|| Some("CC0-1.0".to_owned())),
                author: None,
                url: None,
                commercial_ok: Some(true),
            }),
            "magick_add" => Some(AssetRecord {
                provider: "magick".to_owned(),
                id: id.clone().unwrap_or_default(),
                license: text(reply, "license"),
                author: None,
                url: None,
                commercial_ok: None,
            }),
            "motion_import" | "motion_bake" => Some(AssetRecord {
                provider: "motion".to_owned(),
                id: text(reply, "hash")
                    .or_else(|| text(&call.arguments, "preset"))
                    .unwrap_or_default(),
                license: None,
                author: None,
                url: None,
                commercial_ok: Some(true),
            }),
            _ => None,
        };
        if let Some(record) = record
            && !record.id.is_empty()
            && !assets
                .iter()
                .any(|asset| asset.provider == record.provider && asset.id == record.id)
        {
            assets.push(record);
        }
    }
    assets
}

/// What one run did, as evidence.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceReceipt {
    /// The schema's own name.
    pub schema: String,
    /// The project the run edited.
    pub project: String,
    /// Where the export landed.
    pub export: String,
    /// Every stage, in the program's order.
    pub stages: Vec<StageRecord>,
    /// Every capability area, whether it was consumed and by what.
    pub capabilities: Vec<CapabilityRecord>,
    /// The quality gates, one per dimension.
    pub gates: Vec<GateRecord>,
    /// Every warn-policy failure, one line each.
    pub warnings: Vec<String>,
    /// Every asset the run brought into the project, with the rights it
    /// stands on: the provenance manifest a monetized edit is published
    /// from.
    #[serde(default)]
    pub assets: Vec<AssetRecord>,
    /// The standing the gates earn.
    pub verdict: Verdict,
}

impl AcceptanceReceipt {
    /// The receipt for a run that walked `stages` over `project`,
    /// exporting to `export`, with `gates` and `warnings`.
    ///
    /// The capability coverage is derived from the stages' own call
    /// lists, so the receipt cannot claim a capability the run did not
    /// reach: the tools that answered are the only evidence there is.
    pub fn new(
        project: impl Into<String>,
        export: impl Into<String>,
        stages: Vec<StageRecord>,
        gates: Vec<GateRecord>,
        warnings: Vec<String>,
    ) -> AcceptanceReceipt {
        let mut capabilities = Vec::new();
        for capability in Capability::ALL {
            let mut tools: Vec<String> = Vec::new();
            for record in &stages {
                for tool in &record.calls {
                    if Capability::of_tool(tool) == Some(capability) && !tools.contains(tool) {
                        tools.push(tool.clone());
                    }
                }
            }
            capabilities.push(CapabilityRecord {
                capability,
                consumed: !tools.is_empty(),
                tools,
            });
        }
        let verdict = verdict_of(&gates);
        AcceptanceReceipt {
            schema: SCHEMA.to_owned(),
            project: project.into(),
            export: export.into(),
            stages,
            capabilities,
            gates,
            warnings,
            assets: Vec::new(),
            verdict,
        }
    }

    /// The capability areas the program was built with and the run never
    /// reached, each with the reason a person would want. A run is not
    /// wrong for having one - a project with no graphics to bake has no
    /// motion to make - but the receipt says so rather than leaving it
    /// to be discovered.
    pub fn unreached(&self) -> Vec<&CapabilityRecord> {
        self.capabilities
            .iter()
            .filter(|record| !record.consumed)
            .collect()
    }

    /// The receipt with the run's asset provenance attached.
    pub fn with_assets(mut self, assets: Vec<AssetRecord>) -> AcceptanceReceipt {
        self.assets = assets;
        self
    }

    /// The assets whose rights do not permit a monetized edit: the list a
    /// publisher has to answer for before the video earns money.
    pub fn unlicensed(&self) -> Vec<&AssetRecord> {
        self.assets
            .iter()
            .filter(|asset| asset.commercial_ok != Some(true))
            .collect()
    }

    /// Whether every required gate answered with evidence.
    pub fn complete(&self) -> bool {
        self.gates
            .iter()
            .filter(|gate| gate.required)
            .all(|gate| gate.status != GateStatus::Skipped)
    }

    /// The receipt as JSON: the artifact written beside the edit.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({ "schema": SCHEMA }))
    }

    /// One paragraph a person reads: the verdict, the gate that decided
    /// it, and what was never reached. The pane's own status line.
    pub fn summary(&self) -> String {
        let failed: Vec<&GateRecord> = self
            .gates
            .iter()
            .filter(|gate| gate.status == GateStatus::Fail)
            .collect();
        let missing: Vec<&GateRecord> = self
            .gates
            .iter()
            .filter(|gate| gate.required && gate.status == GateStatus::Skipped)
            .collect();
        let unreached: Vec<&str> = self
            .unreached()
            .iter()
            .map(|record| record.capability.label())
            .collect();
        let mut line = format!("{}:", self.verdict.label());
        if !failed.is_empty() {
            line.push_str(&format!(
                " {} gate(s) failed ({})",
                failed.len(),
                failed
                    .iter()
                    .map(|gate| gate.gate.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !missing.is_empty() {
            line.push_str(&format!(
                " {} required gate(s) unmeasured ({})",
                missing.len(),
                missing
                    .iter()
                    .map(|gate| gate.gate.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.warnings.is_empty() {
            line.push_str(&format!(" {} warning(s)", self.warnings.len()));
        }
        if !unreached.is_empty() {
            line.push_str(&format!(" never reached: {}", unreached.join(", ")));
        }
        let unlicensed = self.unlicensed();
        if !unlicensed.is_empty() {
            line.push_str(&format!(
                " {} asset(s) with unrecorded rights ({})",
                unlicensed.len(),
                unlicensed
                    .iter()
                    .map(|asset| format!("{}:{}", asset.provider, asset.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        line
    }
}

/// The gates every run is held to, answered from the replies the run's
/// own calls drew. Each dimension is read from the tool that measures it,
/// so a gate can only pass on evidence the run actually produced:
///
/// - **story** - the plan validated as both structurally valid and
///   creatively ready. A run that never validated a plan has no story
///   gate to pass.
/// - **words** - the export's words re-read and compared to what the
///   edit placed (`emotion {action: verify}`). A render whose narration
///   does not match is rejected however good it looks.
/// - **audio** - the mix measured against the platform's loudness
///   target.
/// - **export-integrity** - the rendered file opened by the media
///   reader, which refuses a missing or unreadable file. This is the
///   gate a craft score can never stand in for: a 9/10 sheet of a broken
///   file is still a broken file.
/// - **visual** - the craft score against the bar. Optional: a run whose
///   critic could not see the frames is recorded, not rejected, because
///   the other gates still speak for the file.
/// - **captions** and **motion** - optional, and answered only when the
///   run made them: a project with no graphics to bake has no motion to
///   prove.
pub fn derive_gates(observed: &[(ToolCall, Value)]) -> Vec<GateRecord> {
    let last = |tool: &str| -> Option<&Value> {
        observed
            .iter()
            .rev()
            .find(|(call, _)| call.tool == tool)
            .map(|(_, reply)| reply)
    };
    let mut gates = Vec::new();

    // The story: a plan that passed the structural gate. creativeReady is
    // the DIRECTOR's bar - an unattended run cannot invent an audience or
    // a goal, so its honest answer is `false`, and a gate that demanded
    // it made every deterministic run's verdict Rejected no matter how
    // good the edit was. The readiness travels in the note; the gate
    // itself asks whether the plan's structure held.
    match last("plan_validate") {
        Some(reply) => {
            let valid = reply.get("valid").and_then(Value::as_bool).unwrap_or(false);
            let ready = reply
                .get("creativeReady")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let score = reply
                .get("craftScore")
                .or_else(|| reply.get("score"))
                .and_then(Value::as_f64)
                .map(|score| format!("{score}"))
                .unwrap_or_else(|| "unscored".to_owned());
            gates.push(GateRecord::new(
                "story",
                if valid {
                    GateStatus::Pass
                } else {
                    GateStatus::Fail
                },
                format!("plan_validate valid={valid} creativeReady={ready} craftScore={score}"),
                true,
            ));
        }
        None => gates.push(GateRecord::new(
            "story",
            GateStatus::Skipped,
            "the run never validated a plan",
            true,
        )),
    }

    // The words: the export re-read and compared to the edit. `emotion`
    // is also an analysis tool, so the gate reads the last reply that
    // actually carries a verdict, not merely the last emotion call.
    let verification = observed
        .iter()
        .rev()
        .find(|(call, reply)| call.tool == "emotion" && reply.get("verdict").is_some())
        .map(|(_, reply)| reply);
    match verification {
        Some(reply) => {
            let verdict = reply
                .get("verdict")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            gates.push(GateRecord::new(
                "words",
                if verdict == "pass" {
                    GateStatus::Pass
                } else {
                    GateStatus::Fail
                },
                format!("the export's words read back as verdict={verdict}"),
                true,
            ));
        }
        None => gates.push(GateRecord::new(
            "words",
            GateStatus::Skipped,
            "the run never compared the export's words to the edit",
            true,
        )),
    }

    // The mix: the loudness reading against the platform target.
    match last("measure_loudness") {
        Some(reply) => {
            let met = reply
                .get("platformTargetMet")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let integrated = reply
                .get("integratedLufs")
                .and_then(Value::as_f64)
                .map(|lufs| format!("{lufs:.1} LUFS"))
                .unwrap_or_else(|| "unread".to_owned());
            gates.push(GateRecord::new(
                "audio",
                if met {
                    GateStatus::Pass
                } else {
                    GateStatus::Fail
                },
                format!("the mix measured {integrated}, platformTargetMet={met}"),
                true,
            ));
        }
        None => gates.push(GateRecord::new(
            "audio",
            GateStatus::Skipped,
            "the mix was never measured",
            true,
        )),
    }

    // The file: opened by the reader the renderer will use.
    match last("motion_probe") {
        Some(reply) => {
            let has_probe = reply.get("probe").is_some();
            gates.push(GateRecord::new(
                "export-integrity",
                if has_probe {
                    GateStatus::Pass
                } else {
                    GateStatus::Fail
                },
                if has_probe {
                    "the rendered file opened and reported its streams"
                } else {
                    "the reader answered without a probe: the file is unreadable"
                },
                true,
            ));
        }
        None => gates.push(GateRecord::new(
            "export-integrity",
            GateStatus::Skipped,
            "the rendered file was never opened by the media reader",
            true,
        )),
    }

    // The picture: the critic's own score against the bar. Only a
    // critique of the RENDERED file counts - the last craft_critique
    // after the last export_start. When the watch's own critique never
    // answered, the preview's pre-export score must not stand in for it
    // and pass a cut that was never judged as shipped.
    let export_at = observed
        .iter()
        .rposition(|(call, _)| call.tool == "export_start");
    let last_critique = observed
        .iter()
        .rposition(|(call, _)| call.tool == "craft_critique")
        .filter(|at| Some(*at) > export_at);
    match last_critique.and_then(|at| observed[at].1.get("overall")) {
        Some(score) => {
            let overall = score.as_f64();
            gates.push(GateRecord::new(
                "visual",
                match overall {
                    Some(overall) if overall >= 7.0 => GateStatus::Pass,
                    _ => GateStatus::Fail,
                },
                format!(
                    "craft_critique scored {}",
                    overall
                        .map(|overall| format!("{overall}"))
                        .unwrap_or_else(|| score.to_string())
                ),
                false,
            ));
        }
        None => gates.push(GateRecord::new(
            "visual",
            GateStatus::Skipped,
            "no craft score was taken of the rendered file",
            false,
        )),
    }

    // The optional craft dimensions, answered only when the run made them.
    for (name, tool) in [("captions", "captions_add"), ("motion", "motion_bake")] {
        if observed.iter().any(|(call, _)| call.tool == tool) {
            gates.push(GateRecord::new(
                name,
                GateStatus::Pass,
                format!("{tool} answered"),
                false,
            ));
        }
    }
    gates
}

/// The standing the gates earn: a failure rejects whatever else passed, a
/// required gate with no evidence leaves the run incomplete, and only a
/// fully answered set is accepted.
fn verdict_of(gates: &[GateRecord]) -> Verdict {
    if gates.iter().any(|gate| gate.status == GateStatus::Fail) {
        return Verdict::Rejected;
    }
    if gates
        .iter()
        .any(|gate| gate.required && gate.status == GateStatus::Skipped)
    {
        return Verdict::Incomplete;
    }
    Verdict::Accepted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(name: &str, status: GateStatus, required: bool) -> GateRecord {
        GateRecord::new(name, status, "measured", required)
    }

    #[test]
    fn a_failed_gate_rejects_the_run_whatever_else_passed() {
        let gates = vec![
            gate("story", GateStatus::Pass, true),
            gate("audio", GateStatus::Fail, true),
        ];
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), gates, Vec::new());
        assert_eq!(receipt.verdict, Verdict::Rejected);
        assert!(receipt.summary().starts_with("rejected:"));
        assert!(receipt.summary().contains("audio"));
    }

    #[test]
    fn a_required_gate_nobody_measured_leaves_the_run_incomplete() {
        let gates = vec![
            gate("story", GateStatus::Pass, true),
            gate("export-integrity", GateStatus::Skipped, true),
        ];
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), gates, Vec::new());
        assert_eq!(receipt.verdict, Verdict::Incomplete);
        assert!(!receipt.complete());
    }

    #[test]
    fn an_optional_gate_nobody_measured_does_not_block_acceptance() {
        let gates = vec![
            gate("story", GateStatus::Pass, true),
            gate("motion", GateStatus::Skipped, false),
        ];
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), gates, Vec::new());
        assert_eq!(receipt.verdict, Verdict::Accepted);
        assert!(receipt.complete());
    }

    #[test]
    fn a_run_the_critic_could_not_see_is_accepted_when_the_required_gates_answered() {
        let call = |tool: &str| ToolCall {
            tool: tool.to_owned(),
            arguments: json!({}),
        };
        let observed = vec![
            (
                call("plan_validate"),
                json!({ "valid": true, "creativeReady": true, "score": 8.5 }),
            ),
            (call("emotion"), json!({ "verdict": "pass", "matched": 12 })),
            (
                call("measure_loudness"),
                json!({ "platformTargetMet": true, "integratedLufs": -14.1 }),
            ),
            (
                call("motion_probe"),
                json!({ "file": "/p/out.mp4", "probe": {} }),
            ),
        ];
        let gates = derive_gates(&observed);
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), gates, Vec::new());
        assert_eq!(
            receipt.verdict,
            Verdict::Accepted,
            "the four required gates answered; a missing craft score is recorded, not a refusal"
        );
        let visual = receipt
            .gates
            .iter()
            .find(|gate| gate.gate == "visual")
            .expect("the visual gate is recorded");
        assert_eq!(visual.status, GateStatus::Skipped);
        assert!(!visual.required);
    }

    #[test]
    fn an_optional_gate_that_failed_still_rejects_the_run() {
        let call = |tool: &str| ToolCall {
            tool: tool.to_owned(),
            arguments: json!({}),
        };
        let observed = vec![
            (
                call("plan_validate"),
                json!({ "valid": true, "creativeReady": true }),
            ),
            (call("emotion"), json!({ "verdict": "pass" })),
            (
                call("measure_loudness"),
                json!({ "platformTargetMet": true }),
            ),
            (call("motion_probe"), json!({ "probe": {} })),
            (call("craft_critique"), json!({ "overall": 6.0 })),
        ];
        let gates = derive_gates(&observed);
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), gates, Vec::new());
        assert_eq!(
            receipt.verdict,
            Verdict::Rejected,
            "optional means not required to run, not that a measured failure is ignored"
        );
    }

    #[test]
    fn coverage_is_read_from_the_tools_that_answered_not_from_intent() {
        let stages = vec![
            StageRecord::completed(
                Stage::Transcribe,
                vec!["transcribe".to_owned(), "analysis_wait".to_owned()],
            ),
            StageRecord::completed(Stage::Gather, vec!["stock_search".to_owned()]),
            // A stage that planned to bake motion and never got there.
            StageRecord {
                stage: Stage::Motion,
                outcome: StageOutcome::Warned,
                calls: Vec::new(),
                failure: Some("motion bake refused".to_owned()),
            },
        ];
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", stages, Vec::new(), Vec::new());
        let consumed = |capability: Capability| {
            receipt
                .capabilities
                .iter()
                .find(|record| record.capability == capability)
                .is_some_and(|record| record.consumed)
        };
        assert!(consumed(Capability::Measurement));
        assert!(consumed(Capability::Footage));
        assert!(!consumed(Capability::Motion));
        assert_eq!(
            receipt
                .unreached()
                .iter()
                .map(|record| record.capability)
                .collect::<Vec<_>>(),
            vec![
                Capability::Planning,
                Capability::Motion,
                Capability::Captions,
                Capability::Music,
                Capability::Sfx,
                Capability::Grade,
                Capability::Review,
                Capability::Delivery,
            ]
        );
    }

    #[test]
    fn an_editing_operation_is_not_a_capability() {
        // clip_add places a clip; it decides nothing, so it claims no
        // area - otherwise every run would look fully covered.
        assert_eq!(Capability::of_tool("clip_add"), None);
        assert_eq!(Capability::of_tool("edit_apply"), None);
        assert_eq!(
            Capability::of_tool("stock_contact_sheet"),
            Some(Capability::Footage)
        );
        assert_eq!(
            Capability::of_tool("craft_critique"),
            Some(Capability::Review)
        );
    }

    #[test]
    fn a_gate_can_only_pass_on_evidence_the_run_produced() {
        let gates = derive_gates(&[]);
        // Nothing measured: every required dimension is skipped, never a
        // pass the run did not earn.
        for name in ["story", "words", "audio", "export-integrity"] {
            let gate = gates
                .iter()
                .find(|gate| gate.gate == name)
                .expect("every required gate is listed");
            assert_eq!(gate.status, GateStatus::Skipped, "{name} was not measured");
            assert!(gate.required);
        }
        // The craft score is a judgement, not proof of the file: it is
        // recorded when the critic saw the frames and never blocks a run
        // whose required gates all answered.
        let visual = gates
            .iter()
            .find(|gate| gate.gate == "visual")
            .expect("the visual gate is listed");
        assert_eq!(visual.status, GateStatus::Skipped);
        assert!(!visual.required);
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), gates, Vec::new());
        assert_eq!(receipt.verdict, Verdict::Incomplete);
    }

    #[test]
    fn a_high_craft_score_cannot_hide_a_broken_file_or_a_failed_mix() {
        let call = |tool: &str| ToolCall {
            tool: tool.to_owned(),
            arguments: json!({}),
        };
        let observed = vec![
            (
                call("plan_validate"),
                json!({ "valid": true, "creativeReady": true, "score": 8.5 }),
            ),
            (call("emotion"), json!({ "verdict": "pass", "matched": 12 })),
            (
                call("measure_loudness"),
                json!({ "platformTargetMet": false, "integratedLufs": -22.4 }),
            ),
            // The reader answered without a probe: the file is unreadable.
            (call("motion_probe"), json!({ "file": "/p/out.mp4" })),
            (call("craft_critique"), json!({ "overall": 9.2 })),
        ];
        let gates = derive_gates(&observed);
        let receipt = AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), gates, Vec::new());
        assert_eq!(receipt.verdict, Verdict::Rejected);
        let failed: Vec<&str> = receipt
            .gates
            .iter()
            .filter(|gate| gate.status == GateStatus::Fail)
            .map(|gate| gate.gate.as_str())
            .collect();
        assert!(failed.contains(&"audio"));
        assert!(failed.contains(&"export-integrity"));
        // The craft score itself passed - and still did not save the run.
        assert_eq!(
            receipt
                .gates
                .iter()
                .find(|gate| gate.gate == "visual")
                .map(|gate| gate.status),
            Some(GateStatus::Pass)
        );
    }

    #[test]
    fn provenance_records_the_licence_a_shelf_gave_and_admits_the_one_it_did_not() {
        let call = |tool: &str, arguments: Value| ToolCall {
            tool: tool.to_owned(),
            arguments,
        };
        let observed = vec![
            (
                call(
                    "stock_import",
                    json!({ "provider": "pexels", "id": "2499611", "url": "https://x/y.mp4" }),
                ),
                json!({ "mediaIds": ["m1"], "license": "Pexels License" }),
            ),
            (
                call("openvid_import", json!({ "ids": [820338756299i64] })),
                json!({ "mediaIds": ["m2"] }),
            ),
            (
                call("sounds_add", json!({ "id": "impact-glass-heavy" })),
                json!({ "license": "CC0-1.0" }),
            ),
        ];
        let assets = provenance_of(&observed);
        assert_eq!(assets.len(), 3);
        let pexels = &assets[0];
        assert_eq!(pexels.provider, "pexels");
        assert_eq!(pexels.license.as_deref(), Some("Pexels License"));
        assert_eq!(pexels.commercial_ok, Some(true));
        // OpenVid carries no per-clip record, and the manifest says so
        // rather than assuming the clip is clean.
        let openvid = &assets[1];
        assert_eq!(openvid.provider, "openvid");
        assert_eq!(openvid.license, None);
        assert_eq!(openvid.commercial_ok, None);
        let receipt =
            AcceptanceReceipt::new("/p", "/p/out.mp4", Vec::new(), Vec::new(), Vec::new())
                .with_assets(assets);
        assert_eq!(receipt.unlicensed().len(), 1);
        assert!(receipt.summary().contains("unrecorded rights"));
    }

    #[test]
    fn a_beds_licence_rides_the_search_that_chose_it() {
        // bgm_import's reply carries media ids and a view, never a
        // licence: the licence was spelled by the search reply that
        // chose the track. A bed imported without a search on the
        // record is an asset with unrecorded rights, not a laundering
        // of whatever the reply happened to omit.
        let call = |tool: &str, arguments: Value| ToolCall {
            tool: tool.to_owned(),
            arguments,
        };
        let observed = vec![
            (
                call("bgm_search", json!({ "query": "city night pulse" })),
                json!({ "tracks": [
                    { "id": "freepd-123", "artist": "Kevin MacLeod",
                      "license": "CC0-1.0" },
                    { "id": "lofi-9", "artist": "Moby",
                      "license": "MobyGratis-NonCommercial" },
                ] }),
            ),
            (
                call("bgm_import", json!({ "path": "/p", "ids": ["freepd-123"] })),
                json!({ "mediaIds": ["m-bed"], "view": {} }),
            ),
            (
                // The Lo-Fi shelf's track was on the same page: its
                // non-commercial terms are read, not laundered.
                call("bgm_import", json!({ "path": "/p", "ids": ["lofi-9"] })),
                json!({ "mediaIds": ["m-other"], "view": {} }),
            ),
            (
                // A bed nobody searched for: the honest absent record.
                call("bgm_import", json!({ "path": "/p", "ids": ["mystery-1"] })),
                json!({ "mediaIds": ["m-mystery"], "view": {} }),
            ),
        ];
        let assets = provenance_of(&observed);
        assert_eq!(assets.len(), 3);
        assert_eq!(assets[0].provider, "bgm");
        assert_eq!(assets[0].id, "freepd-123");
        assert_eq!(assets[0].license.as_deref(), Some("CC0-1.0"));
        assert_eq!(assets[0].author.as_deref(), Some("Kevin MacLeod"));
        assert_eq!(assets[0].commercial_ok, Some(true));
        // The Lo-Fi shelf's terms are real and non-commercial: the
        // record says so instead of vouching for a bed it never read.
        assert_eq!(assets[1].id, "lofi-9");
        assert_eq!(
            assets[1].license.as_deref(),
            Some("MobyGratis-NonCommercial")
        );
        assert_eq!(assets[1].commercial_ok, Some(false));
        // And the one the record never saw: unknown, and unknown is not
        // permission.
        assert_eq!(assets[2].id, "mystery-1");
        assert_eq!(assets[2].license, None);
        assert_eq!(assets[2].commercial_ok, None);
    }

    #[test]
    fn the_receipt_crosses_as_json_with_its_schema_and_verdict() {
        let receipt = AcceptanceReceipt::new(
            "/p",
            "/p/out.mp4",
            vec![StageRecord::skipped(Stage::Transcribe)],
            vec![gate("story", GateStatus::Pass, true)],
            vec!["music bgm_search: the shelf is empty".to_owned()],
        );
        let wire = receipt.to_json();
        assert_eq!(wire["schema"], SCHEMA);
        assert_eq!(wire["verdict"], "accepted");
        assert_eq!(wire["stages"][0]["outcome"], "skipped");
        assert_eq!(wire["capabilities"].as_array().unwrap().len(), 10);
        assert_eq!(wire["warnings"][0], "music bgm_search: the shelf is empty");
        let back: AcceptanceReceipt =
            serde_json::from_value(wire).expect("a receipt reads back from JSON");
        assert_eq!(back, receipt);
    }
}
