// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! What can go wrong while an auto-edit runs, and whether the same call
//! is worth making again.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::stage::Stage;

/// The tools a run may try again when they fail, because a retry lands
/// where the first attempt stood: reads, searches, analyses and the three
/// repair writes (which overwrite a fixed `out` path). Mutating timeline
/// calls are never here - an ambiguous `clip_add` must stop, not
/// double-place. This is the watcher's own list
/// (`scripts/auto_director.py`), word for word, so a retry means the same
/// thing on both sides of the seam.
const RETRYABLE_TOOLS: [&str; 30] = [
    "transcribe",
    "analysis_wait",
    "project_get",
    "plan_validate",
    "voice_signals",
    "emotion",
    "concat_version",
    "local_vlm",
    "footage_plan_suggest",
    "stock_contact_sheet",
    "detect_music_segments",
    "identify_music",
    "measure_loudness",
    "watch_render",
    "craft_critique",
    "sounds_search",
    "bgm_status",
    "bgm_search",
    "bgm_fetch",
    "openvid_search",
    "openvid_search_batch",
    "openvid_rank_for_slot",
    "openvid_get",
    "openvid_status",
    "timeline_read",
    "collision_check",
    "effects_list",
    "enhance_speech",
    "remove_hiss_hum_rumble",
    "normalize_loudness",
];

/// What a failure sounds like when it is the network's side of the
/// contract, not the tool's. The watcher's own markers, kept word for
/// word.
const RETRYABLE_MARKERS: [&str; 10] = [
    "timeout",
    "timed out",
    "unreachable",
    "connection",
    "reset",
    "temporarily",
    "broken pipe",
    "502",
    "504",
    "try again",
];

/// One tool call, in the shape every tool call already has on the wire:
/// the name `tools/call` carries, and the arguments object the tool's
/// schema takes. The runner's rollback writes these (`edit_undo`), and a
/// failure records the call that was refused, so a person reading the
/// run's events can tell exactly which ask did not land.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// The tool, by its served name: `project_get`, `edit_apply`,
    /// `measure_loudness`.
    pub tool: String,
    /// The call's arguments, exactly as the tool's schema takes them.
    pub arguments: Value,
}

/// A tool call's refusal, as the caller sees it. What the caller knows
/// that the message alone does not: whether the same call could work
/// again, how long to wait before it could, and the JSON-RPC error
/// number the refusal carried, when it carried one.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
#[error("{message}")]
pub struct ToolError {
    /// The sentence a person would be shown.
    pub message: String,
    /// The caller's own word on whether the same call could work again.
    /// `None` - the common case, an adapter with no opinion - leaves the
    /// runner to decide the way the watcher does: the tool is one whose
    /// retries land, and the message sounds like the network's side.
    pub retryable: Option<bool>,
    /// How long the caller says to wait before trying again, when it says.
    pub retry_after: Option<Duration>,
    /// The JSON-RPC error number the refusal carried (-32000 downwards),
    /// when the caller knows it.
    pub code: Option<i64>,
}

/// The failure the state machine records: where the run stood, which ask
/// was refused, what it said, and whether - and when - the same ask could
/// work again. `context` carries anything the stage knew that the
/// message does not say: signal ids, slot ids, the plan's name.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct AutoEditError {
    /// The stage that was running when the failure landed.
    pub stage: Stage,
    /// The call that was refused, when the failure came from one.
    pub tool_call: Option<ToolCall>,
    /// The sentence a person would be shown.
    pub message: String,
    /// Whether the same call could work again - a retryable tool whose
    /// failure sounds like the network's side, or a caller that said so.
    pub retryable: bool,
    /// How long to wait before the same call could work, when it could.
    #[serde(with = "crate::state::optional_seconds", default)]
    pub retry_after: Option<Duration>,
    /// What the stage knew that the message does not say.
    #[serde(default)]
    pub context: Value,
}

/// Whether the same call could work again, the way the watcher decides:
/// the tool is one whose retries land, and the failure sounds like the
/// network's side of the contract. A refusal that says `not found` or
/// `unauthorized` is the plan's to fix, not the network's.
pub fn is_retryable(tool: &str, message: &str) -> bool {
    RETRYABLE_TOOLS.contains(&tool) && {
        let lowered = message.to_lowercase();
        RETRYABLE_MARKERS
            .iter()
            .any(|marker| lowered.contains(marker))
    }
}

