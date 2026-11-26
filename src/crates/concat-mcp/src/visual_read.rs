// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The picture's own reading, parsed from `sense_visual`'s replies.
//!
//! `voice_read` turns the voiceover profile into the decisions a cut can
//! spend; this is its counterpart for the frame. One `measure_clips`
//! reply carries a compact read per candidate - motion and its peak, the
//! camera's own travel, the shot's scale, the technical verdict, the
//! salient subject and the combined score - and the program spends each
//! of those on a named choice rather than letting the reply sit in the
//! log as evidence nobody used.
//!
//! The consumers are listed in [`CONSUMERS`] and held to the fields the
//! parser declares, the same discipline `voice_read`'s guard applies: a
//! reading that no decision spends is a reading the edit paid for and
//! threw away.

use serde_json::Value;

/// One candidate's visual read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisualRead {
    /// The file the row was measured from; the join back to the shelf
    /// candidate the sheet ranked.
    pub path: String,
    /// The strongest frame-to-frame movement, in source seconds. The
    /// shot's entrance is placed just before it.
    pub motion_peak: Option<f64>,
    /// The mean movement over the clip, 0..1 of the work width. The
    /// drift's own reading: a shot already moving needs no push added.
    pub motion_mean: Option<f64>,
    /// True when no sampled pair moved: the one case Ken Burns is for.
    pub still: bool,
    /// The camera's own travel: `pan`, `tilt`, `zoom_in`, `zoom_out`,
    /// `diagonal` or `static`.
    pub camera: String,
    /// Which way the camera travels: `left`, `right`, `up`, `down`, `in`,
    /// `out` or `still`. The kind names the axis; this names the way along
    /// it, which the gate's reversal rule needs.
    pub travel: String,
    /// True when the camera itself moves, as opposed to the subject.
    pub camera_moving: bool,
    /// True when the subject moves inside a held frame.
    pub subject_moving: bool,
    /// `wide`, `medium`, `close_up` or `unknown`.
    pub scale: String,
    /// `pass`, `soft`, `noisy`, `letterbox`, `flat`, `dark`, `blown` or
    /// `unknown`.
    pub verdict: String,
    /// Where the eye lands: `left`, `right`, `upper`, `lower`, `center`
    /// or empty when the read found no blob.
    pub saliency_focus: String,
    /// Shot changes detected inside the clip. Two or more is a montage,
    /// not a take, and a montage cannot fill a narration gap whole.
    pub scene_cuts: usize,
    /// The sheet's combined score: sharpness, noise, bars and motion.
    pub visual_score: Option<f64>,
    /// Whether the clip's frames smear: a pan's motion blur is a movement,
    /// not an entrance, and the sheet's own note says so.
    pub blurred: bool,
    /// The clip's measured colour temperature - warm, cool or neutral -
    /// from the same reply's palette. It is the one reading that says
    /// whether a stock clip will sit inside the grade or fight it.
    pub temperature: Option<String>,
}

