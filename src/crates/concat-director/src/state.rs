// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The run's state, the progress it reports, and the event that carries
//! both to whoever is watching.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AutoEditError;
use crate::stage::Stage;

/// Where an auto-edit stands. One runner holds one of these at a time,
/// and every change of standing is an [`AutoEditEvent`] the same moment
/// it happens - the state a progress reader asks for and the events a
/// watcher drains never disagree.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutoEditState {
    /// A runner that has not run. A fresh runner answers this before its
    /// program starts.
    Idle,
    /// The run is in a stage. `started` is when the run began, in the
    /// record's own clock (UTC, RFC 3339).
    Running {
        /// The stage the run is in.
        stage: Stage,
        /// When the run began, UTC, RFC 3339.
        started: String,
    },
    /// A stage failed and the run stopped. The error says whether the
    /// same call could work again, and `attempt` is which attempt of it
    /// failed - one, or three after two retries.
    Failed {
        /// The stage that was running when the failure landed.
        stage: Stage,
        /// The failure, with the call that was refused.
        error: AutoEditError,
        /// Which attempt of that call failed, counted from one.
        attempt: u32,
    },
    /// The export is rendered, verified and shipped.
    Completed {
        /// Where the export landed - the path the plan promised.
        export: PathBuf,
        /// How long the whole run took.
        #[serde(with = "seconds")]
        duration: Duration,
        /// The last successful answer of the Verify stage: the evidence
        /// an acceptance read wants.
        verification: Value,
    },
    /// The export is shipped with flaws written down: the warn-policy
    /// stages that failed, recorded per slot the way the revise loop
    /// records them.
    CompletedWithWarnings {
        /// Where the export landed - the path the plan promised.
        export: PathBuf,
        /// How long the whole run took.
        #[serde(with = "seconds")]
        duration: Duration,
        /// Every warn-policy failure, one line each: the stage, the
        /// tool, the sentence.
        warnings: Vec<String>,
        /// The last successful answer of the Verify stage.
        verification: Value,
    },
    /// The run was asked to stop, and stopped at a stage boundary.
    /// `rolled_back` says whether every edit the run had applied was
    /// undone. `undone_to` is the earliest stage whose edits were undone
    /// (the timeline stands as it did before that stage applied its
    /// first edit), or `None` when there was nothing to undo.
    Cancelled {
        /// When the run stopped, UTC, RFC 3339.
        at: String,
        /// Whether the rollback reached all the way back.
        rolled_back: bool,
        /// The earliest stage whose edits were undone, when any were.
        undone_to: Option<Stage>,
    },
}

/// How far along a run is: the stage, the counts, the clock and a line
/// saying what is happening right now.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoEditProgress {
    /// The stage the run is in.
    pub stage: Stage,
    /// How many stages into the program this is, counted from zero, in
    /// the program's own order.
    pub stage_index: usize,
    /// How many stages the program has.
    pub total_stages: usize,
    /// How long since the run began.
    #[serde(with = "seconds")]
    pub elapsed_total: Duration,
    /// How long since the stage began.
    #[serde(with = "seconds")]
    pub elapsed_stage: Duration,
    /// What is happening right now: the tool being called, a stage
    /// beginning or ending, a rollback in progress.
    pub detail: String,
}

impl AutoEditProgress {
    /// A progress reading with every field said out loud.
    pub fn new(
        stage: Stage,
        stage_index: usize,
        total_stages: usize,
        elapsed_total: Duration,
        elapsed_stage: Duration,
        detail: impl Into<String>,
    ) -> AutoEditProgress {
        AutoEditProgress {
            stage,
            stage_index,
            total_stages,
            elapsed_total,
            elapsed_stage,
            detail: detail.into(),
        }
    }
}

/// What the runner emitted: the new standing, and the progress reading
/// that came with it. Every event carries both, so a watcher that only
/// reads events has everything a poller has.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoEditEvent {
    /// The run's standing as of this event.
    pub state: AutoEditState,
    /// The progress reading that came with it.
    pub progress: AutoEditProgress,
}

/// What [`Runner::run`](crate::runner::Runner::run) came back with. The
/// details live in the runner's state and its events - the single source
/// of truth a progress reader or a tool answers from - so the outcome
/// says only which of the four ends the run met.
#[derive(Clone, PartialEq, Debug)]
pub enum RunOutcome {
    /// Every stage ran; the export is verified and shipped.
    Completed,
    /// Every stage ran or warned; the export is shipped with its flaws
    /// written down.
    CompletedWithWarnings,
    /// A stage failed and the run stopped, after its retries.
    Failed(AutoEditError),
    /// The run was asked to stop and stopped at a boundary.
    Cancelled,
}

