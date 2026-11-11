// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Rendering the session to a file.
//!
//! The rendering is `concat-export`'s; what is here is the seam the window
//! uses: the session flattens itself, the window adds the destination and
//! the quality, and the one export that can run at a time reports through a
//! callback and stops through a flag.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use concat_export::{ExportClip, ExportRequest, Reporter, render_on};
pub use concat_media::{ColorRange, RateMode, VideoCodec};
pub use concat_render::WgpuCompositor;

use crate::jobs::{Job, SingleFlight};
use crate::session::Session;

/// What the window decides about an export: where it goes, how good it is,
/// and any rasterised titles rejoining as image clips.
#[derive(Clone, Debug, Default)]
pub struct ExportSpec {
    /// The file to write.
    pub output: String,
    /// Constant rate factor; lower is better quality and a bigger file.
    pub crf: u8,
    /// The x264 speed/size preset name, e.g. "medium".
    pub preset: String,
    /// What to encode to.
    pub codec: VideoCodec,
    /// Ten bits a channel rather than eight.
    pub ten_bit: bool,
    /// VBR (the CRF carries the quality) or CBR (the bitrate is the
    /// target). VBR by default, so the sheet's Advanced section being off
    /// means the file is what it always was.
    pub rate_mode: concat_media::RateMode,
    /// Target bitrate in kbps, used when `rate_mode` is CBR.
    pub bitrate_kbps: u32,
    /// The levels the file is written in and tagged with. Video range,
    /// 16-235, unless the sheet's Advanced section says full.
    /// https://github.com/jub0t/Concat/issues/103
    pub color_range: ColorRange,
    /// An HDR timeline written as HDR, its own HLG or PQ, rather than
    /// tone-mapped to SDR. Nothing for an SDR timeline.
    pub hdr: bool,
}

/// One progress report: which frame of how many, in which stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Frames done.
    pub frame: i64,
    /// Frames in total.
    pub total: i64,
    /// What the exporter is doing: "rendering", "mixing audio", "muxing".
    pub stage: &'static str,
}

/// The active session timeline flattened into a request the engine can render.
pub fn request(session: &Session, spec: &ExportSpec, titles: Vec<ExportClip>) -> ExportRequest {
    request_for_timeline(session, spec, titles, &session.project().active_timeline_id)
        .expect("the active timeline always belongs to its project")
}

/// Builds an export request for a specific timeline without changing the
/// editor's active tab. A stale or unknown timeline id is an error: silently
/// falling back to another timeline could export the wrong cut.
pub fn request_for_timeline(
    session: &Session,
    spec: &ExportSpec,
    titles: Vec<ExportClip>,
    timeline_id: &str,
) -> Result<ExportRequest, String> {
    let video = session
        .video_for_timeline(timeline_id)
        .ok_or_else(|| format!("timeline {timeline_id:?} does not exist in this project"))?;
    let mut clips = session
        .flattened_clips_for_timeline(timeline_id)
        .ok_or_else(|| format!("timeline {timeline_id:?} does not exist in this project"))?;
    clips.extend(titles);
    Ok(ExportRequest {
        output: spec.output.clone(),
        width: video.width,
        height: video.height,
        rate_num: video.rate_num,
        rate_den: video.rate_den,
        crf: spec.crf,
        preset: spec.preset.clone(),
        codec: spec.codec,
        ten_bit: spec.ten_bit,
        rate_mode: spec.rate_mode,
        bitrate_kbps: spec.bitrate_kbps,
        color_range: spec.color_range,
        color_space: video.color_space,
        hdr: spec.hdr,
        effect_time_offset: 0.0,
        clips,
    })
}