impl VisualRead {
    /// Parses one `measure_clips` row. A row that failed to measure is
    /// `None`: an unread candidate is not a candidate judged bad.
    pub fn parse(row: &Value) -> Option<Self> {
        if row.get("ok").and_then(Value::as_bool) == Some(false) {
            return None;
        }
        let path = row.get("path").and_then(Value::as_str)?.to_owned();
        let number = |at: &Value| at.as_f64();
        let motion = row.get("motion");
        let camera = row.get("camera");
        let quality = row.get("quality");
        let scale = row.get("shotScale");
        let saliency = row.get("saliency");
        Some(Self {
            path,
            motion_peak: motion.and_then(|row| number(&row["peakAt"])),
            motion_mean: motion.and_then(|row| number(&row["mean"])),
            still: motion
                .and_then(|row| row.get("still"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            camera: camera
                .and_then(|row| row["kind"].as_str())
                .unwrap_or_default()
                .to_owned(),
            travel: camera
                .and_then(|row| row["travel"].as_str())
                .unwrap_or_default()
                .to_owned(),
            camera_moving: camera
                .and_then(|row| row.get("cameraMoving"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            subject_moving: camera
                .and_then(|row| row.get("subjectMoving"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            scale: scale
                .and_then(|row| row["scale"].as_str())
                .unwrap_or_default()
                .to_owned(),
            verdict: quality
                .and_then(|row| row["verdict"].as_str())
                .unwrap_or_default()
                .to_owned(),
            saliency_focus: saliency
                .and_then(|row| row["focus"].as_str())
                .unwrap_or_default()
                .to_owned(),
            scene_cuts: row
                .get("scene")
                .and_then(|scene| scene["cuts"].as_u64())
                .unwrap_or(0) as usize,
            visual_score: number(&row["visualScore"]),
            blurred: row
                .get("blur")
                .and_then(|blur| blur.get("blurredShare"))
                .and_then(Value::as_f64)
                .is_some_and(|share| share >= 0.5),
            temperature: row
                .get("palette")
                .and_then(|palette| palette.get("temperature"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    /// Whether the picture passed the technical read. `unknown` passes:
    /// a candidate the reader could not judge is not one it refused.
    pub fn passes(&self) -> bool {
        matches!(self.verdict.as_str(), "pass" | "unknown" | "")
    }

    /// Whether the picture is one a slot can take: it passed the
    /// technical read and its frames are not smeared. `passes` alone is
    /// the quality verdict; a pan's blur passes quality and still makes a
    /// bad entrance.
    pub fn usable(&self) -> bool {
        self.passes() && !self.blurred
    }

    /// Whether anything in the frame already moves - the camera, the
    /// subject, or both. The drift stands down here: adding a push to a
    /// shot that already travels is the redundant Ken Burns the visual
    /// read exists to prevent.
    pub fn moves(&self) -> bool {
        self.camera_moving
            || self.subject_moving
            || !self.still
            || self.motion_mean.is_some_and(|mean| mean >= 0.01)
    }

    /// The in-point that lands the entrance on the movement: a quarter
    /// second before the peak, clamped inside the media the clip can
    /// still play. `limit` is the source second the clip's last frame may
    /// start from (`media length - placed length`).
    pub fn in_point(&self, limit: f64) -> f64 {
        let Some(peak) = self.motion_peak else {
            return 0.0;
        };
        (peak - 0.25).clamp(0.0, limit.max(0.0))
    }

    /// The clip's travel as the continuity gate names it. The measured
    /// `travel` word decides; the kind is the fallback for a reply that
    /// predates it, because a pan with no sign is still a moving frame.
    pub fn direction(&self) -> concat_motion::continuity::TravelDirection {
        use concat_motion::continuity::TravelDirection as Travel;
        if !self.camera_moving {
            return Travel::Static;
        }
        match self.travel.as_str() {
            "left" => Travel::Left,
            "right" => Travel::Right,
            "up" => Travel::Up,
            "down" => Travel::Down,
            "in" => Travel::ZoomIn,
            "out" => Travel::ZoomOut,
            "still" => Travel::Static,
            _ => match self.camera.as_str() {
                "pan" | "diagonal" => Travel::Right,
                "tilt" => Travel::Down,
                "zoom_in" => Travel::ZoomIn,
                "zoom_out" => Travel::ZoomOut,
                _ => Travel::Static,
            },
        }
    }

    /// The shot's scale as the continuity gate names it, or `None` when
    /// the read could not place it.
    pub fn shot_scale(&self) -> Option<concat_motion::continuity::ShotScale> {
        use concat_motion::continuity::ShotScale as Scale;
        match self.scale.as_str() {
            "wide" => Some(Scale::Wide),
            "medium" => Some(Scale::Medium),
            "close_up" => Some(Scale::CloseUp),
            _ => None,
        }
    }

    /// Where the eye lands, when the read found a subject.
    pub fn saliency_focus(&self) -> Option<&str> {
        (!self.saliency_focus.is_empty()).then_some(self.saliency_focus.as_str())
    }

    /// The sheet's combined score, when the read produced one.
    pub fn score(&self) -> Option<f64> {
        self.visual_score
    }

    /// True when the clip cuts inside itself: a montage, not one take.
    pub fn montage(&self) -> bool {
        self.scene_cuts >= 2
    }
}

/// Every `measure_clips` reply in the log, one read per path, the last
/// measurement winning. A later read is a re-measurement, not a second
/// opinion: the same file measured twice is the same file.
pub fn visual_reads(log: &[Value]) -> Vec<VisualRead> {
    let mut reads: Vec<VisualRead> = Vec::new();
    for reply in log {
        let Some(rows) = reply.get("clips").and_then(Value::as_array) else {
            continue;
        };
        for row in rows {
            let Some(read) = VisualRead::parse(row) else {
                continue;
            };
            match reads.iter_mut().find(|known| known.path == read.path) {
                Some(known) => *known = read,
                None => reads.push(read),
            }
        }
    }
    reads
}

/// One path's read, when the log measured it.
pub fn visual_read_of<'a>(reads: &'a [VisualRead], path: &str) -> Option<&'a VisualRead> {
    reads.iter().find(|read| read.path == path)
}

/// What each reading is spent on, in the words the guard checks. A field
/// with no entry here fails `every_visual_reading_has_a_consumer`.
pub const CONSUMERS: &[(&str, &str)] = &[
    (
        "path",
        "the join back to the shelf candidate the sheet ranked",
    ),
    (
        "motion_peak",
        "the in-point: the shot's entrance lands on the movement",
    ),
    (
        "motion_mean",
        "the drift's veto: a shot already moving gets no push added",
    ),
    (
        "still",
        "the drift's veto and the continuity gate's stillness floor",
    ),
    (
        "camera",
        "the continuity gate's travel direction: a pan never reverses",
    ),
    (
        "travel",
        "the continuity gate's direction: a pan left is not a pan right, so the reversal rule can fire",
    ),
    (
        "camera_moving",
        "the drift's veto: the camera already carries the frame",
    ),
    (
        "subject_moving",
        "the drift's veto: the subject already carries the frame",
    ),
    (
        "scale",
        "the cut rhythm: a close-up holds, a wide shot pushes, and two of one scale in a row is flagged",
    ),
    (
        "verdict",
        "the stock veto: a soft, noisy, letterboxed, flat, dark or blown candidate sinks",
    ),
    (
        "saliency_focus",
        "the caption band: words move away from where the eye already is",
    ),
    (
        "scene_cuts",
        "the stock veto: a clip that cuts inside itself is a montage, not a take",
    ),
    (
        "visual_score",
        "the stock ranking: the sheet's combined score breaks the choice",
    ),
    (
        "blurred",
        "the gather's veto: a pan's smear passes quality and is still no entrance",
    ),
    (
        "temperature",
        "the gather's neighbour tie-break: a clip that fights the last one's warmth reads as another film",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row() -> Value {
        json!({
            "path": "/cache/a.mp4",
            "ok": true,
            "motion": { "mean": 0.021, "peak": 0.08, "peakAt": 1.6, "still": false },
            "camera": { "kind": "pan", "cameraMoving": true, "subjectMoving": true, "travel": "left" },
            "shotScale": { "scale": "wide" },
            "quality": { "verdict": "pass" },
            "saliency": { "focus": "lower", "peak": { "x": 0.5, "y": 0.8 } },
            "scene": { "cuts": 0, "dissolves": 0, "firstCut": null },
            "visualScore": 71.5,
        })
    }

    #[test]
    fn a_smeared_pan_is_not_an_entrance_and_the_palette_names_its_warmth() {
        let mut smeared = row();
        smeared["blur"] = json!({ "verdict": "directional blur", "blurredShare": 0.8 });
        smeared["palette"] = json!({ "dominant": [], "temperature": "cool", "saturation": 0.3 });
        let read = VisualRead::parse(&smeared).expect("a measured row");
        // Quality said pass; the frames still smear, and a smear is a
        // movement, not an entrance.
        assert!(read.passes());
        assert!(read.blurred);
        assert!(!read.usable());
        assert_eq!(read.temperature.as_deref(), Some("cool"));
        // A clean row carries no blur and no palette, and stays usable.
        let clean = VisualRead::parse(&row()).expect("a measured row");
        assert!(!clean.blurred && clean.usable());
        assert_eq!(clean.temperature, None);
    }

    #[test]
    fn a_row_is_parsed_into_the_readings_the_edit_spends() {
        let read = VisualRead::parse(&row()).expect("a measured row");
        assert_eq!(read.path, "/cache/a.mp4");
        assert_eq!(read.motion_peak, Some(1.6));
        assert_eq!(read.motion_mean, Some(0.021));
        assert!(!read.still);
        assert_eq!(read.camera, "pan");
        assert_eq!(read.travel, "left");
        assert!(read.camera_moving && read.subject_moving);
        assert_eq!(read.scale, "wide");
        assert_eq!(read.verdict, "pass");
        assert_eq!(read.saliency_focus(), Some("lower"));
        assert_eq!(read.score(), Some(71.5));
        assert!(!read.montage());
        assert!(read.passes() && read.moves());
        assert_eq!(
            read.shot_scale(),
            Some(concat_motion::continuity::ShotScale::Wide)
        );
    }

    #[test]
    fn a_failed_row_is_not_a_candidate_judged_bad() {
        assert!(VisualRead::parse(&json!({ "path": "/x.mp4", "ok": false })).is_none());
    }

    #[test]
    fn a_clip_that_cuts_inside_itself_is_a_montage() {
        let mut row = row();
        row["scene"] = json!({ "cuts": 3, "dissolves": 1, "firstCut": 0.9 });
        let read = VisualRead::parse(&row).expect("a measured row");
        assert_eq!(read.scene_cuts, 3);
        assert!(read.montage());
    }

    #[test]
    fn the_in_point_lands_before_the_peak_and_inside_the_media() {
        let read = VisualRead::parse(&row()).expect("a measured row");
        assert!((read.in_point(10.0) - 1.35).abs() < 1e-9);
        assert_eq!(read.in_point(1.0), 1.0, "clamped to the media's limit");
        assert_eq!(read.in_point(0.0), 0.0);
        let still = VisualRead::default();
        assert_eq!(still.in_point(10.0), 0.0, "no peak, no move");
    }

    #[test]
    fn a_moving_shot_needs_no_added_push_and_a_still_one_does() {
        let moving = VisualRead::parse(&row()).expect("a measured row");
        assert!(moving.moves());
        let still = VisualRead::parse(&json!({
            "path": "/cache/b.mp4", "ok": true,
            "motion": { "mean": 0.0, "still": true },
            "camera": { "kind": "static", "cameraMoving": false, "subjectMoving": false },
        }))
        .expect("a measured row");
        assert!(!still.moves());
    }

    #[test]
    fn a_pan_left_is_not_a_pan_right() {
        use concat_motion::continuity::TravelDirection as Travel;
        let left = VisualRead::parse(&row()).expect("a measured row");
        assert_eq!(left.direction(), Travel::Left);
        let mut right = row();
        right["camera"]["travel"] = json!("right");
        assert_eq!(
            VisualRead::parse(&right)
                .expect("a measured row")
                .direction(),
            Travel::Right
        );
        let mut tilt = row();
        tilt["camera"] = json!({ "kind": "tilt", "cameraMoving": true,
                                 "subjectMoving": false, "travel": "up" });
        assert_eq!(
            VisualRead::parse(&tilt)
                .expect("a measured row")
                .direction(),
            Travel::Up
        );
        // A reply from before the travel word: the kind still says the
        // frame moves, so the gate is not handed a still shot.
        let mut old = row();
        old["camera"] = json!({ "kind": "pan", "cameraMoving": true, "subjectMoving": false });
        assert_eq!(
            VisualRead::parse(&old).expect("a measured row").direction(),
            Travel::Right
        );
        let still = VisualRead::parse(&json!({
            "path": "/cache/b.mp4", "ok": true,
            "camera": { "kind": "static", "cameraMoving": false, "travel": "still" },
        }))
        .expect("a measured row");
        assert_eq!(still.direction(), Travel::Static);
    }

    #[test]
    fn the_last_measurement_of_a_path_wins() {
        let first = json!({ "clips": [row()] });
        let mut later = row();
        later["quality"]["verdict"] = json!("soft");
        let log = vec![first, json!({ "clips": [later] })];
        let reads = visual_reads(&log);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].verdict, "soft");
        assert!(!reads[0].passes());
        assert_eq!(
            visual_read_of(&reads, "/cache/a.mp4").map(|r| r.verdict.as_str()),
            Some("soft")
        );
        assert!(visual_read_of(&reads, "/cache/missing.mp4").is_none());
    }

    #[test]
    fn every_visual_reading_has_a_consumer() {
        let source = include_str!("visual_read.rs");
        let mut fields: Vec<&str> = Vec::new();
        for line in source.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("pub ") else {
                continue;
            };
            let Some((name, _)) = rest.split_once(':') else {
                continue;
            };
            if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
                fields.push(name);
            }
        }
        assert!(
            fields.len() > 8,
            "the field scan found {} fields, too few to be the reader",
            fields.len()
        );
        let unaccounted: Vec<&str> = fields
            .iter()
            .copied()
            .filter(|field| !CONSUMERS.iter().any(|(name, _)| name == field))
            .collect();
        assert!(
            unaccounted.is_empty(),
            "these readings are parsed and spent by no decision: {unaccounted:?} - \
             wire each to a choice, or add it to CONSUMERS with its reason"
        );
        for (name, _) in CONSUMERS {
            assert!(
                fields.contains(name),
                "CONSUMERS names {name}, which the reader does not declare"
            );
        }
    }
}
