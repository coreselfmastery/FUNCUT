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

/// What the sheet shows about one job in the export queue.
#[derive(Clone, Debug, PartialEq)]
pub enum QueueState {
    /// Waiting for the render slot.
    Queued,
    /// Holding the render slot: how far along, and what stage.
    Running {
        /// Of the whole, `0..=1`.
        progress: f32,
        /// What the exporter is doing, in the person's language.
        stage: String,
    },
    /// The file written.
    Done {
        /// Where it landed.
        path: String,
    },
    /// Why it did not finish. A job the person stopped is
    /// [`QueueState::Cancelled`], not a failure.
    Failed {
        /// The exporter's reason.
        error: String,
    },
    /// Stopped by the person before it finished.
    Cancelled,
}

/// One job in the export queue: the request snapshot built when it was
/// queued - later edits do not leak into a job already queued, and what
/// the person queued is what renders - held until the render slot takes
/// it, and the state the sheet shows for it.
#[derive(Clone)]
pub struct QueueJob {
    /// The job's id, unique for as long as the queue lives.
    pub id: u64,
    /// The timeline's name, as the picker listed it when queued.
    pub timeline_name: String,
    /// The file the job writes, for the row's label.
    pub output: String,
    /// The request to render. Taken by the slot when the job starts; a
    /// job that has not started keeps its snapshot.
    pub request: Option<ExportRequest>,
    /// What the sheet shows.
    pub state: QueueState,
}

/// What cancelling a job did, so the sheet knows whether the render
/// itself has to be stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancelled {
    /// The job was queued, not running: it is gone from the queue and
    /// nothing else has to happen.
    Removed,
    /// The job held the slot: the caller must ask the running render to
    /// stop, and the job already reads as cancelled.
    Stopping,
    /// No such job was waiting or running.
    Missing,
}

/// The export queue: every export asked for, one render slot, per-job
/// progress and per-job cancel. The pane pushes jobs as the person asks
/// for them and drives the queue with the render's reports; the queue
/// itself never starts a thread - it says what the slot should run next,
/// and the pane starts it, so the one-export-at-a-time invariant stays
/// where it has always been enforced (see [`Exporter`]).
///
/// Jobs run in the order they were queued. A job's request is built from
/// the timeline it names at the moment it is queued: the queue is what
/// the person saw, not whatever the timeline becomes while it waits.
#[derive(Default)]
pub struct ExportQueue {
    next_id: u64,
    jobs: Vec<QueueJob>,
}

impl ExportQueue {
    /// An empty queue; ids count from 1, so 0 can mean "no job".
    pub fn new() -> Self {
        Self {
            next_id: 1,
            jobs: Vec::new(),
        }
    }

    /// Adds a job and returns its id.
    pub fn push(
        &mut self,
        timeline_name: impl Into<String>,
        output: impl Into<String>,
        request: ExportRequest,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(QueueJob {
            id,
            timeline_name: timeline_name.into(),
            output: output.into(),
            state: QueueState::Queued,
            request: Some(request),
        });
        id
    }

    /// The job the render slot should take now: the first queued one,
    /// marked running and handed its request. `None` when nothing waits
    /// or a job already holds the slot - the slot is one, and taking a
    /// second job would race the first for its temp files and cancel
    /// flag.
    pub fn start_next(&mut self) -> Option<(u64, ExportRequest)> {
        if self.running().is_some() {
            return None;
        }
        let job = self
            .jobs
            .iter_mut()
            .find(|job| job.state == QueueState::Queued)?;
        job.state = QueueState::Running {
            progress: 0.0,
            stage: String::new(),
        };
        Some((job.id, job.request.take().expect("queued jobs keep one")))
    }

    /// The job holding the slot, if one is.
    pub fn running(&self) -> Option<&QueueJob> {
        self.jobs
            .iter()
            .find(|job| matches!(job.state, QueueState::Running { .. }))
    }