/// Renders `request` and returns the path written, reporting through
/// `progress` and stopping at the next frame once `cancel` is set. Blocks for
/// the whole render: run it on its own thread. Draws on a device of its
/// own; [`run_on`] draws on one the caller lends.
pub fn run(
    request: &ExportRequest,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<String, String> {
    run_on(request, None, cancel, progress)
}

/// [`run`] drawing on `compositor` where the caller has one to lend: the
/// window's, a sibling of its monitor's (see `Monitor::sibling`), so the
/// export draws on the device that has drawn the monitor all along rather
/// than opening a second one through the drivers, which on Windows laptops
/// with NVIDIA chips took the app down at the first frame (issue #202).
/// `None` draws as [`run`] does.
pub fn run_on(
    request: &ExportRequest,
    compositor: Option<WgpuCompositor>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<String, String> {
    let mut report = |frame: i64, total: i64, stage: &'static str| {
        progress(Progress {
            frame,
            total,
            stage,
        });
    };
    render_on(
        request,
        compositor,
        Reporter {
            progress: &mut report,
            cancel,
        },
    )
}

/// The slot for the one export that can run at a time - enforced, not
/// documented: a second export while one runs is refused instead of racing
/// the first for its temp files and cancel flag.
#[derive(Clone, Default)]
pub struct Exporter {
    slot: Arc<SingleFlight>,
}

impl Exporter {
    /// An idle exporter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Claims the slot for a new export, whose cancel flag the render polls.
    pub fn begin(&self) -> Result<Job, String> {
        self.slot.begin("export")
    }

    /// Asks the running export to stop at the next frame. Idle is a no-op.
    pub fn cancel(&self) {
        self.slot.cancel();
    }

    /// Whether an export is running.
    pub fn is_busy(&self) -> bool {
        self.slot.is_busy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::DocumentSettings;

    #[test]
    fn non_active_timeline_request_uses_its_own_format_without_switching_tabs() {
        let scratch =
            std::env::temp_dir().join(format!("concat-export-non-active-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let settings = DocumentSettings {
            name: "Multiple timelines".into(),
            width: 1920,
            height: 1080,
            rate_num: 30,
            rate_den: 1,
        };
        let mut session = Session::open(&scratch.to_string_lossy(), settings).expect("opens");
        let original_id = session.project().active_timeline_id.clone();

        let created = session
            .apply(concat_project::Command::AddTimeline)
            .expect("adds a second timeline")
            .created_id
            .expect("new timeline id");
        session
            .set_video(concat_project::model::VideoSettings {
                width: 1080,
                height: 1920,
                rate_num: 30000,
                rate_den: 1001,
                color_space: Default::default(),
            })
            .expect("sets the second timeline's portrait format");
        session
            .apply(concat_project::Command::SelectTimeline {
                timeline_id: original_id.clone(),
            })
            .expect("returns to the original timeline");
        assert_eq!(session.project().active_timeline_id, original_id);

        // This second timeline is intentionally empty and is not active.
        // It must still be a valid export target with its own format.
        let request = request_for_timeline(
            &session,
            &ExportSpec::default(),
            Vec::new(),
            &created,
        )
        .expect("non-active timeline exists");
        assert_eq!((request.width, request.height), (1080, 1920));
        assert_eq!((request.rate_num, request.rate_den), (30000, 1001));
        assert!(request.clips.is_empty(), "empty timeline must not borrow active clips");
        assert_eq!(
            session.project().active_timeline_id,
            original_id,
            "building an export request must not switch the editor's active tab"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn timeline_request_uses_that_timelines_format_and_rejects_unknown_ids() {
        let scratch =
            std::env::temp_dir().join(format!("concat-export-request-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let settings = DocumentSettings {
            name: "Export request".into(),
            width: 1920,
            height: 1080,
            rate_num: 30,
            rate_den: 1,
        };
        let session = Session::open(&scratch.to_string_lossy(), settings).expect("opens");
        let timeline_id = session.project().active_timeline_id.clone();
        let request =
            request_for_timeline(&session, &ExportSpec::default(), Vec::new(), &timeline_id)
                .expect("known timeline");
        assert_eq!((request.width, request.height), (1920, 1080));
        assert_eq!((request.rate_num, request.rate_den), (30, 1));
        assert!(
            request_for_timeline(
                &session,
                &ExportSpec::default(),
                Vec::new(),
                "missing-timeline",
            )
            .is_err(),
            "an unknown id must not silently export a different timeline"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }
}