/// A [`Duration`] on the wire is seconds - one number a progress reader
/// can show, rather than the `{secs, nanos}` pair serde writes for
/// `Duration` by default.
pub(crate) mod seconds {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Seconds, as a number.
    pub(crate) fn serialize<S: Serializer>(
        duration: &Duration,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(duration.as_secs_f64())
    }

    /// A duration, from seconds.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Duration, D::Error> {
        let seconds = f64::deserialize(deserializer)?;
        read(seconds)
    }

    /// The shared half: seconds that are a number and not a negative or
    /// nonsensical one.
    pub(crate) fn read<E: serde::de::Error>(seconds: f64) -> Result<Duration, E> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(E::custom(
                "a duration on the wire is seconds: a non-negative, finite number",
            ));
        }
        Ok(Duration::from_secs_f64(seconds))
    }
}

/// [`seconds`](self), for the durations that may be absent: null when
/// there is no duration to say.
pub(crate) mod optional_seconds {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Seconds, as a number, or null.
    pub(crate) fn serialize<S: Serializer>(
        duration: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match duration {
            Some(duration) => serializer.serialize_some(&duration.as_secs_f64()),
            None => serializer.serialize_none(),
        }
    }

    /// A duration, from seconds or null.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        match Option::<f64>::deserialize(deserializer)? {
            Some(seconds) => super::seconds::read(seconds).map(Some),
            None => Ok(None),
        }
    }
}

/// The moment, as the run's records write it: UTC, RFC 3339, whole
/// seconds - the same shape the watcher's own run records carry
/// (`2026-11-13T16:05:44Z`).
pub(crate) fn timestamp() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    rfc3339(since_epoch.as_secs())
}

/// Seconds since 1970-01-01 as the record's clock writes them.
fn rfc3339(seconds: u64) -> String {
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    let second_of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60
    )
}

/// Days since 1970-01-01 as a (year, month, day), Howard Hinnant's
/// `civil_from_days`: the plain arithmetic every calendar library holds,
/// held here so the crate needs no clock dependency for a timestamp.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_epoch_and_the_leap_days_read_back_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
        // 2100 is not a leap year: the century rule the calendar carries.
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        assert_eq!(civil_from_days(20_771), (2026, 11, 14));
    }

    #[test]
    fn the_clock_writes_whole_seconds_in_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_794_648_000), "2026-11-14T09:20:00Z");
    }

    #[test]
    fn an_event_crosses_as_camel_case_json_with_seconds_as_numbers() {
        let event = AutoEditEvent {
            state: AutoEditState::Running {
                stage: Stage::HandoffVoiceover,
                started: "2026-11-13T16:05:44Z".to_owned(),
            },
            progress: AutoEditProgress::new(
                Stage::HandoffVoiceover,
                0,
                15,
                Duration::from_millis(1_500),
                Duration::ZERO,
                "stage began",
            ),
        };
        let wire = serde_json::to_value(&event).expect("an event is JSON");
        assert_eq!(wire["state"]["state"], "running");
        assert_eq!(wire["state"]["stage"], "handoff-voiceover");
        assert_eq!(wire["state"]["started"], "2026-11-13T16:05:44Z");
        assert_eq!(wire["progress"]["stage"], "handoff-voiceover");
        assert_eq!(wire["progress"]["stageIndex"], 0);
        assert_eq!(wire["progress"]["totalStages"], 15);
        assert_eq!(wire["progress"]["elapsedTotal"], json!(1.5));
        assert_eq!(wire["progress"]["elapsedStage"], json!(0.0));
        assert_eq!(wire["progress"]["detail"], "stage began");
        let back: AutoEditEvent =
            serde_json::from_value(wire).expect("an event reads back from JSON");
        assert_eq!(back, event);
    }

    #[test]
    fn a_terminal_state_crosses_with_its_own_fields() {
        let state = AutoEditState::Cancelled {
            at: "2026-11-13T16:09:02Z".to_owned(),
            rolled_back: true,
            undone_to: Some(Stage::Timeline),
        };
        let wire = serde_json::to_value(&state).expect("a state is JSON");
        assert_eq!(wire["state"], "cancelled");
        assert_eq!(wire["at"], "2026-11-13T16:09:02Z");
        assert_eq!(wire["rolledBack"], true);
        assert_eq!(wire["undoneTo"], "timeline");
        let back: AutoEditState =
            serde_json::from_value(wire).expect("a state reads back from JSON");
        assert_eq!(back, state);
    }

    #[test]
    fn a_negative_duration_is_refused_rather_than_panicked_over() {
        let wire = json!({ "stage": "plan", "stageIndex": 5, "totalStages": 15,
            "elapsedTotal": -1.0, "elapsedStage": 0.0, "detail": "stage began" });
        let outcome: Result<AutoEditProgress, _> = serde_json::from_value(wire);
        assert!(outcome.is_err());
    }
}