    /// The render's report for the job holding the slot. A report for
    /// any other id - a job already ended, one never queued - is not an
    /// error to the caller; the queue just takes no notice.
    pub fn progress(&mut self, id: u64, progress: f32, stage: impl Into<String>) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id)
            && let QueueState::Running { .. } = job.state
        {
            job.state = QueueState::Running {
                progress: progress.clamp(0.0, 1.0),
                stage: stage.into(),
            };
        }
    }

    /// A job's render ended: the file written, or why not. A result that
    /// arrives for a job already cancelled - the person asked it to stop,
    /// and the render's refusal is the stop it asked for - changes
    /// nothing: the job reads as the cancel it was. A result for an
    /// unknown id is no-one's.
    pub fn finish(&mut self, id: u64, result: Result<String, String>) {
        let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return;
        };
        if matches!(job.state, QueueState::Cancelled) {
            return;
        }
        job.state = match result {
            Ok(path) => QueueState::Done { path },
            Err(error) => QueueState::Failed { error },
        };
    }

    /// Whether any job is waiting for the slot.
    pub fn has_waiting(&self) -> bool {
        self.jobs.iter().any(|job| job.state == QueueState::Queued)
    }

    /// Cancels the job: a queued one is removed outright - it never
    /// started, so there is nothing to stop - and the running one reads
    /// as cancelled from here on, whoever reports its end first. The
    /// caller stops the render itself (see [`Exporter::cancel`]); a
    /// cancelled job's late report cannot un-cancel it
    /// (see [`ExportQueue::finish`]).
    pub fn cancel(&mut self, id: u64) -> Cancelled {
        let Some(index) = self.jobs.iter().position(|job| job.id == id) else {
            return Cancelled::Missing;
        };
        match self.jobs[index].state {
            QueueState::Queued => {
                self.jobs.remove(index);
                Cancelled::Removed
            }
            QueueState::Running { .. } => {
                self.jobs[index].state = QueueState::Cancelled;
                Cancelled::Stopping
            }
            _ => Cancelled::Missing,
        }
    }

    /// Drops the rows a person no longer needs to see: the done, the
    /// failed and the cancelled. The queued and the running stay.
    pub fn clear_ended(&mut self) {
        self.jobs.retain(|job| {
            !matches!(
                job.state,
                QueueState::Done { .. } | QueueState::Failed { .. } | QueueState::Cancelled
            )
        });
    }

    /// Every job, oldest first, for the sheet's list.
    pub fn jobs(&self) -> &[QueueJob] {
        &self.jobs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::{Command, DocumentSettings, MediaKind, NewMedia};

    #[test]
    fn non_active_timeline_request_uses_its_own_format_without_switching_tabs() {
        let scratch = std::env::temp_dir().join(format!(
            "concat-export-non-active-test-{}",
            std::process::id()
        ));
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
        let request = request_for_timeline(&session, &ExportSpec::default(), Vec::new(), &created)
            .expect("non-active timeline exists");
        assert_eq!((request.width, request.height), (1080, 1920));
        assert_eq!((request.rate_num, request.rate_den), (30000, 1001));
        assert!(
            request.clips.is_empty(),
            "empty timeline must not borrow active clips"
        );
        assert_eq!(
            session.project().active_timeline_id,
            original_id,
            "building an export request must not switch the editor's active tab"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn non_active_timeline_request_uses_its_own_clips_not_active_timeline_clips() {
        let scratch = std::env::temp_dir().join(format!(
            "concat-export-distinct-timelines-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let settings = DocumentSettings {
            name: "Distinct timelines".into(),
            width: 1920,
            height: 1080,
            rate_num: 30,
            rate_den: 1,
        };
        let mut session = Session::open(&scratch.to_string_lossy(), settings).expect("opens");
        let active_id = session.project().active_timeline_id.clone();

        let target_id = session
            .apply(Command::AddTimeline)
            .expect("adds target timeline")
            .created_id
            .expect("target timeline id");
        session
            .set_video(concat_project::model::VideoSettings {
                width: 1080,
                height: 1920,
                rate_num: 30000,
                rate_den: 1001,
                color_space: Default::default(),
            })
            .expect("sets target format");

        let target_path = scratch
            .join("target-only.mp4")
            .to_string_lossy()
            .into_owned();
        let target_media = session
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: target_path.clone(),
                    name: "target-only.mp4".into(),
                    duration: Some(7.0),
                    kind: MediaKind::Video,
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: Some(30.0),
                    frame_rate_fraction: Some("30/1".into()),
                    video_codec: Some("h264".into()),
                    audio_codec: None,
                    has_audio: false,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .expect("adds target media")
            .created_id
            .expect("target media id");
        let target_track = session.project().active().tracks[0].id.clone();
        session
            .apply(Command::AddClip {
                media_id: target_media,
                track_id: target_track,
                start: 1.0,
                ripple: false,
            })
            .expect("places target clip");

        session
            .apply(Command::SelectTimeline {
                timeline_id: active_id.clone(),
            })
            .expect("returns to active timeline");
        let active_path = scratch
            .join("active-only.mp4")
            .to_string_lossy()
            .into_owned();
        let active_media = session
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: active_path.clone(),
                    name: "active-only.mp4".into(),
                    duration: Some(3.0),
                    kind: MediaKind::Video,
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: Some(30.0),
                    frame_rate_fraction: Some("30/1".into()),
                    video_codec: Some("h264".into()),
                    audio_codec: None,
                    has_audio: false,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .expect("adds active media")
            .created_id
            .expect("active media id");
        let active_track = session.project().active().tracks[0].id.clone();
        session
            .apply(Command::AddClip {
                media_id: active_media,
                track_id: active_track,
                start: 0.0,
                ripple: false,
            })
            .expect("places active clip");

        let request =
            request_for_timeline(&session, &ExportSpec::default(), Vec::new(), &target_id)
                .expect("builds request for non-active timeline");
        assert_eq!((request.width, request.height), (1080, 1920));
        assert_eq!((request.rate_num, request.rate_den), (30000, 1001));
        assert_eq!(
            request.clips.len(),
            1,
            "only the target timeline clip exports"
        );
        assert_eq!(request.clips[0].path, target_path);
        assert_ne!(request.clips[0].path, active_path);
        assert_eq!(
            session.project().active_timeline_id,
            active_id,
            "building a request must not switch the editor's active tab"
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

    /// A session with two timelines of different shapes and different
    /// clips, for the queue's tests: what a queued job carries must be
    /// the timeline it was queued from, not the active one and not the
    /// other job's.
    fn two_timeline_session(tag: &str) -> (Session, String, String) {
        let scratch =
            std::env::temp_dir().join(format!("concat-export-queue-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let settings = DocumentSettings {
            name: "Queue".into(),
            width: 1920,
            height: 1080,
            rate_num: 30,
            rate_den: 1,
        };
        let mut session = Session::open(&scratch.to_string_lossy(), settings).expect("opens");
        let first = session.project().active_timeline_id.clone();
        let second = session
            .apply(Command::AddTimeline)
            .expect("adds a second timeline")
            .created_id
            .expect("new timeline id");
        session
            .set_video(concat_project::model::VideoSettings {
                width: 1080,
                height: 1920,
                rate_num: 24000,
                rate_den: 1001,
                color_space: Default::default(),
            })
            .expect("sets the second timeline's format");
        session
            .apply(Command::SelectTimeline {
                timeline_id: first.clone(),
            })
            .expect("returns to the first timeline");
        (session, first, second)
    }

    /// Every job runs, in the order queued, and the one that holds the
    /// slot is the only one that does: a second export while one runs is
    /// queued, not raced.
    #[test]
    fn jobs_run_in_order_one_at_a_time() {
        let mut queue = ExportQueue::new();
        let spec = ExportSpec::default();
        let request = ExportRequest {
            output: spec.output.clone(),
            ..ExportRequest::default()
        };
        let first = queue.push("Timeline 1", "one.mp4", request.clone());
        let second = queue.push("Timeline 2", "two.mp4", request);
        assert!(queue.has_waiting());

        let (running, _) = queue.start_next().expect("the first job starts");
        assert_eq!(running, first);
        assert!(
            queue.start_next().is_none(),
            "the slot is one; the second job waits"
        );
        queue.finish(first, Ok("one.mp4".into()));
        let (running, _) = queue.start_next().expect("the second job starts");
        assert_eq!(running, second);
        assert!(!queue.has_waiting());
        queue.finish(second, Ok("two.mp4".into()));
        assert!(queue.start_next().is_none(), "nothing waits");
    }

    /// Each queued job exports the timeline it was queued from: the
    /// request a job carries is the one built from that timeline, in
    /// frame, rate and clips, whatever the editor's active tab and
    /// whatever the other jobs carry.
    #[test]
    fn each_job_exports_the_timeline_it_was_queued_from() {
        let (session, first, second) = two_timeline_session("shapes");
        let scratch =
            std::env::temp_dir().join(format!("concat-export-queue-shapes-{}", std::process::id()));
        let spec = ExportSpec {
            output: "out.mp4".into(),
            ..ExportSpec::default()
        };
        let mut queue = ExportQueue::new();
        let portrait = request_for_timeline(&session, &spec, Vec::new(), &second)
            .expect("the portrait timeline exists");
        let landscape = request_for_timeline(&session, &spec, Vec::new(), &first)
            .expect("the landscape timeline exists");
        let portrait_id = queue.push("Timeline 2", "portrait.mp4", portrait);
        queue.push("Timeline 1", "landscape.mp4", landscape);

        let (_, request) = queue.start_next().expect("portrait first");
        assert_eq!((request.width, request.height), (1080, 1920));
        assert_eq!((request.rate_num, request.rate_den), (24000, 1001));
        queue.finish(portrait_id, Ok("portrait.mp4".into()));

        let (_, request) = queue.start_next().expect("landscape second");
        assert_eq!((request.width, request.height), (1920, 1080));
        assert_eq!((request.rate_num, request.rate_den), (30, 1));
        drop(session);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Progress reaches only the job that is running; a job's state is
    /// its own, not the queue's.
    #[test]
    fn progress_reaches_only_its_own_job() {
        let mut queue = ExportQueue::new();
        let request = ExportRequest::default();
        queue.push("Timeline 1", "one.mp4", request.clone());
        queue.push("Timeline 2", "two.mp4", request);
        let (first, _) = queue.start_next().expect("starts");

        queue.progress(first, 0.5, "Rendering video");
        let running = queue.running().expect("one job runs");
        match &running.state {
            QueueState::Running { progress, stage } => {
                assert_eq!(*progress, 0.5);
                assert_eq!(stage, "Rendering video");
            }
            other => panic!("the job is running, not {other:?}"),
        }

        // A report for a job that is not running changes nothing.
        queue.progress(99, 0.9, "Rendering video");
        let running = queue.running().expect("still running");
        match &running.state {
            QueueState::Running { progress, .. } => assert_eq!(*progress, 0.5),
            other => panic!("still running, not {other:?}"),
        }
    }

    /// Cancelling a queued job drops it without starting it; cancelling
    /// the running job is the stop of the render it names, and the
    /// render's late report cannot turn the cancel into a failure.
    #[test]
    fn cancel_removes_a_queued_job_and_stops_the_running_one() {
        let mut queue = ExportQueue::new();
        let request = ExportRequest::default();
        let first = queue.push("Timeline 1", "one.mp4", request.clone());
        let second = queue.push("Timeline 2", "two.mp4", request);
        assert_eq!(queue.cancel(second), Cancelled::Removed);
        assert_eq!(queue.jobs().len(), 1, "the queued job is gone");

        let (running, _) = queue.start_next().expect("starts");
        assert_eq!(running, first);
        assert_eq!(queue.cancel(first), Cancelled::Stopping);
        queue.finish(first, Err("cancelled at frame 12".into()));
        assert_eq!(
            queue.jobs()[0].state,
            QueueState::Cancelled,
            "the late refusal stays a cancel"
        );

        // The slot is free for whatever waits, which is nothing here.
        assert!(queue.start_next().is_none());
    }

    /// A failed job carries its reason and frees the slot for the next.
    #[test]
    fn a_failed_job_carries_its_reason_and_frees_the_slot() {
        let mut queue = ExportQueue::new();
        let request = ExportRequest::default();
        let first = queue.push("Timeline 1", "one.mp4", request.clone());
        let second = queue.push("Timeline 2", "two.mp4", request);
        let (running, _) = queue.start_next().expect("starts");
        assert_eq!(running, first);
        queue.finish(first, Err("the disk was full".into()));
        match &queue.jobs()[0].state {
            QueueState::Failed { error } => assert_eq!(error, "the disk was full"),
            other => panic!("failed, not {other:?}"),
        }
        assert!(
            queue.start_next().is_some(),
            "the slot freed for the second"
        );

        // And the rows a person no longer needs can be cleared.
        queue.finish(second, Ok("two.mp4".into()));
        queue.clear_ended();
        assert!(queue.jobs().is_empty());
    }
}