impl AutoEditError {
    /// The failure `error` was, at `stage`, answering `call`. The
    /// caller's own retryability word wins when it has one; otherwise
    /// the tool and the message decide, the watcher's way.
    pub fn new(stage: Stage, call: Option<ToolCall>, error: ToolError) -> AutoEditError {
        let retryable = match (&call, error.retryable) {
            (_, Some(word)) => word,
            (Some(call), None) => is_retryable(&call.tool, &error.message),
            (None, None) => false,
        };
        AutoEditError {
            stage,
            tool_call: call,
            message: error.message,
            retryable,
            retry_after: error.retry_after,
            context: error
                .code
                .map(|code| json!({ "code": code }))
                .unwrap_or(Value::Null),
        }
    }

    /// The failure as one line a warning list can carry: the stage, the
    /// tool, the sentence - `watch craft_critique: the contact sheet is
    /// blank` - the shape the revise loop's per-slot failures will read
    /// in.
    pub fn line(&self) -> String {
        match &self.tool_call {
            Some(call) => format!("{} {}: {}", self.stage.label(), call.tool, self.message),
            None => format!("{}: {}", self.stage.label(), self.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_is_a_tool_whose_retries_land_and_a_network_sounding_failure() {
        assert!(is_retryable("transcribe", "the connection was reset"));
        assert!(is_retryable("bgm_search", "OpenVid timed out"));
        // The tool retries safely, but nothing about the message says the
        // network refused: trying the identical call again would fail the
        // identical way.
        assert!(!is_retryable("transcribe", "the file is not audio"));
        // The message sounds transient, but the call mutates: a retry
        // could double-place what the first attempt already placed.
        assert!(!is_retryable("clip_add", "the connection was reset"));
    }

    #[test]
    fn the_callers_own_word_about_retrying_wins() {
        let error = AutoEditError::new(
            Stage::Export,
            Some(ToolCall {
                tool: "export_start".to_owned(),
                arguments: json!({ "path": "/edits/Reel" }),
            }),
            ToolError {
                message: "the export queue is busy".to_owned(),
                retryable: Some(true),
                retry_after: Some(Duration::from_secs(3)),
                code: Some(-32004),
            },
        );
        assert!(error.retryable);
        assert_eq!(error.retry_after, Some(Duration::from_secs(3)));
        // The JSON-RPC number the refusal carried rides in the context,
        // where a reader of the record can find it.
        assert_eq!(error.context, json!({ "code": -32004 }));
    }

    #[test]
    fn a_failure_without_a_call_is_not_retryable() {
        let error = AutoEditError::new(
            Stage::Plan,
            None,
            ToolError {
                message: "the program has no stages to run".to_owned(),
                retryable: Some(false),
                retry_after: None,
                code: None,
            },
        );
        assert!(!error.retryable);
        assert_eq!(
            error.line(),
            "plan: the program has no stages to run".to_owned()
        );
    }

    #[test]
    fn a_warning_line_names_the_stage_the_tool_and_the_sentence() {
        let error = AutoEditError::new(
            Stage::Watch,
            Some(ToolCall {
                tool: "craft_critique".to_owned(),
                arguments: json!({ "overall": 6 }),
            }),
            ToolError {
                message: "the contact sheet is blank".to_owned(),
                retryable: None,
                retry_after: None,
                code: None,
            },
        );
        assert_eq!(
            error.line(),
            "watch craft_critique: the contact sheet is blank".to_owned()
        );
    }

    #[test]
    fn the_record_crosses_as_camel_case_json_with_seconds_as_numbers() {
        let error = AutoEditError::new(
            Stage::Gather,
            Some(ToolCall {
                tool: "openvid_search".to_owned(),
                arguments: json!({ "words": "harbor dawn" }),
            }),
            ToolError {
                message: "the index is temporarily unreachable".to_owned(),
                retryable: None,
                retry_after: None,
                code: None,
            },
        );
        let json = serde_json::to_value(&error).expect("an error is JSON");
        assert_eq!(json["stage"], "gather");
        assert_eq!(json["toolCall"]["tool"], "openvid_search");
        assert_eq!(json["toolCall"]["arguments"]["words"], "harbor dawn");
        assert_eq!(json["retryable"], true);
        assert_eq!(json["retryAfter"], serde_json::Value::Null);
        let back: AutoEditError =
            serde_json::from_value(json).expect("an error reads back from JSON");
        assert_eq!(back, error);
    }
}
