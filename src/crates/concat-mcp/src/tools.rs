// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The tools: which API method or edit command each one is, and what an
//! agent is told about it.
//!
//! Every tool is a [`concat_api::Request`] in the end. Most are one method
//! or one edit command as it is, their arguments that request's fields and
//! their schema cut from the API's ([`crate::schema`]); a handful compose,
//! as captions are a batch of titles and waiting on an export is asking its
//! status until it ends, and say so. None decides what an edit means.
//!
//! Names are stable: an agent's instructions and a user's allow-list name
//! them. A tool is added, never renamed; a removed one would be a version
//! bump of this server.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use concat_api::{ApiError, ErrorCode, Request};
use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::schema::Schemas;
use crate::summary;

/// The longest preview side handed back when the agent names no size: a
/// frame an agent can look at without a 4K PNG in its context - and the
/// long side every proxy serves whole, so the default preview reads
/// each clip's working copy where it has one instead of decoding the
/// originals, which is the pause between one frame and the next. Name
/// `width` and `height` to see bigger.
const PREVIEW_SIDE: f64 = 960.0;
/// How often a wait asks how an export stands.
const POLL: Duration = Duration::from_millis(250);
/// How long `export_wait` waits when told nothing, and at most. Kept
/// under the tool's own 600 s outer limit so the wait's own answer -
/// "still running, call again" - is what a caller at the maximum gets,
/// not the watcher's cancellation at the deadline.
const WAIT_DEFAULT: f64 = 60.0;
const WAIT_LONGEST: f64 = 590.0;
/// How many stages an export has, for one progress figure across them.
const STAGES: usize = 5;

/// Where a stage comes in an export, from 0. A job's status names the
/// stage the exporter reports - "rendering", "mixing audio", "muxing" -
/// and the API's documentation names them "video", "audio" and "mux"; both
/// are known here, and one this does not know counts as the first.
fn stage_place(stage: &str) -> usize {
    match stage {
        "cutout" => 0,
        "stabilize" => 1,
        "video" | "rendering" => 2,
        "audio" | "mixing audio" => 3,
        "mux" | "muxing" => 4,
        _ => 0,
    }
}

/// What a tool does with its arguments.
#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// An API method, its arguments the method's params.
    Method {
        /// The method's name, `area.verb`.
        method: &'static str,
        /// What the reply is shown as.
        reply: Reply,
    },
    /// One edit command on a project: `edit.apply` with the command's
    /// fields as arguments, beside the project's `path`.
    Command {
        /// The command's `op`.
        op: &'static str,
    },
    /// `version`, with how this server reaches Concat added.
    Version,
    /// `project.get`, summarised.
    TimelineRead,
    /// Titles at the given times, as one batch.
    Captions,
    /// `export.status` until the job ends or the time is up.
    ExportWait,
    /// `updateClip` with only a mask.
    Mask,
    /// Apply base gain and optional automation in decibels.
    AudioLevelsDb,
    /// Validate the director's edit plan without changing a project.
    PlanValidate,
    /// `preview.frame`, as an image.
    PreviewFrame,
    /// The motion library, with the engine's own availability.
    MotionPresets,
    /// One comp summarised, ops applied first when given.
    MotionInspect,
    /// One comp read as an engineer reads it: census, timing, text,
    /// legibility, assets and the lint - no frame drawn.
    SceneAnalysis,
    /// One frame of a comp, drawn and written under the project.
    MotionFrame,
    /// The comp's motion watched: frames spread across the span.
    MotionPreview,
    /// A comp baked, and encoded when asked.
    MotionBake,
    /// A word-timed transcript as one caption video.
    CaptionsAnimate,
    /// A whole storyboard as one reel.
    MotionReel,
    /// What a media file is, as the engine reads it.
    MediaProbe,
    /// The validated timeline a storyboard becomes, printed as the
    /// engine reads it.
    SceneTimeline,
    /// What a diagnostic code means, in the engine's own words.
    GenevaExplain,
    /// A farm render's progress stream, read live.
    MotionFarmProgress,
    /// The farm's legs in the cloud: push, pull, a worker's day.
    MotionFarmCloud,
    /// Sample a render or the open timeline into a contact sheet.
    WatchRender,
    /// Look at B-roll candidates before choosing them.
    StockSheet,
    /// Score the cut on six craft axes and name concrete revisions.
    CraftCritique,
    /// Status or first-use setup of the local sub-1B vision critic.
    LocalVlm,
    /// The emotional read of the voiceover, and the gates it drives.
    Emotion,
    /// The voiceover read as a score: loudness, pauses, rate, prosody,
    /// speakers, entities - and the cut decisions they drive.
    VoiceSignals,
    /// One of the consumption-layer readings, reached through its
    /// family tool: the name selects the Python family module and the
    /// row that says which fields travel. Discourse, voice quality,
    /// prosody, audio repair and captions - what the director DOES with
    /// the voice_signals reading.
    Sense(&'static str),
    /// A family door to the consumption-layer readings: `action` in the
    /// arguments names one of the family's [`Sense`] members, and this
    /// dispatch forwards to it - thirty-three readings, five tools, so
    /// an agent's tool list stays small without losing a single one.
    SenseFamily(&'static str),
    /// Read or write the project's persisted creative brief.
    CreativeBrief,
    /// Composite a transparent motion bake over real footage.
    MotionOverlay,
    /// Measure an exported mix's integrated LUFS and true peak.
    MeasureLoudness,
    /// Beat-aligned cut times from the soundtrack, no Python extras.
    SuggestCuts,
    /// The subject's path across a clip, frame by frame: the multi-frame
    /// answer to the single-frame cropHint, as crop keyframes.
    TrackSubject,
    /// `project.get`, read for every place two clips share a lane.
    CollisionCheck,
    /// `openvid.search` once per query, one reply.
    OpenvidSearchBatch,
    /// Rank a page of stock against a slot's rich description: the CLIP
    /// second pass, the tags blended in, the hard filters enforced twice.
    OpenvidRankForSlot,
    /// The plan-level continuity gate over a filled manifest: colour,
    /// motion, scale, monotony, stillness - each flag with its slot,
    /// rule and remedy.
    FootageContinuity,
}

/// How a reply is shown to the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// As the API sent it.
    Raw,
    /// An editor view, as [`summary::view`].
    Summary,
}

/// One tool.
#[derive(Clone, Debug)]
pub struct Tool {
    /// The name an agent calls it by.
    pub name: &'static str,
    /// The name a person is shown.
    pub title: &'static str,
    /// What the agent is told: what it does, its units, its ranges.
    pub description: &'static str,
    /// Changes nothing.
    pub read_only: bool,
    /// May remove or overwrite something.
    pub destructive: bool,
    /// Calling it twice is calling it once.
    pub idempotent: bool,
    /// What it does.
    pub action: Action,
}

const fn method(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    method: &'static str,
    reply: Reply,
) -> Tool {
    Tool {
        name,
        title,
        description,
        read_only: false,
        destructive: false,
        idempotent: false,
        action: Action::Method { method, reply },
    }
}

const fn command(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    op: &'static str,
) -> Tool {
    Tool {
        name,
        title,
        description,
        read_only: false,
        destructive: false,
        idempotent: false,
        action: Action::Command { op },
    }
}

impl Tool {
    const fn reading(mut self) -> Tool {
        self.read_only = true;
        self.idempotent = true;
        self
    }

    const fn destroying(mut self) -> Tool {
        self.destructive = true;
        self
    }

    const fn idempotent(mut self) -> Tool {
        self.idempotent = true;
        self
    }
}

/// Every tool, in the order `tools/list` gives them.
pub fn catalogue() -> Vec<Tool> {
    use Reply::{Raw, Summary};
    vec![
        Tool {
            name: "concat_version",
            title: "Concat version",
            description: "The API version, build, capabilities and MCP mode (window or headless). Call it first when unsure what is available.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::Version,
        },
        method(
            "project_list",
            "Recent projects",
            "Recent projects on this machine, newest first: path, name, frame size and rate.",
            "project.list",
            Raw,
        )
        .reading(),
        method(
            "project_create",
            "Create a project",
            "Create and open a project folder `name` inside `location`; the summary's `path` is what later tools take.",
            "project.create",
            Summary,
        ),
        method(
            "project_open",
            "Open a project",
            "Open a project folder to read and edit it; opening one already open returns it as it stands, unsaved edits included.",
            "project.open",
            Summary,
        )
        .idempotent(),
        method(
            "project_save",
            "Save a project",
            "Write an open project's document to its folder; edits autosave anyway, so this is for renaming or tidiness.",
            "project.save",
            Raw,
        )
        .idempotent(),
        method(
            "project_close",
            "Close a project",
            "Close an open project; `save: true` writes the document once more before it goes.",
            "project.close",
            Raw,
        )
        .destroying(),
        Tool {
            name: "timeline_read",
            title: "Read the timeline",
            description: "The active timeline as a compact summary: lanes, clips in time order with ids, the media bin. Read it for the ids other tools need.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::TimelineRead,
        },
        Tool {
            name: "plan_validate",
            title: "Validate an edit plan",
            description: "Validate a director plan's schema, cross-field constraints AND creative readiness before editing; returns blockers, craft score, and checks the signalLog.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::PlanValidate,
        },
        Tool {
            name: "clip_audio_levels_db",
            title: "Set clip audio levels in dB",
            description: "Set a clip's base gain and automation in dB relative to unity (-60 to +24), as one undoable batch.",
            read_only: false,
            destructive: true,
            idempotent: true,
            action: Action::AudioLevelsDb,
        },
        Tool {
            name: "collision_check",
            title: "Check for clips sharing a lane",
            description: "Report every place two clips share a lane - only a damaged or hand-written document has them.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::CollisionCheck,
        },
        method(
            "project_get",
            "Get the whole project",
            "An open project's full state; large, and `timeline_read` is usually enough.",
            "project.get",
            Raw,
        )
        .reading(),
        method(
            "project_set_video",
            "Set frame size and rate",
            "Set the active timeline's frame size and rate as one undoable edit.",
            "project.setVideo",
            Summary,
        ),
        method(
            "media_probe",
            "Probe a media file",
            "Report what is inside a media file - duration, streams, frame size, codecs - without touching any project.",
            "media.probe",
            Raw,
        )
        .reading(),
        method(
            "media_import",
            "Import media",
            "Add a file to an open project's media bin; the summary's `createdId` places it with clip_add.",
            "media.import",
            Summary,
        )
        .idempotent(),
        method(
            "openvid_status",
            "OpenVid library status",
            "The OpenVid stock library's state: missing, downloading (with percent), ready or failed, and how many clips it holds.",
            "openvid.status",
            Raw,
        )
        .reading(),
        method(
            "openvid_prepare",
            "Download the OpenVid index",
            "Start the one-time ~435 MB download of the OpenVid index; poll openvid_status until it is ready.",
            "openvid.prepare",
            Raw,
        )
        .idempotent(),
        method(
            "openvid_search",
            "Search stock clips",
            "Search 937,957 stock clips by caption words and quality numbers; a local CLIP rerank re-orders a relevance page by what the clips SHOW.",
            "openvid.search",
            Raw,
        )
        .reading(),
        Tool {
            name: "openvid_search_batch",
            title: "Search stock clips, several queries at once",
            description: "The openvid_search search once per entry of `queries` - shop a whole plan's B-roll in one round trip, then look with stock_contact_sheet.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::OpenvidSearchBatch,
        },
        Tool {
            name: "openvid_rank_for_slot",
            title: "Rank stock against a slot",
            description: "Rank a page of stock against a slot's rich description with the CLIP second pass, tags blended at tag_weight; used ids and clips under the motion floor are never returned.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::OpenvidRankForSlot,
        },
        Tool {
            name: "footage_continuity",
            title: "Check the filled slots' continuity",
            description: "Read a filled manifest the way an editor scrubs: colour jumps, reversed travels, repeated scales, one-category runs, clips under their slot's motion floor - each flag names its slot, rule and remedy.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::FootageContinuity,
        },
        method(
            "openvid_get",
            "Read clips in full",
            "Everything about up to 20 clips by id, for checking a candidate before committing to the fetch.",
            "openvid.get",
            Raw,
        )
        .reading(),
        method(
            "openvid_fetch",
            "Fetch a clip",
            "Fetch one clip's MP4 into the app's cache and return its `localPath`; adds nothing to any project.",
            "openvid.fetch",
            Raw,
        )
        .idempotent(),
        method(
            "openvid_import",
            "Import stock clips",
            "Fetch stock clips and add them to a project's bin, each copied into the project's own openvid folder.",
            "openvid.import",
            Raw,
        ),
        method(
            "sounds_search",
            "Search the sound library",
            "Search freesound effects by words, category and length; candidates carry ids, durations and licenses.",
            "sounds.search",
            Raw,
        )
        .reading(),
        method(
            "sounds_add",
            "Add a sound",
            "Add a freesound sound to an open project's bin, and place it on the timeline when asked.",
            "sounds.add",
            Raw,
        ),
        method(
            "bgm_status",
            "Music library status",
            "The background-music library's state: missing, downloading, ready or failed.",
            "bgm.status",
            Raw,
        )
        .reading(),
        method(
            "bgm_prepare",
            "Load the music library",
            "Start the one-time download of the BGM library; poll bgm_status until it is ready.",
            "bgm.prepare",
            Raw,
        )
        .idempotent(),
        method(
            "bgm_search",
            "Search music",
            "Search the free music library by mood words; candidates carry ids, durations and licenses.",
            "bgm.search",
            Raw,
        )
        .reading(),
        method(
            "bgm_fetch",
            "Fetch a track",
            "Fetch one music track into the app's cache and return its `localPath`.",
            "bgm.fetch",
            Raw,
        )
        .idempotent(),
        method(
            "bgm_import",
            "Import music",
            "Fetch a music track and add it to a project's bin, optionally placing it on the timeline.",
            "bgm.import",
            Raw,
        ),
        method(
            "magick_status",
            "Sticker library status",
            "The ImageMagick sticker library's state: missing, downloading, ready or failed.",
            "magick.status",
            Raw,
        )
        .reading(),
        method(
            "magick_prepare",
            "Load the sticker library",
            "Start the one-time download of the sticker index; poll magick_status until it is ready.",
            "magick.prepare",
            Raw,
        )
        .idempotent(),
        method(
            "magick_search",
            "Search stickers",
            "Search the sticker library by words and style; candidates carry ids and previews.",
            "magick.search",
            Raw,
        )
        .reading(),
        method(
            "magick_fetch",
            "Fetch a sticker",
            "Fetch one sticker's file into the app's cache and return its `localPath`.",
            "magick.fetch",
            Raw,
        )
        .idempotent(),
        method(
            "magick_add",
            "Add a sticker",
            "Add a sticker to an open project, on the timeline when asked.",
            "magick.add",
            Raw,
        ),
        method(
            "edit_apply",
            "Apply an edit",
            "Apply one or many edit commands as one undoable batch - the whole editing vocabulary in one call.",
            "edit.apply",
            Summary,
        ),
        method(
            "edit_undo",
            "Undo",
            "Undo the last applied edit - every edit tool's change, one step per batch.",
            "edit.undo",
            Summary,
        ),
        method(
            "edit_redo",
            "Redo",
            "Redo the last undone edit, after edit_undo took it back.",
            "edit.redo",
            Summary,
        ),
        command(
            "clip_add",
            "Place a clip",
            "Place media from the bin on a lane as one undoable edit; a clip landing on another moves to the nearest free start.",
            "addClip",
        ),
        command(
            "clips_add_sequence",
            "Place clips end to end",
            "Lay media items on one lane back to back in the order given, as one undoable step.",
            "addClipsInSequence",
        ),
        command(
            "clip_trim",
            "Trim a clip",
            "Move one edge of a clip by `delta` seconds; `ripple: true` closes or opens the gap behind it.",
            "trimClip",
        ),
        command(
            "clips_split",
            "Split clips",
            "Split one clip at a time into two, on the same lane.",
            "splitClips",
        ),
        command(
            "clip_cut_spans",
            "Cut spans out of a clip",
            "Cut spans out of a clip across its lane - the dead-air remover.",
            "removeSpans",
        ),
        command(
            "clips_move",
            "Move clips",
            "Move clips to another lane or start, as one undoable step.",
            "moveClips",
        ),
        command(
            "clips_remove",
            "Delete clips",
            "Remove clips from the timeline, as one undoable step.",
            "removeClips",
        )
        .destroying(),
        command(
            "clip_update",
            "Change a clip",
            "Change a clip's fields - transform, speed, volume, opacity, effects, text - as one undoable patch.",
            "updateClip",
        ),
        Tool {
            name: "clip_mask",
            title: "Mask a clip",
            description: "Set or clear one clip's mask: the shape that hides part of the picture.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::Mask,
        },
        command(
            "clip_stabilize",
            "Stabilize a clip",
            "Stabilize a clip's camera shake; a job whose progress analysis_wait reports.",
            "setClipStabilize",
        ),
        command(
            "clip_frame",
            "Fit or fill the frame",
            "Set a clip's crop/pan framing - fill (cover + crop) or fit - for the timeline's aspect.",
            "frameClip",
        ),
        command(
            "timeline_background",
            "Set the background",
            "Set the timeline's background colour, gradient or blur plate.",
            "setTimelineBackground",
        ),
        command(
            "timeline_linkage",
            "Link the lanes to the main one",
            "Turn the timeline's ripple linkage between lanes on or off.",
            "setTimelineLinkage",
        ),
        command(
            "clip_transform",
            "Place, scale or rotate a clip",
            "Set a clip's transform: position, scale, rotation, opacity, anchor.",
            "setClipTransform",
        ),
        command(
            "clip_speed",
            "Change a clip's speed",
            "Change a clip's playback speed, pitch-corrected when asked.",
            "setClipSpeed",
        ),
        command(
            "title_add",
            "Add a title",
            "Add a styled title to a lane - the caption and text-card tool, with per-word styling.",
            "addTextClip",
        ),
        command(
            "title_style_words",
            "Style some of a title's words",
            "Style individual words of a title: the kinetic-caption look, per word.",
            "styleTextRange",
        ),
        command(
            "title_clear_word_styles",
            "Clear words' own looks",
            "Take a title's per-word styles off, back to the shared style.",
            "clearTextStyles",
        ),
        Tool {
            name: "captions_add",
            title: "Add captions",
            description: "Add segment-timed captions to the timeline as one batch of titles.",
            read_only: false,
            destructive: false,
            idempotent: false,
            action: Action::Captions,
        },
        command(
            "keyframe_set",
            "Set a keyframe",
            "Set keyframes on a clip's property - the volume duck, the zoom, the fade.",
            "setClipKey",
        ),
        command(
            "keyframe_clear",
            "Remove a keyframe",
            "Clear the keyframes of one property on a clip.",
            "clearClipKey",
        )
        .destroying(),
        command(
            "effect_layer_add",
            "Add an effect layer",
            "Add an effect as a timeline-wide layer - the grade - with its params.",
            "addLayerClip",
        ),
        command(
            "effect_key_set",
            "Animate an effect parameter",
            "Keyframe one of an effect's parameters over time - the grade that breathes.",
            "setEffectKey",
        ),
        command(
            "track_add",
            "Add a lane",
            "Add a lane (track) to the timeline for clips, titles or sounds.",
            "addTrack",
        ),
        method(
            "effects_list",
            "List effects",
            "List the effect library: filters, transitions, their ids and params.",
            "catalogue.list",
            Raw,
        )
        .reading(),
        method(
            "template_list",
            "List templates",
            "List the project templates available to instantiate.",
            "template.list",
            Raw,
        )
        .reading(),
        method(
            "template_instantiate",
            "Make a project from a template",
            "Instantiate a template project into a new project.",
            "template.instantiate",
            Summary,
        ),
        method(
            "export_start",
            "Start an export",
            "Start rendering the timeline to a movie; a job whose progress export_status reports.",
            "export.run",
            Raw,
        ),
        method(
            "export_status",
            "Export status",
            "An export job's state, stage and progress.",
            "export.status",
            Raw,
        )
        .reading(),
        Tool {
            name: "export_wait",
            title: "Wait for an export",
            description: "Poll an export until it ends or the time is up - the agent's wait-for-the-render call.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::ExportWait,
        },
        method(
            "export_cancel",
            "Cancel an export",
            "Cancel a running export someone started; the partial file is not kept.",
            "export.cancel",
            Raw,
        )
        .destroying(),
        Tool {
            name: "preview_frame",
            title: "Preview a frame",
            description: "One frame of the timeline as an image the agent can see; name width/height for bigger.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::PreviewFrame,
        },
        method(
            "analyze_scenes",
            "Find scene cuts",
            "Detect scene cuts in a media file; returns cut times with confidence.",
            "analysis.scenes",
            Raw,
        ),
        method(
            "analyze_beats",
            "Find the beat grid",
            "Detect musical beats in a media file; returns a beat grid (needs the Python extras).",
            "analysis.beats",
            Raw,
        ),
        method(
            "analyze_silence",
            "Find dead air",
            "Detect silences in a media file; returns spans with durations - the dead-air map.",
            "analysis.silence",
            Raw,
        ),
        method(
            "transcribe",
            "Transcribe a recording",
            "Transcribe a media file's speech; a job returning segments and word timings.",
            "speech.transcribe",
            Raw,
        ),
        method(
            "tts_generate",
            "Speak a voiceover",
            "Generate speech from text with a local voice; a job returning an audio file.",
            "speech.generate",
            Raw,
        ),
        method(
            "models_list",
            "List speech models",
            "List the local AI models and which are missing.",
            "models.list",
            Raw,
        )
        .reading(),
        method(
            "models_download",
            "Download a speech model",
            "Download a missing model - voice, transcriber, vision; a job, poll models_list.",
            "models.download",
            Raw,
        ),
        Tool {
            name: "motion_presets",
            title: "List the motion library",
            description: "The motion-comp preset library, with each preset's availability.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::MotionPresets,
        },
        Tool {
            name: "motion_inspect",
            title: "Inspect a motion comp",
            description: "One motion comp summarised: census, timing, text, legibility, assets and the lint.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::MotionInspect,
        },
        Tool {
            name: "scene_analysis",
            title: "Analyse a scene before a frame is drawn",
            description: "One comp read as an engineer reads it - no frame drawn: census, timing, text, legibility, lint.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::SceneAnalysis,
        },
        Tool {
            name: "motion_frame",
            title: "Draw one motion frame",
            description: "Draw one frame of a comp and write it under the project, for looking.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionFrame,
        },
        Tool {
            name: "motion_preview",
            title: "Watch a comp's motion",
            description: "The comp's motion watched: frames spread across the span.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionPreview,
        },
        Tool {
            name: "motion_bake",
            title: "Bake a motion comp",
            description: "Bake a comp to frames, and encode when asked - the render step.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionBake,
        },
        Tool {
            name: "captions_animate",
            title: "Caption a transcript word by word",
            description: "A word-timed transcript in, one kinetic caption video out.",
            read_only: false,
            destructive: false,
            idempotent: false,
            action: Action::CaptionsAnimate,
        },
        Tool {
            name: "motion_reel",
            title: "Render a storyboard as one reel",
            description: "A whole storyboard document in, one animated reel out.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionReel,
        },
        Tool {
            name: "motion_probe",
            title: "Inspect a media file",
            description: "What the motion engine can do here: fonts, presets, availability.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::MediaProbe,
        },
        Tool {
            name: "scene_timeline",
            title: "Print a validated storyboard timeline",
            description: "The validated timeline a storyboard becomes, printed as the engine reads it.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::SceneTimeline,
        },
        Tool {
            name: "geneva_explain",
            title: "Explain a motion diagnostic code",
            description: "What a diagnostic code means, in the engine's own words.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::GenevaExplain,
        },
        Tool {
            name: "motion_farm_progress",
            title: "Read a farm render's progress",
            description: "A farm render's progress stream, read live.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::MotionFarmProgress,
        },
        Tool {
            name: "motion_farm_cloud",
            title: "The farm in the cloud: push, pull, or a worker's day",
            description: "The render farm on object storage: push, pull, status, or work one part - the bucket and keys come from the operator's env.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionFarmCloud,
        },
        Tool {
            name: "analysis_wait",
            title: "Wait for a job",
            description: "Wait for an analysis or model job (stabilize, transcribe, tts, download) to finish; polls until done.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::ExportWait,
        },
        Tool {
            name: "watch_render",
            title: "Watch the cut",
            description: "Sample a render or the timeline into a contact sheet the agent can SEE, with loudness and craft notes.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::WatchRender,
        },
        Tool {
            name: "stock_contact_sheet",
            title: "Look at B-roll candidates",
            description: "Look at B-roll candidates before choosing them: a labelled contact sheet image.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::StockSheet,
        },
        Tool {
            name: "craft_critique",
            title: "Score the cut",
            description: "Score the cut on six craft axes from a contact sheet, with concrete revisions named.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::CraftCritique,
        },
        Tool {
            name: "suggest_cuts",
            title: "Find the musical cut points",
            description: "Beat-aligned cut times from any soundtrack's energy onsets (no Python extras).",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::SuggestCuts,
        },
        Tool {
            name: "measure_loudness",
            title: "Measure the mix",
            description: "Measure an exported mix's integrated LUFS, true peak and LRA against a platform target.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MeasureLoudness,
        },
        Tool {
            name: "track_subject",
            title: "Follow the subject",
            description: "The subject's path across a clip, frame by frame, as crop keyframes.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::TrackSubject,
        },
        Tool {
            name: "motion_overlay",
            title: "Composite motion over footage",
            description: "Composite a transparent motion bake over real footage.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionOverlay,
        },
        Tool {
            name: "local_vlm",
            title: "Local or hosted vision critic",
            description: "Status or first-use setup of the local sub-1B vision critic.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::LocalVlm,
        },
        Tool {
            name: "emotion",
            title: "Read what the voiceover feels",
            description: "The voiceover's emotional read per line - emotion, act, delivery, colour, duck depth - and the stock gates it drives.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::Emotion,
        },
        Tool {
            name: "voice_signals",
            title: "Read the voiceover as a score",
            description: "The voiceover read as a score: loudness, pauses, rate, emphasis, speakers, entities - with per-signal confidence, the decision policy and the cut decisions they drive.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::VoiceSignals,
        },
        // The five family doors to the thirty-three consumption-layer
        // readings: what the director DOES with the voice_signals profile,
        // grouped so an agent's tool list stays small without losing a
        // single reading - the `action` names the reading, the family
        // names the Python module behind it, and every one degrades
        // loudly, never silently.
        Tool {
            name: "sense_discourse",
            title: "The script as language",
            description: "The script as language, behind one `action`: connectives, topics, speech acts, readability, claim graph, script quality, sarcasm, code switching, cross-lingual emotion.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::SenseFamily("discourse"),
        },
        Tool {
            name: "sense_voice_quality",
            title: "The take's health",
            description: "The take's health, behind one `action`: vocal events, voice class, recording class, reverb, restarts, filler types.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::SenseFamily("voiceq"),
        },
        Tool {
            name: "sense_prosody_style",
            title: "How the delivery moves",
            description: "The delivery, behind one `action`: emphasis types, speaking style, rhythm and meter, tempo changes, accent, vocal tract.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::SenseFamily("prosody"),
        },
        Tool {
            name: "sense_audio_repair",
            title: "The clean and the music",
            description: "The clean and the music, behind one `action`: enhance speech, remove hiss/hum/rumble, normalize loudness, detect/identify/structure music.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::SenseFamily("audiorepair"),
        },
        Tool {
            name: "sense_captions",
            title: "The words made to agree with the take",
            description: "The words made to agree with the take, behind one `action`: TTS quality, AI-voice detection, voice sealing, caption alignment, subtitle hygiene.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::SenseFamily("captions"),
        },
        Tool {
            name: "creative_brief",
            title: "Set the creative intent",
            description: "Read or write the project's persisted creative brief - the WHY a plan is validated against.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::CreativeBrief,
        },
    ]
}

impl Tool {
    /// How long one call to this tool may run, in seconds, before the
    /// server answers for it: the per-tool table that replaces one
    /// global 900-second client timeout. The patient work keeps its
    /// patience - transcription is real-time audio decode, an export is
    /// a whole render, a reel or a bake or a watch is a sidecar render -
    /// and everything else is a document read or an edit, which has no
    /// business taking a minute. The engine's own calls are bounded a
    /// second time, per call, by the Geneva guard (see docs/MOTION.md),
    /// so a hanging sidecar is killed at its own limit and this table is
    /// the outer answer.
    ///
    /// Every limit here sits at or above the tool's own internal
    /// deadline - the sense scripts' `kill_group` budgets, the VLM
    /// download, the reel's per-call caps - so a healthy call is never
    /// answered for at the limit while its work is still legitimately
    /// running: the inner deadline is what names a stuck call, and this
    /// is the net under a call whose inner deadline somehow never fires.
    pub fn timeout_secs(&self) -> u64 {
        const TRANSCRIBE: &[&str] = &["transcribe", "analysis_wait"];
        const EXPORT: &[&str] = &[
            "export_start",
            "export_wait",
            // The sense family, whose own budgets are 240-480 s: a
            // critique, a cut suggestion, a loudness pass, an emotion
            // read, a voice-signal read, a contact sheet with its ranking,
            // and the five family doors behind which the thirty-three
            // consumption-layer readings live (their member budgets are
            // the sense.rs table's own).
            "craft_critique",
            "suggest_cuts",
            "measure_loudness",
            "emotion",
            "voice_signals",
            "stock_contact_sheet",
            "sense_discourse",
            "sense_voice_quality",
            "sense_prosody_style",
            "sense_audio_repair",
            "sense_captions",
            // The library work whose cost is the network's: a semantic
            // stock search runs a 240 s rerank budget inside it, and an
            // import fetches and copies up to twenty items - a slow link
            // is not a stuck call, and the 60 s default would cancel
            // healthy work mid-flight.
            "openvid_search",
            "openvid_import",
            "bgm_import",
        ];
        const RENDER: &[&str] = &[
            "motion_bake",
            "motion_frame",
            "motion_preview",
            "captions_animate",
            "watch_render",
        ];
        const DEEP: &[&str] = &[
            // The two whose own internal deadline is 900 s: the local
            // VLM's model download and an overlay encode. Their outer
            // bound sits above the inner one, so the inner deadline is
            // what answers a genuinely stuck call - the outer net never
            // cancels work that is still inside its own budget.
            "local_vlm",
            "motion_overlay",
            // A reel render's own ceiling (genevaTimeoutSecs, 1140 at
            // most) has to sit under the outer bound too, or a caller
            // asking for the documented maximum is cancelled by the
            // net rather than named by the deadline.
            "motion_reel",
            // The cloud farm's push and its worker's whole day: an
            // upload of a staged tree on a slow link, or a part drawn
            // through the sidecar, both outlive a render's own budget.
            "motion_farm_cloud",
            // A batch runs up to eight searches, each with the 240 s
            // rerank budget `openvid_search` runs inside it; the outer
            // bound is the net the rarest, slowest batch hits, not the
            // arithmetic of every budget at once.
            "openvid_search_batch",
        ];
        if DEEP.contains(&self.name) {
            1200
        } else if TRANSCRIBE.contains(&self.name) {
            900
        } else if EXPORT.contains(&self.name) {
            600
        } else if RENDER.contains(&self.name) {
            300
        } else {
            60
        }
    }
}

/// What a tool call came to: MCP content, and whether it is an error the
/// agent should read and correct.
pub struct Outcome {
    /// The content blocks.
    pub content: Vec<Value>,
    /// The same result as JSON, for a client that reads it structured.
    pub structured: Option<Value>,
    /// A tool execution error.
    pub is_error: bool,
}

impl Outcome {
    pub(crate) fn json(value: Value) -> Outcome {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Outcome {
            content: vec![json!({ "type": "text", "text": text })],
            structured: value.is_object().then_some(value),
            is_error: false,
        }
    }

    fn error(error: &ApiError) -> Outcome {
        let code = serde_json::to_value(error.code)
            .ok()
            .and_then(|code| code.as_str().map(str::to_owned))
            .unwrap_or_default();
        Outcome {
            content: vec![
                json!({ "type": "text", "text": format!("{} (code: {code})", error.message) }),
            ],
            structured: Some(json!({ "error": { "code": code, "message": error.message } })),
            is_error: true,
        }
    }
}

/// What a call may use besides its arguments.
pub struct Context<'a> {
    /// Where requests go.
    pub backend: &'a mut dyn Backend,
    /// Set when the client cancels the call.
    pub cancelled: &'a AtomicBool,
    /// Reports progress, when the client asked for it: done so far, out
    /// of a total, and a sentence.
    pub progress: Option<&'a dyn Fn(f64, f64, String)>,
}

/// The tools with their schemas, made once.
pub struct Toolbox {
    tools: Vec<(Tool, Value)>,
    schemas: Schemas,
}

impl Toolbox {
    /// Every tool in [`catalogue`], with the schema the API gives it.
    /// Panics on a tool whose method or command the API does not have: a
    /// test builds this, so the panic is a failed build, not a user's.
    pub fn new() -> Toolbox {
        let schemas = Schemas::generate();
        let tools = catalogue()
            .into_iter()
            .map(|tool| {
                let schema = input_schema(&schemas, &tool)
                    .unwrap_or_else(|| panic!("the API has nothing for the {} tool", tool.name));
                (tool, schema)
            })
            .collect();
        Toolbox { tools, schemas }
    }

    /// `tools/list`'s list.
    pub fn list(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|(tool, schema)| {
                json!({
                    "name": tool.name,
                    "title": tool.title,
                    "description": tool.description,
                    "inputSchema": schema,
                    "annotations": {
                        "title": tool.title,
                        "readOnlyHint": tool.read_only,
                        "destructiveHint": tool.destructive,
                        "idempotentHint": tool.idempotent,
                        "openWorldHint": false,
                    },
                })
            })
            .collect()
    }

    /// The tool called `name`, with its schema.
    pub fn find(&self, name: &str) -> Option<&(Tool, Value)> {
        self.tools.iter().find(|(tool, _)| tool.name == name)
    }

    /// The schemas tools are cut from.
    pub fn schemas(&self) -> &Schemas {
        &self.schemas
    }

    /// Runs the tool `name`; `None` when there is no such tool.
    pub fn call(&self, name: &str, arguments: Value, context: Context<'_>) -> Option<Outcome> {
        let (tool, schema) = self.find(name)?;
        let arguments = match arguments {
            Value::Null => Value::Object(Map::new()),
            other => other,
        };
        // The schema the tool was listed with is the contract both sides
        // were shown, and it is checked before anything runs: a key it
        // does not name is a misspelling - `style.text` where the words
        // go, `trackId` on a command that takes `track` - that the
        // request layer would otherwise read as silently absent, and a
        // refusal that names it is one round trip instead of an edit
        // that lands half-read.
        if let Err(why) = crate::schema::check(schema, &arguments, schema, name) {
            return Some(Outcome::error(&ApiError::invalid(format!(
                "invalid arguments: {why}"
            ))));
        }
        Some(match run(tool, arguments, context) {
            Ok(outcome) => outcome,
            Err(error) => Outcome::error(&error),
        })
    }
}

impl Default for Toolbox {
    fn default() -> Self {
        Toolbox::new()
    }
}

/// The schema a tool's arguments follow.
fn input_schema(schemas: &Schemas, tool: &Tool) -> Option<Value> {
    match tool.action {
        Action::Method { method, .. } => schemas.method(method),
        Action::Command { op } => schemas.command(op),
        Action::Version => schemas.method("version"),
        Action::TimelineRead => schemas.method("project.get"),
        Action::PlanValidate => plan_validate_schema(),
        Action::AudioLevelsDb => audio_levels_schema(schemas),
        Action::ExportWait => {
            let mut schema = schemas.method("export.status")?;
            schema["properties"]["timeoutSeconds"] = json!({
                "type": "number",
                "minimum": 0,
                "maximum": WAIT_LONGEST,
                "default": WAIT_DEFAULT,
                "description": "How long to wait, in seconds.",
            });
            Some(schema)
        }
        Action::PreviewFrame => schemas.method("preview.frame"),
        Action::CollisionCheck => schemas.method("project.get"),
        Action::OpenvidSearchBatch => {
            // The same shape `openvid_search` takes, one query a list of
            // them: the properties are cut from the API's own schema so
            // the floors and filters cannot drift from it.
            let search = schemas.method("openvid.search")?;
            let mut properties = search
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            properties.remove("query");
            properties.insert(
                "queries".to_owned(),
                json!({
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 8,
                    "items": { "type": "string", "minLength": 1 },
                    "description": "The searches to run, one `openvid_search` query each: two or three words, a noun a camera could film.",
                }),
            );
            properties.insert(
                "rerank".to_owned(),
                json!({
                    "type": "boolean",
                    "default": true,
                    "description": "Whether each query gets the CLIP rerank `openvid_search` runs (the default) or a fast BM25-only sweep.",
                }),
            );
            Some(json!({
                "type": "object",
                "properties": properties,
                "required": ["queries"],
                "additionalProperties": false,
            }))
        }
        Action::OpenvidRankForSlot => Some(json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "minLength": 1,
                    "description": "REQUIRED. The slot's rich description - the words CLIP ranks the page against. Empty or whitespace is an error, not a fallback to the queries.",
                },
                "queries": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "The slot's fallback keyword phrases, blended in at tag_weight.",
                },
                "k": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "description": "How many clips to return. Absent: 30 when hero, else 12.",
                },
                "hero": {
                    "type": "boolean",
                    "default": false,
                    "description": "Whether the slot is the shot the thumbnail and the first glance live on; widens the default k to 30.",
                },
                "tag_weight": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 1,
                    "default": 0.3,
                    "description": "How much the tags count in the blend; the CLIP order carries the rest.",
                },
                "motion_min": {
                    "type": "number",
                    "description": "Hard floor: clips below this motion score are never returned. (motionMin accepted.)",
                },
                "exclude_ids": {
                    "type": "array",
                    "maxItems": 500,
                    "items": { "type": "integer" },
                    "description": "Hard filter: clip ids already used in this edit are never returned. (excludeIds accepted.)",
                },
            },
            "required": ["description"],
            "additionalProperties": false,
        })),
        Action::FootageContinuity => Some(json!({
            "type": "object",
            "properties": {
                "clips": {
                    "type": "array",
                    "maxItems": 200,
                    "description": "The filled slots, in play order. Each carries the slot's id, the clip that filled it, and what the search measured: colour bias, motion, travel, scale, category - and the motion floor the slot asked for.",
                    "items": {
                        "type": "object",
                        "required": ["slotId", "clipId"],
                        "additionalProperties": false,
                        "properties": {
                            "slotId": { "type": "string", "minLength": 1 },
                            "clipId": { "type": "string", "minLength": 1 },
                            "colorTemperature": { "type": "number", "minimum": -1, "maximum": 1, "description": "Colour bias, -1.0 cool through +1.0 warm." },
                            "motion": { "type": "number", "description": "How much moves, on the library's own motion scale." },
                            "dominantDirection": { "type": "string", "enum": ["left", "right", "up", "down", "zoom_in", "zoom_out", "static"] },
                            "shotScale": { "type": "string", "enum": ["wide", "medium", "close_up"] },
                            "category": { "type": "string", "description": "The clip's broad category, as the plan's vocabulary names it." },
                            "motionMin": { "type": "number", "description": "The least motion the slot would take." },
                        },
                    },
                },
            },
            "required": ["clips"],
            "additionalProperties": false,
        })),
        Action::MotionPresets
        | Action::MotionInspect
        | Action::SceneAnalysis
        | Action::MotionFrame
        | Action::MotionPreview
        | Action::MotionBake
        | Action::CaptionsAnimate
        | Action::MotionReel
        | Action::MediaProbe
        | Action::SceneTimeline
        | Action::GenevaExplain
        | Action::MotionFarmProgress
        | Action::MotionFarmCloud => motion_schema(tool.name),
        Action::WatchRender
        | Action::StockSheet
        | Action::CraftCritique
        | Action::LocalVlm
        | Action::Emotion
        | Action::VoiceSignals
        | Action::Sense(_)
        | Action::CreativeBrief
        | Action::MotionOverlay
        | Action::MeasureLoudness
        | Action::SuggestCuts
        | Action::TrackSubject => sense_schema(tool.name),
        Action::SenseFamily(_) => Some(crate::sense::sense_family_schema(tool.name)),
        Action::Mask => {
            let update = schemas.command("updateClip")?;
            let mut mask = schemas.def("ClipMask")?;
            mask["type"] = json!(["object", "null"]);
            mask["description"] = json!("The shape and its numbers, or null to take the mask off.");
            Some(json!({
                "type": "object",
                "properties": {
                    "path": update.pointer("/properties/path")?.clone(),
                    "clipId": update.pointer("/properties/clipId")?.clone(),
                    "mask": mask,
                },
                "required": ["path", "clipId", "mask"],
                "additionalProperties": false,
            }))
        }
        Action::Captions => {
            let path = schemas
                .method("project.get")?
                .pointer("/properties/path")?
                .clone();
            let mut style = schemas.def("TextStyle")?;
            style["description"] = json!(
                "The look every caption shares, as a title's style; only the fields that differ from the default are needed. Its content is replaced by each caption's text."
            );
            Some(json!({
                "type": "object",
                "properties": {
                    "path": path,
                    "captions": {
                        "type": "array",
                        "minItems": 1,
                        "description": "The captions, each shown from start to end.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "start": { "type": "number", "minimum": 0, "description": "When it appears, in seconds on the timeline." },
                                "end": { "type": "number", "minimum": 0, "description": "When it goes, in seconds on the timeline; after start." },
                                "text": { "type": "string", "description": "The words. A newline breaks the line." },
                            },
                            "required": ["start", "end", "text"],
                        },
                    },
                    "style": style,
                    "offsetY": {
                        "type": "number",
                        "default": 0.35,
                        "minimum": -3,
                        "maximum": 3,
                        "description": "Vertical placement as a fraction of frame height from the centre, positive down.",
                    },
                },
                "required": ["path", "captions"],
                "additionalProperties": false,
            }))
        }
    }
}

/// The sense tools' inputs. They wrap the embedded Python perception
/// layer rather than an API method, so no generated schema exists for them.
fn sense_schema(name: &str) -> Option<Value> {
    let string = |description: &str| json!({ "type": "string", "description": description });
    let count = |minimum: u64, maximum: u64, default: u64, description: &str| {
        json!({
            "type": "integer", "minimum": minimum, "maximum": maximum,
            "default": default, "description": description,
        })
    };
    let times = json!({
        "type": "array", "description": "Explicit sample times in seconds; overrides `samples`.",
        "items": { "type": "number", "minimum": 0 },
    });
    let props: Value = match name {
        "watch_render" => json!({
            "path": string("A rendered movie file to sample. Omit to sample the open project's true composited timeline."),
            "project": string("The project folder, used when `path` is absent."),
            "samples": count(2, 24, 12, "How many frames to sample."),
            "times": times,
            "duration": { "type": "number", "minimum": 0, "description": "Timeline duration in seconds; read from the project when absent." },
            "cols": count(1, 6, 4, "Contact-sheet columns."),
            "tileWidth": count(64, 480, 320, "Width of each sampled frame."),
            "tileHeight": count(48, 360, 180, "Height of each sampled frame."),
            "audio": { "type": "boolean", "default": true, "description": "Measure the soundtrack's energy envelope AND integrated LUFS / true peak (movie files only)." },
            "targetLufs": { "type": "number", "default": -14, "minimum": -70, "maximum": 0, "description": "Platform loudness target in LUFS; -14 short-form, -23 broadcast." },
            "targetTruePeakDbtp": { "type": "number", "default": -1, "minimum": -20, "maximum": 6 },
            "out": string("Also write the contact sheet PNG here."),
            "autoSetup": { "type": "boolean", "default": true, "description": "Bootstrap a static ffmpeg if no decoder is on this machine." },
        }),
        "stock_contact_sheet" => json!({
            "query": string("openvid_search words; the top `limit` results become candidates."),
            "ids": { "type": "array", "description": "Explicit OpenVid clip ids.", "items": { "type": "integer" } },
            "paths": { "type": "array", "description": "Already-local video files, for offline use.", "items": { "type": "string" } },
            "project": string("Project folder: loads its creative_brief for ranking."),
            "brief": { "type": "object", "description": "Creative brief used for semantic ranking." },
            "rank": { "type": "boolean", "default": false, "description": "Re-rank candidates against the brief with the local vision model." },
            "limit": count(1, 12, 8, "Maximum candidates to fetch and show."),
            "cols": count(1, 6, 4, "Contact-sheet columns."),
            "tileWidth": count(64, 480, 320, "Width of each candidate frame."),
            "minAesthetic": { "type": "number", "description": "Minimum OpenVid aesthetic score." },
            "minMotion": { "type": "number", "description": "Minimum OpenVid motion score." },
            "orientation": string("vertical, horizontal or square."),
            "source": string("pixabay, pexels, celebv or other."),
            "sort": string("relevance, aesthetic, motion, temporal, shortest, longest or surprise."),
            "out": string("Also write the contact sheet PNG here."),
            "aspect": { "type": "string", "enum": ["9:16", "16:9", "1:1", "4:5"], "default": "9:16", "description": "Target frame; used to turn each cropHint into ready-to-use clip_transform offsets (reframeHint)." },
            "autoSetup": { "type": "boolean", "default": true },
            "profile": string("mini, image or minicpm."),
        }),
        "craft_critique" => json!({
            "sheet": {
                "type": ["object", "string"],
                "description": "The JSON watch_render or stock_contact_sheet returned; its pngPath is the image to judge. A bare pngPath string - as motion_reel's sheet reply names it - is accepted too.",
            },
            "pngB64": string("A base64 PNG contact sheet, when `sheet` is not given."),
            "pngPath": string("A PNG file to judge, overriding the sheet's own pngPath."),
            "brief": { "type": "object", "description": "The creative brief; loaded from `project` when absent." },
            "project": string("Project folder holding creative-brief.json."),
            "autoSetup": { "type": "boolean", "default": false, "description": "Fetch and start the local vision model on first use." },
            "profile": string("mini, image or minicpm."),
        }),
        "suggest_cuts" => json!({
            "path": string("Media file whose audio carries the rhythm (music or voiceover)." ),
            "cuts": { "type": "integer", "minimum": 1, "maximum": 64, "default": 12 },
            "minGap": { "type": "number", "minimum": 0.05, "maximum": 10, "default": 0.35 },
            "bpm": { "type": "number", "minimum": 30, "maximum": 300, "description": "Optional BPM hint when the estimate is wrong." },
            "autoSetup": { "type": "boolean", "default": true, "description": "Bootstrap a pinned static ffmpeg into the app's data folder when no decoder is on the machine." },
        }),
        "measure_loudness" => json!({
            "path": string("The media file with an audio stream to measure."),
            "targetLufs": { "type": "number", "default": -14, "minimum": -70, "maximum": 0, "description": "The platform loudness target; -14 short-form, -23 broadcast EBU R128." },
            "targetTruePeakDbtp": { "type": "number", "default": -1, "minimum": -20, "maximum": 6 },
            "autoSetup": { "type": "boolean", "default": true, "description": "Bootstrap a pinned static ffmpeg into the app's data folder when no decoder is on the machine." },
        }),
        "track_subject" => json!({
            "path": string("The media file whose subject the crop should follow."),
            "samples": count(3, 24, 12, "How many frames to sample across the span."),
            "from": { "type": "number", "minimum": 0, "description": "Seconds into the file where the span starts; the start by default." },
            "to": { "type": "number", "minimum": 0, "description": "Seconds where the span ends; the file's end by default." },
            "times": times,
            "minConfidence": { "type": "number", "default": 0.18, "minimum": 0.05, "maximum": 0.9, "description": "How concentrated a frame's subject weight must be to count as found; below it the path holds the last position." },
            "aspect": { "type": "string", "enum": ["9:16", "16:9", "1:1", "4:5"], "default": "9:16", "description": "Target frame; each keyframe's position becomes the exact clip_transform offsets for it." },
            "autoSetup": { "type": "boolean", "default": true },
        }),
        "motion_overlay" => json!({
            "frames": string("The bake's frames/ directory holding 0001.png upwards."),
            "hash": string("A motion_bake hash; used with `project` to find the frames dir."),
            "project": string("Project folder; also the default home for the output file."),
            "plate": string("A video file to composite the motion over (the real B-roll)."),
            "out": string("Output .mp4 path; defaults to <project>/motion/overlay.mp4."),
            "fps": { "type": "number", "minimum": 1, "maximum": 120, "default": 30 },
            "at": { "type": "number", "minimum": 0, "description": "Seconds into the plate where the comp appears." },
            "width": { "type": "integer", "minimum": 2, "maximum": 8192 },
            "height": { "type": "integer", "minimum": 2, "maximum": 8192 },
            "crf": { "type": "integer", "minimum": 0, "maximum": 51, "default": 18 },
            "background": string("Colour when there is no plate, e.g. black or 101018."),
            "x": string("Overlay x expression; default (W-w)/2."),
            "y": string("Overlay y expression; default (H-h)/2."),
            "gpu": { "type": ["boolean", "string"], "enum": ["auto", "on", "off", true, false], "default": "auto", "description": "Whether the encode rides the machine's GPU: auto uses h264_vaapi where a VAAPI driver and render node exist, on asks for it and falls back to libx264 with a note when they do not, off encodes with libx264." },
            "autoSetup": { "type": "boolean", "default": true },
        }),
        "local_vlm" => json!({
            "action": { "type": "string", "enum": ["status", "setup", "configure", "providers", "clear"], "default": "status" },
            "profile": { "type": "string", "enum": ["mini", "image", "minicpm"], "default": "mini" },
            "provider": string("With action 'configure': groq, gemini, zai, openrouter or custom."),
            "apiKey": string("With action 'configure': the provider's API key; stored in the app's own vlm.json, never echoed back in full."),
            "endpoint": string("With action 'configure' and provider 'custom': an OpenAI-compatible base URL."),
            "model": string("Override the provider's default vision model id."),
        }),
        "emotion" => json!({
            "action": { "type": "string", "enum": ["extract", "brand", "gate", "verify", "template", "selftest"], "default": "extract" },
            "project": string("The project folder; its creative-brief.json supplies the brief (emotionalArc) and brandKit when not given directly."),
            "transcript": { "type": "object", "description": "A transcribe job's output: {segments: [{text, start, end, words: [{text,start,end}]}]}." },
            "lines": { "type": "array", "description": "Lines as [{text, start, end}] (or bare strings) when there is no transcript.", "items": { "type": ["object", "string"] } },
            "script": string("Raw narration text; split into sentences when no transcript or lines are given."),
            "brief": { "type": "object", "description": "The creative brief; its emotionalArc is checked against the states the lines actually visit." },
            "sections": { "type": "array", "description": "The plan's sections [{role, start, end}]; a line inside a hook/cta/outro section inherits that act.", "items": { "type": "object" } },
            "brand": { "type": "object", "description": "The brand guide (action 'template' returns one to copy): id, name, vibe, palette, type, grade, taboos, and optional motion, sfx, bgm, voice." },
            "apiKey": string("A HuggingFace token for the hosted emotion model; FUNCUT_EMOTION_API_KEY / HF_TOKEN otherwise. Without one the English-only lexicon engine answers."),
            "model": string("Override the default emotion model id (tabularisai/multilingual-emotion-classification)."),
            "segment": { "type": "object", "description": "With action 'gate': the beat a candidate is up for, {emotion: one of the 7, act: one of the 7}." },
            "candidates": { "type": "array", "description": "With action 'gate': openvid_search / stock_contact_sheet results carrying caption, keywords or query.", "items": { "type": "object" } },
            "expected": { "type": ["object", "array"], "description": "With action 'verify': the edit's words - a transcribe job's output or a [{text,start,end}] word list." },
            "actual": { "type": ["object", "array"], "description": "With action 'verify': the exported file's re-transcribed words." },
            "toleranceMs": { "type": "number", "default": 150, "minimum": 0, "description": "With action 'verify': how far a word may start from where the edit placed it." },
        }),
        "voice_signals" => json!({
            "action": { "type": "string", "enum": ["extract", "selftest"], "default": "extract" },
            "audio": string("The voiceover file (any format ffmpeg reads): the take the transcript timed."),
            "transcript": { "type": "object", "description": "A transcribe job's output: {segments: [{text, start, end, words: [{text,start,end}]}]} - the word timings the per-line signals need." },
            "lines": { "type": "array", "description": "Lines as [{text, start, end, words}] when there is no transcript.", "items": { "type": ["object", "string"] } },
            "script": string("Raw narration text when there is no transcript or audio: only the text signals (sentence types, motifs, fillers, entities, language) answer."),
            "language": string("The language hint the transcribe call used (ISO code); the script detection cross-checks it."),
            "emotion": { "type": "object", "description": "An emotion extract's output; its arc (shape, variety, score, warnings) rides along and feeds the cut-style hint." },
            "thresholdDb": { "type": "number", "default": 35, "description": "Silence threshold below full scale, as analysis.silence takes it (30 for a hot VO, 40 for a noisy room)." },
            "minSilenceMs": { "type": "integer", "default": 350, "minimum": 50, "description": "Silences shorter than this are breaths, not spans (120 catches every breath; 500 only real gaps)." },
            "maxSpeakers": { "type": "integer", "default": 4, "minimum": 1, "maximum": 8, "description": "The diarization's speaker ceiling." },
            "speakerThreshold": { "type": "number", "default": 0.6, "description": "The cosine similarity at which two voice-print windows may share a speaker; lower merges more eagerly." },
            "whisperx": { "type": ["string", "boolean"], "default": "auto", "description": "auto (default) uses WhisperX for word alignment when it is installed; off never does." },
        }),
        "creative_brief" => json!({
            "path": string("The project folder."),
            "action": { "type": "string", "enum": ["get", "set"], "default": "get" },
            "brief": { "type": "object", "description": "Fields to merge: audience, platform, goal, thesis, curiosityGap, payoff, emotionalArc, referenceStyle, tone, durationSeconds, brandKit, constraints, avoid." },
        }),
        _ => return None,
    };
    let required = match name {
        "creative_brief" => vec!["path"],
        _ => vec![],
    };
    Some(json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    }))
}

/// The full director schema used at runtime to validate a plan.
fn canonical_plan_schema() -> Option<Value> {
    serde_json::from_str(include_str!(
        "../../../../.claude/skills/funcut-director/templates/edit_plan.schema.json"
    ))
    .ok()
}

/// The compact MCP input schema. The full canonical schema is also checked
/// at runtime; this keeps a 7 KB plan template out of every client's tool list.
fn plan_validate_schema() -> Option<Value> {
    Some(json!({
        "type": "object",
        "properties": {
            "plan": {
                "type": "object",
                "description": "A director plan following the canonical edit_plan.schema.json, validated fully at runtime.",
                "required": ["aspect", "mode", "sections", "segments", "audio", "grade"],
                "properties": {
                    "aspect": { "type": "string", "enum": ["16:9", "9:16", "1:1", "4:5"] },
                    "mode": { "type": "string", "enum": ["reel", "longform"] },
                    "sections": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["role", "start", "end"],
                            "properties": {
                                "role": { "type": "string", "enum": ["hook", "intro", "body", "chapter", "rehook", "cta", "outro"] }
                            }
                        }
                    },
                    "grade": {
                        "type": "object",
                        "required": ["filter", "intensityKey", "intensity"]
                    },
                    "segments": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 500,
                        "items": {
                            "type": "object",
                            "required": ["start", "end", "visual"],
                            "properties": {
                                "start": { "type": "number", "minimum": 0 },
                                "end": { "type": "number", "minimum": 0 },
                                "visual": {
                                    "type": "object",
                                    "required": ["type"],
                                    "properties": {
                                        "type": { "type": "string", "enum": ["broll", "textCard", "titleCard", "existing"] }
                                    }
                                }
                            }
                        }
                    },
                    "audio": { "type": "object" },
                    "creativeBrief": {
                        "type": "object",
                        "description": "The WHY of the piece. Without it plan_validate returns creativeReady=false with blockers. audience, platform, goal, thesis, curiosityGap, payoff, emotionalArc (3+ beats), referenceStyle, tone, brandKit, constraints, avoid.",
                        "properties": {
                            "audience": { "type": "string" },
                            "platform": { "type": "string" },
                            "goal": { "type": "string" },
                            "thesis": { "type": "string" },
                            "curiosityGap": { "type": "string" },
                            "payoff": { "type": "string" },
                            "emotionalArc": { "oneOf": [
                                { "type": "array", "minItems": 3, "items": { "type": "string" } },
                                { "type": "string" }
                            ] },
                            "referenceStyle": { "type": "string" },
                            "tone": { "type": "string" },
                            "durationSeconds": { "type": "number" },
                            "brandKit": {},
                            "constraints": {},
                            "avoid": {}
                        }
                    },
                    "signalLog": {
                        "type": "array",
                        "description": "The decision log: for each cut, caption, graphic, BGM and transition choice, which voice_signals signals drove it. An entry is {at: seconds, decision: cut|caption|graphic|bgm|transition, signals: [names from the profile's decisionPolicy], note?}. A plan with a voiceover and no signalLog is not creativeReady.",
                        "items": {
                            "type": "object",
                            "required": ["at", "decision", "signals"],
                            "properties": {
                                "at": { "type": "number", "minimum": 0 },
                                "decision": { "type": "string", "enum": ["cut", "caption", "graphic", "bgm", "transition"] },
                                "signals": { "type": "array", "minItems": 1, "items": { "type": "string" } },
                                "note": { "type": "string" }
                            },
                            "additionalProperties": false
                        }
                    }
                }
            }
        },
        "required": ["plan"],
        "additionalProperties": false,
    }))
}

/// The motion tools' inputs, written by hand: they wrap `concat-motion`
/// rather than an API method, so there is no generated schema to read.
/// Each names what it takes in the same words its description does.
fn motion_schema(name: &str) -> Option<Value> {
    // The four fields every comp-naming tool takes, merged into each
    // tool's own properties by hand - json! has no spread, and these
    // schemas are written, not generated.
    let mut shared = Map::new();
    shared.insert(
        "preset".to_owned(),
        json!({
            "type": "string",
            "description": "A preset id from motion_presets, e.g. funcut.lower-third.glass."
        }),
    );
    shared.insert(
        "comp".to_owned(),
        json!({
            "type": "string",
            "description": "A comp file under the project's comps/, e.g. 'abc123.json'."
        }),
    );
    shared.insert(
        "project".to_owned(),
        json!({
            "type": "string",
            "description": "The project folder - where comps/, cache/ and motion/ live."
        }),
    );
    shared.insert(
        "ops".to_owned(),
        json!({
            "type": "array",
            "description": "Authoring operations applied before anything else: set-slot, \
             set-text, set-words, set-markup, move, retime, set-track, set-track-points, \
             set-noise-track (seeded organic drift: seed, amount, rate), fade-in, \
             slide-to, pop, add-group, add-rect, add-text, set-blend, set-opacity, \
             set-duration, set-background, delete.",
            "items": { "type": "object" }
        }),
    );
    let with = |mut properties: Map<String, Value>, extra: &[(&str, Value)]| {
        for (key, value) in extra {
            properties.insert((*key).to_owned(), value.clone());
        }
        properties
    };
    let object = |properties: Map<String, Value>, required: &[&str]| {
        json!({
            "type": "object",
            "required": required,
            "properties": properties,
            "additionalProperties": false,
        })
    };
    let schema = match name {
        "motion_presets" => object(Map::new(), &[]),
        "motion_inspect" => object(shared, &[]),
        "scene_analysis" => object(shared, &[]),
        "motion_frame" => object(
            with(
                shared,
                &[
                    (
                        "at",
                        json!({
                            "type": "number", "minimum": 0,
                            "description": "Seconds into the comp."
                        }),
                    ),
                    (
                        "width",
                        json!({
                            "type": "integer", "minimum": 1, "maximum": 8192,
                            "description": "Draw at this width; the comp's own when absent."
                        }),
                    ),
                ],
            ),
            &["project", "at"],
        ),
        "motion_preview" => object(
            with(
                shared,
                &[
                    (
                        "frames",
                        json!({
                            "type": "integer", "minimum": 1, "maximum": 12, "default": 6,
                            "description": "How many stills spread across the comp's duration - six is the glance, twelve the strip."
                        }),
                    ),
                    (
                        "width",
                        json!({
                            "type": "integer", "minimum": 16, "maximum": 640, "default": 480,
                            "description": "The width each sample draws at - a preview, not a bake."
                        }),
                    ),
                ],
            ),
            &["project"],
        ),
        "motion_bake" => object(
            with(
                shared,
                &[
                    (
                        "width",
                        json!({ "type": "integer", "minimum": 1, "maximum": 4096, "description": "Clamped to 4096; the project's own frame when absent." }),
                    ),
                    (
                        "height",
                        json!({ "type": "integer", "minimum": 1, "maximum": 4096, "description": "Clamped to 4096; the project's own frame when absent." }),
                    ),
                    (
                        "fps",
                        json!({ "type": "number", "minimum": 1, "maximum": 240 }),
                    ),
                    (
                        "crf",
                        json!({ "type": "integer", "minimum": 0, "maximum": 51, "default": 18 }),
                    ),
                    (
                        "encode",
                        json!({
                            "type": "boolean",
                            "description": "Also write one video file under motion/, ready \
                             for media_import."
                        }),
                    ),
                    (
                        "name",
                        json!({
                            "type": "string",
                            "description": "The encoded file's name; the comp's hash is \
                             prepended when missing, because export finds a motion clip's \
                             bake by that hash. .webm is VP9, .mov ProRes 4444, otherwise \
                             H.264."
                        }),
                    ),
                ],
            ),
            &["project"],
        ),
        "captions_animate" => object(
            with(
                shared,
                &[
                    (
                        "words",
                        json!({
                            "type": "array", "minItems": 1,
                            "description": "The transcript as [text, start, end] triples, \
                             seconds from the voiceover's start.",
                            "items": {}
                        }),
                    ),
                    (
                        "wordsPerLine",
                        json!({ "type": "integer", "minimum": 1, "maximum": 12, "default": 5 }),
                    ),
                    (
                        "maxLineSeconds",
                        json!({ "type": "number", "minimum": 0.5, "maximum": 12, "default": 3.6 }),
                    ),
                    (
                        "gapSeconds",
                        json!({ "type": "number", "minimum": 0, "maximum": 5, "default": 0.6 }),
                    ),
                    (
                        "leadSeconds",
                        json!({ "type": "number", "minimum": 0, "maximum": 2 }),
                    ),
                    (
                        "tailSeconds",
                        json!({ "type": "number", "minimum": 0, "maximum": 5 }),
                    ),
                    ("width", json!({ "type": "integer", "default": 1080 })),
                    ("height", json!({ "type": "integer", "default": 1920 })),
                    ("fps", json!({ "type": "number", "default": 30 })),
                    (
                        "plate",
                        json!({
                            "type": "string",
                            "description": "A still under the project the captions render over."
                        }),
                    ),
                    (
                        "curtain",
                        json!({
                            "type": "boolean", "default": true,
                            "description": "Keep the curtain dip between caption lines; false gives hard cuts, which read better when the reel sits over a voiceover."
                        }),
                    ),
                    (
                        "kinetic",
                        json!({
                            "type": ["boolean", "object"], "default": false,
                            "description": "The kinetic typography hand: true takes the word-staggered default (80 ms, slide-up, the settle curve); an object spells it out - stagger (word|char), delay seconds, entrance (blur-in|slide-up|scale-in), seconds, curve, weight {from, to} for a static weight ramp across the words (the engine cannot animate a weight: a static font-weight per word is the one it draws). The caption becomes markup whose per-word spans appear, light and settle on the recogniser's own word times.",
                            "properties": {
                                "stagger": { "type": "string", "enum": ["word", "char"], "default": "word" },
                                "delay": { "type": "number", "minimum": 0, "default": 0.08 },
                                "entrance": { "type": "string", "enum": ["blur-in", "slide-up", "scale-in"], "default": "slide-up" },
                                "seconds": { "type": "number", "minimum": 0, "default": 0.5 },
                                "weight": {
                                    "type": "object",
                                    "description": "A static weight ramp across the units, 100..900: the first at from, the last at to, interpolated between - drawn per word as a static font-weight, the crescendo the engine renders.",
                                    "properties": {
                                        "from": { "type": "integer", "minimum": 100, "maximum": 900 },
                                        "to": { "type": "integer", "minimum": 100, "maximum": 900 }
                                    },
                                    "required": ["from", "to"]
                                }
                            }
                        }),
                    ),
                    ("name", json!({ "type": "string" })),
                ],
            ),
            &["project", "words"],
        ),
        "motion_reel" => {
            // The storyboard's own shape, written once here so the tool
            // describes what it actually eats: scenes with layers, assets
            // by id, audio beds and a curtain.
            let layer = json!({
                "type": "object",
                "description": "A layer of the scene: an asset or a named scene, placed with this layer's own timing, drift, blend and fades.",
                "properties": {
                    "asset": { "type": "string", "description": "An asset id from the storyboard's assets. Exactly one of asset and scene." },
                    "scene": { "type": "string", "description": "A named scene of this storyboard (a scene with an id), placed as a nested composition. Exactly one of asset and scene; a scene cannot be built from itself." },
                    "zoom": { "type": "number", "description": "Growth over the span as a scale delta: 0.04 is four percent." },
                    "pan": { "type": "array", "minItems": 2, "maxItems": 2, "items": { "type": "number" },
                             "description": "Drift over the span as [x, y] fractions of the frame; a cover layer grows by its own travel so the edge never shows." },
                    "offset": { "type": "array", "minItems": 2, "maxItems": 2, "items": { "type": "number" },
                                "description": "A static offset from the centre, as [x, y] fractions of the frame." },
                    "scale": { "type": "number", "description": "Base scale multiplier; under 1 shows the frame's edges on purpose." },
                    "fit": { "type": "string", "enum": ["cover", "contain", "fill", "none"], "default": "cover" },
                    "blend": { "type": "string", "enum": ["normal", "multiply", "screen", "overlay", "darken", "lighten", "difference", "soft-light", "add"], "default": "normal",
                               "description": "screen/add turn a soft radial picture into a light leak; multiply shades one into a vignette." },
                    "opacity": { "type": "number", "minimum": 0, "maximum": 1 },
                    "fade_in": { "type": "number", "minimum": 0, "description": "Fade up from transparent, seconds. fadeIn also parses." },
                    "fade_out": { "type": "number", "minimum": 0, "description": "Fade down to transparent, seconds." },
                    "start": { "type": "number", "minimum": 0, "description": "When it starts, from the scene's own start." },
                    "hold": { "type": "number", "minimum": 0, "description": "How long it holds; omitted runs to the scene's end." },
                    "parallax": { "type": "number", "exclusiveMinimum": 0,
                                  "description": "How fast it drifts, as a multiple of its own zoom and pan (or of the classic plate's 4% growth / 3% slide, when it carries none): 0.5 half speed, 2.0 twice - nearer layers move faster, which is depth." },
                    "above": { "type": "boolean", "default": false,
                               "description": "Paint over the comp - grain, vignettes, light leaks, foreground cards - rather than under it." }
                }
            });
            let scene = json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The scene's own name, so another scene's layer can place it as a nested composition." },
                    "preset": { "type": "string", "description": "A preset id from motion_presets." },
                    "comp": { "type": "string", "description": "A comp file under the project's comps/." },
                    "at": { "type": "number", "description": "Where it starts; omitted means after the scene before it." },
                    "hold": { "type": "number", "description": "How long it holds; omitted means the comp's own length." },
                    "plate": { "type": "string", "description": "The short form: one still under the comp." },
                    "zoom": { "type": "number", "description": "The short form's plate drift." },
                    "layers": { "type": "array", "items": layer,
                                "description": "The scene's layers, bottom first: plates under the comp, above:true overlays over it." },
                    "ops": { "type": "array", "items": {}, "description": "Authoring operations applied to the comp before it is placed." }
                }
            });
            let storyboard = json!({
                "type": "object",
                "required": ["scenes"],
                "properties": {
                    "width": { "type": "integer", "default": 1920 },
                    "height": { "type": "integer", "default": 1080 },
                    "fps": { "type": "number", "default": 30 },
                    "assets": { "type": "object",
                                "description": "Assets by id; each value is a path under the project.",
                                "additionalProperties": { "type": "string" } },
                    "scenes": { "type": "array", "minItems": 1, "items": scene },
                    "audio": { "type": "array", "items": { "type": "object", "required": ["asset"], "properties": {
                        "asset": { "type": "string" }, "gain_db": { "type": "number", "default": 0 },
                        "fade_in": { "type": "number" }, "fade_out": { "type": "number" } } } },
                    "curtain": { "type": "object", "properties": {
                        "color": { "type": "string", "default": "#05060aff" },
                        "open": { "type": "number", "default": 0.6 }, "close": { "type": "number", "default": 0.8 },
                        "dip": { "type": "number", "default": 0.2 }, "depth": { "type": "number", "default": 1.0 } } }
                }
            });
            let mut properties = Map::new();
            properties.insert(
                "project".to_owned(),
                json!({
                    "type": "string",
                    "description": "The project folder - where the assets live, comps/ is read from and motion/ is written to."
                }),
            );
            properties.insert("storyboard".to_owned(), storyboard);
            properties.insert(
                "name".to_owned(),
                json!({
                    "type": "string",
                    "description": "The rendered file's name under motion/; .mp4 H.264, .webm VP9, .mov ProRes 4444."
                }),
            );
            properties.insert(
                "frame".to_owned(),
                json!({
                    "type": "number", "minimum": 0,
                    "description": "Render one still at this many seconds and return it as an inline image, instead of the whole reel."
                }),
            );
            properties.insert(
                "plan".to_owned(),
                json!({
                    "type": "boolean", "default": false,
                    "description": "Lay the reel out and return the timeline without rendering."
                }),
            );
            properties.insert(
                "crf".to_owned(),
                json!({ "type": "integer", "minimum": 0, "maximum": 51, "default": 18 }),
            );
            properties.insert(
                "sheet".to_owned(),
                json!({
                    "type": "boolean", "default": false,
                    "description": "Render four stills of the reel (a quarter in, half, three quarters, near the end), tiled into one contact sheet, returned as an inline image and written under cache/sense/ for craft_critique (its `sheet` argument takes the pngPath). The look-before-you-commit of the creative loop, over the reel's own output: score it against the brief, and anything under 7 is a revision."
                }),
            );
            properties.insert(
                "genevaTimeoutSecs".to_owned(),
                json!({
                    "type": "number", "minimum": 5, "maximum": 1140, "default": 300,
                    "description": "No call to the render sidecar may run past this, in \
                     seconds; a call that does is killed and named, not hung. The default \
                     300 covers the long reels; the ceiling 1140 sits under the tool's own \
                     1200 s outer bound, so a call at the maximum is named by this deadline \
                     and not cancelled by the net. The quick calls (plan, one frame) are \
                     capped at their own 60 s when this is looser than that. A sidecar \
                     that fails three times in a minute is refused with the fallback \
                     story instead of being asked a fourth."
                }),
            );
            object(properties, &["project", "storyboard"])
        }
        "motion_probe" => {
            let mut properties = Map::new();
            properties.insert(
                "path".to_owned(),
                json!({
                    "type": "string",
                    "description": "The media file to inspect: absolute, or a name under the project when `project` is given. Any picture, video or sound the engine reads."
                }),
            );
            properties.insert(
                "project".to_owned(),
                json!({
                    "type": "string",
                    "description": "The project folder a relative `path` resolves against."
                }),
            );
            object(properties, &["path"])
        }
        "geneva_explain" => {
            let mut properties = Map::new();
            properties.insert(
                "code".to_owned(),
                json!({
                    "type": "string",
                    "description": "The diagnostic code (E302) or a refusal sentence that carries one; its first code-shaped token is the code asked about."
                }),
            );
            object(properties, &["code"])
        }
        "motion_farm_progress" => {
            let mut properties = Map::new();
            properties.insert(
                "farm".to_owned(),
                json!({
                    "type": "string",
                    "description": "The farm directory a farmed render writes to: absolute, or relative to `project`."
                }),
            );
            properties.insert(
                "project".to_owned(),
                json!({
                    "type": "string",
                    "description": "The project folder a relative `farm` resolves against."
                }),
            );
            object(properties, &["farm"])
        }
        "motion_farm_cloud" => {
            let mut properties = Map::new();
            properties.insert(
                "action".to_owned(),
                json!({
                    "type": "string",
                    "enum": ["push", "pull", "status", "work"],
                    "description": "push uploads the farm (and `staged`) to object storage; pull downloads them back; status says what differs; work pulls, renders one missing part through the sidecar, and pushes it back."
                }),
            );
            properties.insert(
                "farm".to_owned(),
                json!({
                    "type": "string",
                    "description": "The farm directory: absolute, or relative to `project`."
                }),
            );
            properties.insert(
                "staged".to_owned(),
                json!({
                    "type": "string",
                    "description": "The staged render directory the timeline lives in (the render's --cache), uploaded under `staged/` in the prefix; `work` needs it."
                }),
            );
            properties.insert(
                "project".to_owned(),
                json!({
                    "type": "string",
                    "description": "The project folder a relative `farm` resolves against."
                }),
            );
            properties.insert(
                "parts".to_owned(),
                json!({
                    "type": "integer", "minimum": 1, "maximum": 64,
                    "description": "For `work`: the number of parts the orchestrator asked for, when the farm's progress.jsonl does not say it."
                }),
            );
            properties.insert(
                "part".to_owned(),
                json!({
                    "type": "integer", "minimum": 0,
                    "description": "For `work`: render this part rather than the first missing one."
                }),
            );
            properties.insert(
                "geneva".to_owned(),
                json!({
                    "type": "string",
                    "description": "The sidecar binary for `work` (CONCAT_GENEVA_BIN and PATH are the other doors)."
                }),
            );
            properties.insert(
                "prefix".to_owned(),
                json!({
                    "type": "string",
                    "description": "The key prefix in the bucket; the farm's own name under farms/ when absent. The bucket, the endpoint and the credentials come from the operator's environment, never from here."
                }),
            );
            properties.insert(
                "timeoutSeconds".to_owned(),
                json!({
                    "type": "number", "minimum": 5, "maximum": 3600,
                    "description": "For `work`: the sidecar's own per-part ceiling."
                }),
            );
            object(properties, &["action", "farm"])
        }
        "scene_timeline" => {
            // The storyboard's own shape, the same document
            // motion_reel takes.
            let storyboard = {
                let layer = json!({
                    "type": "object",
                    "properties": {
                        "asset": { "type": "string" },
                        "scene": { "type": "string" },
                        "zoom": { "type": "number" },
                        "pan": { "type": "array", "minItems": 2, "maxItems": 2, "items": { "type": "number" } },
                        "offset": { "type": "array", "minItems": 2, "maxItems": 2, "items": { "type": "number" } },
                        "scale": { "type": "number" },
                        "fit": { "type": "string", "enum": ["cover", "contain", "fill", "none"] },
                        "blend": { "type": "string" },
                        "opacity": { "type": "number", "minimum": 0, "maximum": 1 },
                        "fadeIn": { "type": "number" },
                        "fadeOut": { "type": "number" },
                        "fadeCurve": { "type": "string",
                                       "description": "A name from the easing library (linear, ease, ease-in, ease-out, ease-in-out, emphasis, drift, snap, accelerate, anticipate, spring, spring-pop, typewriter, bounce) or a full curve object." },
                        "start": { "type": "number" },
                        "hold": { "type": "number" },
                        "parallax": { "type": "number" },
                        "wobble": {
                            "type": "object",
                            "description": "Organic drift on a seeded noise field, folded onto the layer's drift: seed (the whole of the randomness - deterministic), amount (reach, fractions of the frame), rate (keys a second).",
                            "properties": {
                                "seed": { "type": "integer" },
                                "amount": { "type": "number", "minimum": 0, "maximum": 0.5 },
                                "rate": { "type": "number", "minimum": 1, "maximum": 30 }
                            },
                            "required": ["seed"]
                        },
                        "above": { "type": "boolean" }
                    }
                });
                let scene = json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "preset": { "type": "string" },
                        "comp": { "type": "string" },
                        "at": { "type": "number" },
                        "hold": { "type": "number" },
                        "plate": { "type": "string" },
                        "zoom": { "type": "number" },
                        "layers": { "type": "array", "items": layer },
                        "ops": { "type": "array", "items": {} },
                        "grade": { "type": "object",
                                   "description": "The scene's colour grade: one of the emotion read's temperatures (hot, sickly, cold, warm, balanced, cool, split) with its own tuning, `amount` 0-1 scaling the cast, optional saturate/contrast/brightness overrides, a `hue` turn in degrees (-360..360) that rotates the whole palette, and a brand `tint` as #rrggbb worn instead of the stock cast. Baked in under the comp's own words as backdrop-filter (saturate, hue-rotate, contrast, brightness) plus a soft-light cast - the LUT approximation a comp carries." },
                        "look": { "type": "object",
                                  "description": "The texture the scene wears over its picture: grain (amount 0-1, size 2-8 px, seed) - a seeded field of motes over everything, ink included; glass (blur 0-24, saturate 0.5-2) - a frost under the comp's own words, over the plate; chromatic (amount 0-1) - the warm-left cool-right edge fringe over everything; vignette (amount 0-1) - the corners' dark fall-off from all four edges over everything; letterbox (ratio 1-3) - the cinema bars holding the frame to a wider ratio, 2.39 the scope screen, a ratio under the comp's own the pillarbox." },
                        "style": { "type": "string", "enum": ["calm", "punchy", "editorial"],
                                   "description": "The personality the scene wears over its layout: the weight the type carries, the tracking, the easing hand, the tempo - and the colour when no grade is named. One layout, three moods." },
                        "transition": { "type": "object",
                                         "description": "The choreographed cut INTO this scene: whip-pan (direction, seconds, blur), zoom-through, inverse-zoom, rack-focus (radius), velocity-match or stagger-exit (step)." },
                        "exit": { "type": "object",
                                  "description": "This scene's own way OUT, when the next scene names no transition of its own." },
                        "act": { "type": "string", "enum": ["hook", "setup", "tension", "turn", "reveal", "proof", "cta"],
                                 "description": "The act the scene plays in, as the emotion read names it. A pulse that names no curve of its own gets the act's - calm for the hook, wound for the tension, the overshoot for the reveal; a pulse that names a curve keeps it." },
                        "pulse": { "type": "object",
                                   "description": "The audio-driven keyframes: beats (measured times), driven (scale|opacity|position), amount, release, curve (a name from the easing library - bounce, spring-pop, typewriter - or a full curve object), nodes." }
                    }
                });
                json!({
                    "type": "object",
                    "required": ["scenes"],
                    "properties": {
                        "width": { "type": "integer", "default": 1920 },
                        "height": { "type": "integer", "default": 1080 },
                        "fps": { "type": "number", "default": 30 },
                        "assets": { "type": "object", "additionalProperties": { "type": "string" } },
                        "scenes": { "type": "array", "minItems": 1, "items": scene },
                        "audio": { "type": "array", "items": { "type": "object", "required": ["asset"], "properties": {
                            "asset": { "type": "string" }, "gain_db": { "type": "number" },
                            "fade_in": { "type": "number" }, "fade_out": { "type": "number" } } } },
                        "curtain": { "type": "object" }
                    }
                })
            };
            let mut properties = Map::new();
            properties.insert("storyboard".to_owned(), storyboard);
            properties.insert(
                "project".to_owned(),
                json!({
                    "type": "string",
                    "description": "The project folder the board's assets resolve against."
                }),
            );
            object(properties, &["storyboard"])
        }
        _ => return None,
    };
    Some(schema)
}

/// The dB-aware audio-level tool's inputs. Its values are converted before
/// they become the project model's linear-gain numbers.
fn audio_levels_schema(schemas: &Schemas) -> Option<Value> {
    let path = schemas
        .method("project.get")?
        .pointer("/properties/path")?
        .clone();
    let clip_id = schemas
        .command("updateClip")?
        .pointer("/properties/clipId")?
        .clone();
    let ease = schemas
        .command("setClipKey")?
        .pointer("/properties/ease")?
        .clone();
    let decibels = json!({
        "type": "number",
        "minimum": -60,
        "maximum": 24,
        "description": "Gain in dB relative to source unity: 0 dB is unchanged, −60 dB is the editor's silence floor, negative values attenuate, and positive values boost.",
    });
    Some(json!({
        "type": "object",
        "properties": {
            "path": path,
            "clipId": clip_id,
            "baseDb": {
                "type": "number",
                "minimum": -60,
                "maximum": 24,
                "description": "The clip's base gain in dB relative to source unity; this is the value the engine uses outside keyed regions.",
            },
            "replaceAutomation": {
                "type": "boolean",
                "default": false,
                "description": "Clear existing volume keys before applying the supplied keys.",
            },
            "keyframes": {
                "type": "array",
                "maxItems": 500,
                "description": "Optional absolute dB gain targets relative to source unity at clip-relative positions; keyframe values are converted to linear multipliers.",
                "items": {
                    "type": "object",
                    "properties": {
                        "at": { "type": "number", "minimum": 0, "maximum": 1 },
                        "gainDb": decibels,
                        "ease": ease,
                    },
                    "required": ["at", "gainDb"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["path", "clipId", "baseDb"],
        "additionalProperties": false,
    }))
}

/// A request from a method's arguments.
fn request(method: &str, mut arguments: Value) -> Result<Request, ApiError> {
    let Some(fields) = arguments.as_object_mut() else {
        return Err(ApiError::invalid("the arguments are an object"));
    };
    fields.insert("method".to_owned(), json!(method));
    serde_json::from_value(arguments)
        .map_err(|error| ApiError::invalid(format!("invalid arguments: {error}")))
}

/// `edit.apply` of one command, from the command's arguments and the
/// project's `path` beside them.
fn edit(op: &str, mut arguments: Value) -> Result<Request, ApiError> {
    let Some(fields) = arguments.as_object_mut() else {
        return Err(ApiError::invalid("the arguments are an object"));
    };
    let path = fields
        .remove("path")
        .ok_or_else(|| ApiError::invalid("invalid arguments: missing field `path`"))?;
    fields.insert("op".to_owned(), json!(op));
    request("edit.apply", json!({ "path": path, "command": arguments }))
}

fn run(tool: &Tool, arguments: Value, context: Context<'_>) -> Result<Outcome, ApiError> {
    let backend = context.backend;
    match tool.action {
        Action::Method { method, reply } if method == "openvid.search" => {
            // The one search this layer adds a step to: BM25's page, then
            // the CLIP rerank in sense. The arguments are remembered
            // before `request` stamps the method into them, because the
            // rerank reads the caller's query, sort and offset back out.
            let remembered = arguments.clone();
            let result = backend.call(request(method, arguments)?)?;
            let shown = match reply {
                Reply::Raw => crate::sense::semantic_rerank(&remembered, result, backend),
                Reply::Summary => summary::view(&result),
            };
            Ok(Outcome::json(shown))
        }
        Action::Method { method, reply } => {
            let result = backend.call(request(method, arguments)?)?;
            let shown = match reply {
                Reply::Raw => result,
                Reply::Summary => summary::view(&result),
            };
            // A reply that is a bare array - the projects, the clips, the
            // stickers, the packages, the templates - has no object for
            // structuredContent to be, and the tools that return one were
            // the only ones a structured client read nothing from. Each
            // lands under the name a person would give its entries.
            Ok(Outcome::json(match shown {
                Value::Array(items) => {
                    let key = match method {
                        "project.list" => "projects",
                        "openvid.get" | "openvid.fetch" => "clips",
                        "magick.fetch" => "stickers",
                        "catalogue.list" => "packages",
                        "template.list" => "templates",
                        _ => "result",
                    };
                    json!({ key: items })
                }
                whole => whole,
            }))
        }
        Action::Command { op } => {
            let result = backend.call(edit(op, arguments)?)?;
            Ok(Outcome::json(summary::view(&result)))
        }
        Action::Version => {
            let mut result = backend.call(request("version", arguments)?)?;
            result["mcp"] = backend.describe();
            Ok(Outcome::json(result))
        }
        Action::TimelineRead => {
            let result = backend.call(request("project.get", arguments)?)?;
            Ok(Outcome::json(summary::view(&result)))
        }
        Action::Captions => {
            let result = backend.call(captions(arguments)?)?;
            Ok(Outcome::json(summary::view(&result)))
        }
        Action::AudioLevelsDb => {
            let result = backend.call(audio_levels_db(arguments)?)?;
            Ok(Outcome::json(summary::view(&result)))
        }
        Action::PlanValidate => Ok(Outcome::json(validate_plan_tool(arguments)?)),
        Action::ExportWait => export_wait(arguments, backend, context.cancelled, context.progress),
        Action::PreviewFrame => preview(arguments, backend),
        Action::MotionPresets => crate::motion::presets_list(),
        Action::MotionInspect => crate::motion::inspect(arguments),
        Action::MotionFrame => crate::motion::frame(arguments, backend),
        Action::MotionPreview => crate::motion::preview(arguments, backend),
        Action::MotionBake => crate::motion::bake(arguments, backend),
        Action::CaptionsAnimate => crate::motion::captions_animate(arguments, backend),
        Action::MotionReel => crate::motion::reel(arguments, backend),
        Action::MediaProbe => crate::motion::media_probe(arguments),
        Action::SceneTimeline => crate::motion::scene_timeline(arguments, backend),
        Action::GenevaExplain => crate::motion::geneva_explain(arguments),
        Action::MotionFarmProgress => crate::motion::farm_progress(arguments),
        Action::MotionFarmCloud => crate::motion::farm_cloud(arguments),
        Action::SceneAnalysis => crate::motion::scene_analysis(arguments),
        Action::WatchRender => crate::sense::watch_render(arguments, backend),
        Action::StockSheet => crate::sense::stock_contact_sheet(arguments, backend),
        Action::CraftCritique => crate::sense::craft_critique(arguments, backend),
        Action::LocalVlm => crate::sense::local_vlm(arguments),
        Action::Emotion => crate::sense::emotion(arguments),
        Action::VoiceSignals => crate::sense::voice_signals(arguments),
        Action::Sense(name) => crate::sense::sense_family(name, arguments),
        Action::SenseFamily(family) => crate::sense::sense_family_action(family, arguments),
        Action::CreativeBrief => crate::sense::creative_brief(arguments, backend),
        Action::MotionOverlay => crate::sense::motion_overlay(arguments, backend),
        Action::MeasureLoudness => crate::sense::measure_loudness(arguments),
        Action::SuggestCuts => crate::sense::suggest_cuts(arguments),
        Action::TrackSubject => crate::sense::track_subject(arguments),
        Action::CollisionCheck => {
            let result = backend.call(request("project.get", arguments)?)?;
            Ok(collision_report(summary::view(&result)))
        }
        Action::OpenvidSearchBatch => openvid_search_batch(arguments, backend),
        Action::OpenvidRankForSlot => openvid_rank_for_slot(arguments, backend),
        Action::FootageContinuity => footage_continuity(arguments),
        Action::Mask => {
            let result = backend.call(mask(arguments)?)?;
            Ok(Outcome::json(summary::view(&result)))
        }
    }
}

/// `clip_mask`: a clip patch with only its mask.
fn mask(mut arguments: Value) -> Result<Request, ApiError> {
    let invalid = |why: &str| ApiError::invalid(format!("invalid arguments: {why}"));
    let fields = arguments
        .as_object_mut()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let path = fields
        .remove("path")
        .ok_or_else(|| invalid("missing field `path`"))?;
    let clip = fields
        .remove("clipId")
        .ok_or_else(|| invalid("missing field `clipId`"))?;
    let mask = fields
        .remove("mask")
        .ok_or_else(|| invalid("missing field `mask`"))?;
    if let Some(unknown) = fields.keys().next() {
        return Err(invalid(&format!("unknown field `{unknown}`")));
    }
    edit(
        "updateClip",
        json!({ "path": path, "clipId": clip, "patch": { "mask": mask } }),
    )
}

/// `captions_add`: one batch of titles.
fn captions(mut arguments: Value) -> Result<Request, ApiError> {
    let invalid = |why: &str| ApiError::invalid(format!("invalid arguments: {why}"));
    let fields = arguments
        .as_object_mut()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let path = fields
        .remove("path")
        .ok_or_else(|| invalid("missing field `path`"))?;
    let captions = match fields.remove("captions") {
        Some(Value::Array(captions)) if !captions.is_empty() => captions,
        _ => return Err(invalid("`captions` is a list of {start, end, text}")),
    };
    let offset = match fields.remove("offsetY") {
        None | Some(Value::Null) => 0.35,
        Some(value) => value
            .as_f64()
            .ok_or_else(|| invalid("`offsetY` is a number"))?,
    };
    let style = match fields.remove("style") {
        None | Some(Value::Null) => json!({
            "fontFamily": "Hanken Grotesk",
            "fontSize": 0.04,
            "fontWeight": 600,
        }),
        // Some words' looks are about one title's words, not every
        // caption's, so a style's runs do not carry over.
        Some(Value::Object(mut style)) => {
            style.remove("runs");
            Value::Object(style)
        }
        Some(_) => return Err(invalid("`style` is an object")),
    };
    if let Some(unknown) = fields.keys().next() {
        return Err(invalid(&format!("unknown field `{unknown}`")));
    }
    let mut commands = Vec::with_capacity(captions.len());
    for (index, caption) in captions.iter().enumerate() {
        let number = |key: &str| caption.get(key).and_then(Value::as_f64);
        let (Some(start), Some(end), Some(text)) = (
            number("start"),
            number("end"),
            caption.get("text").and_then(Value::as_str),
        ) else {
            return Err(invalid(&format!(
                "caption {index} needs a start, an end and a text"
            )));
        };
        if !(start >= 0.0 && end > start) {
            return Err(invalid(&format!(
                "caption {index} ends at {end} s, not after its start at {start} s"
            )));
        }
        let mut style = style.clone();
        style["content"] = json!(text);
        commands.push(json!({
            "op": "addTextClip",
            "trackId": null,
            "above": true,
            "start": start,
            "duration": end - start,
            "style": style,
            "offsetY": offset,
        }));
    }
    request(
        "edit.apply",
        json!({ "path": path, "command": { "op": "batch", "commands": commands } }),
    )
}

/// `clip_audio_levels_db`: converts absolute dB targets to the clip model's
/// linear base gain and absolute volume-key values in one undoable batch.
fn audio_levels_db(mut arguments: Value) -> Result<Request, ApiError> {
    let invalid = |why: &str| ApiError::invalid(format!("invalid arguments: {why}"));
    let fields = arguments
        .as_object_mut()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let path = fields
        .remove("path")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| invalid("`path` is required and must be a non-empty string"))?;
    let clip_id = fields
        .remove("clipId")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| invalid("`clipId` is required and must be a non-empty string"))?;
    let base_db = fields
        .remove("baseDb")
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && (-60.0..=24.0).contains(value))
        .ok_or_else(|| invalid("`baseDb` must be between -60 and +24 dB"))?;
    let replace_automation = match fields.remove("replaceAutomation") {
        None => false,
        Some(Value::Bool(value)) => value,
        Some(_) => return Err(invalid("`replaceAutomation` must be a boolean")),
    };
    let keyframes = match fields.remove("keyframes") {
        None => Vec::new(),
        Some(Value::Array(values)) if values.len() <= 500 => values,
        Some(Value::Array(_)) => {
            return Err(invalid("`keyframes` may contain at most 500 entries"));
        }
        Some(_) => return Err(invalid("`keyframes` must be a list")),
    };
    if let Some(unknown) = fields.keys().next() {
        return Err(invalid(&format!("unknown field `{unknown}`")));
    }

    let base_gain = gain_from_db(base_db);

    let mut commands = Vec::with_capacity(keyframes.len() + 2);
    if replace_automation {
        commands.push(json!({
            "op": "clearClipKeys",
            "clipId": clip_id,
            "property": "volume",
        }));
    }
    commands.push(json!({
        "op": "updateClip",
        "clipId": clip_id,
        "patch": { "volume": base_gain },
    }));

    for (index, keyframe) in keyframes.into_iter().enumerate() {
        let Some(values) = keyframe.as_object() else {
            return Err(invalid(&format!("keyframes[{index}] must be an object")));
        };
        let mut values = values.clone();
        let at = values
            .remove("at")
            .and_then(|value| value.as_f64())
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            .ok_or_else(|| invalid(&format!("keyframes[{index}].at must be between 0 and 1")))?;
        let target_db = values
            .remove("gainDb")
            .and_then(|value| value.as_f64())
            .filter(|value| value.is_finite() && (-60.0..=24.0).contains(value))
            .ok_or_else(|| {
                invalid(&format!(
                    "keyframes[{index}].gainDb must be between -60 and +24 dB"
                ))
            })?;
        let ease = values.remove("ease");
        if let Some(unknown) = values.keys().next() {
            return Err(invalid(&format!(
                "unknown field `{unknown}` in keyframes[{index}]"
            )));
        }
        // A key at (nearly) the same position as one before it is a
        // replace, not a refusal - the same rule `keyframe_set` applies,
        // so re-sending a duck curve never errors on its own keys.
        let target_gain = gain_from_db(target_db);
        let mut command = json!({
            "op": "setClipKey",
            "clipId": clip_id,
            "property": "volume",
            "at": at,
            "value": target_gain,
        });
        if let Some(ease) = ease {
            let valid = ease.as_array().is_some_and(|points| {
                points.len() == 4
                    && points.iter().enumerate().all(|(place, point)| {
                        point.as_f64().is_some_and(|number| {
                            number.is_finite()
                                // The two x coordinates (places 0 and 2) are
                                // times, not values: a CSS timing function
                                // keeps them in 0-1, and the engine's solver
                                // assumes they are. The y coordinates
                                // (places 1 and 3) may overshoot past 1 - a
                                // cubic-bezier(0.34, 1.56, 0.64, 1) is a
                                // legal spring, and clamping its y1 used to
                                // refuse it while accepting x2 > 1.
                                && (place != 0 && place != 2 || (0.0..=1.0).contains(&number))
                        })
                    })
            });
            if !valid {
                return Err(invalid(&format!(
                    "keyframes[{index}].ease must contain four finite cubic-bezier numbers"
                )));
            }
            command["ease"] = ease;
        }
        commands.push(command);
    }

    let command = if commands.len() == 1 {
        commands.pop().expect("one command")
    } else {
        json!({ "op": "batch", "commands": commands })
    };
    request("edit.apply", json!({ "path": path, "command": command }))
}

/// Linear gain multiplier for dB relative to unity, matching the editor fader.
fn gain_from_db(decibels: f64) -> f64 {
    if decibels <= -60.0 {
        0.0
    } else {
        10.0_f64.powf(decibels / 20.0)
    }
}

/// `plan_validate`: validate the director's plan before tool-driven edits.
fn validate_plan_tool(mut arguments: Value) -> Result<Value, ApiError> {
    let invalid = |why: &str| ApiError::invalid(format!("invalid arguments: {why}"));
    let fields = arguments
        .as_object_mut()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let plan = fields
        .remove("plan")
        .ok_or_else(|| invalid("`plan` is required"))?;
    if let Some(unknown) = fields.keys().next() {
        return Err(invalid(&format!("unknown field `{unknown}`")));
    }
    let schema = canonical_plan_schema()
        .ok_or_else(|| invalid("the canonical edit-plan schema could not be loaded"))?;
    crate::schema::check(&schema, &plan, &schema, "plan").map_err(|message| invalid(&message))?;
    let result = validate_edit_plan(&plan).map_err(ApiError::invalid)?;
    Ok(result)
}

/// An actual id-shaped reference, without claiming that a live project or
/// media library contains it.
fn valid_identifier(value: &Value) -> bool {
    match value {
        Value::String(id) => !id.trim().is_empty(),
        Value::Number(id) => {
            id.as_i64().is_some_and(|id| id > 0) || id.as_u64().is_some_and(|id| id > 0)
        }
        _ => false,
    }
}

/// Cross-field checks JSON Schema cannot express: actual timeline continuity,
/// usable visual references, and realistic audio/caption values.
fn validate_edit_plan(plan: &Value) -> Result<Value, String> {
    let object = plan
        .as_object()
        .ok_or_else(|| "plan must be a JSON object".to_owned())?;
    let aspect = object
        .get("aspect")
        .and_then(Value::as_str)
        .ok_or_else(|| "aspect is required (16:9, 9:16, 1:1, or 4:5)".to_owned())?;
    if !["16:9", "9:16", "1:1", "4:5"].contains(&aspect) {
        return Err(format!("aspect `{aspect}` is not supported"));
    }
    if !object.get("audio").is_some_and(Value::is_object) {
        return Err("audio must be an object".to_owned());
    }
    let segments = object
        .get("segments")
        .and_then(Value::as_array)
        .filter(|segments| !segments.is_empty() && segments.len() <= 500)
        .ok_or_else(|| "segments must be a non-empty array with at most 500 entries".to_owned())?;
    let mut warnings = Vec::<String>::new();
    let mut previous_end: Option<f64> = None;
    let mut runtime = 0.0_f64;
    // What the structure checks below need, gathered on the way past.
    let mut boundaries = Vec::<f64>::new();
    let mut cut_types = Vec::<&str>::new();
    let mut static_entrances = 0_usize;
    let mut missing_entrances = 0_usize;
    let mut comps = 0_usize;
    let mut sfx_uses = Vec::<(&str, f64)>::new();
    for (index, segment) in segments.iter().enumerate() {
        let segment = segment
            .as_object()
            .ok_or_else(|| format!("segments[{index}] must be an object"))?;
        let number = |key: &str| {
            segment
                .get(key)
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("segments[{index}].{key} must be a finite number"))
        };
        let start = number("start")?;
        let end = number("end")?;
        if start < 0.0 || end <= start {
            return Err(format!(
                "segments[{index}] must have start >= 0 and end > start"
            ));
        }
        if index == 0 && start > 0.05 {
            return Err(format!(
                "segments[0] starts at {start:.3}s; the plan must start at 0s (or within 50 ms)"
            ));
        }
        if let Some(previous_end) = previous_end {
            let delta = start - previous_end;
            if delta > 0.05 {
                return Err(format!("gap of {delta:.3}s before segments[{index}]"));
            }
            if delta < -0.05 {
                return Err(format!(
                    "overlap of {:.3}s before segments[{index}]",
                    -delta
                ));
            }
            if delta.abs() > 0.001 {
                warnings.push(format!(
                    "segments[{index}] differs from the previous boundary by {delta:.3}s"
                ));
            }
        }
        let visual = segment
            .get("visual")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("segments[{index}].visual must be an object"))?;
        let kind = visual
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("segments[{index}].visual.type is required"))?;
        match kind {
            "broll" => {
                let query = visual
                    .get("query")
                    .and_then(Value::as_str)
                    .is_some_and(|q| !q.trim().is_empty());
                let id = visual.get("clipId").is_some_and(valid_identifier);
                if !query && !id {
                    return Err(format!(
                        "segments[{index}] B-roll needs a search query or a non-empty clipId"
                    ));
                }
            }
            "existing" => {
                let media = visual
                    .get("mediaId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.trim().is_empty());
                let clip = visual.get("clipId").is_some_and(valid_identifier);
                let file = visual
                    .get("file")
                    .and_then(Value::as_str)
                    .is_some_and(|path| !path.trim().is_empty());
                if !media && !clip && !file {
                    return Err(format!(
                        "segments[{index}] existing footage needs mediaId, clipId, or file"
                    ));
                }
            }
            "textCard" | "titleCard" => {
                if visual
                    .get("text")
                    .and_then(Value::as_str)
                    .is_none_or(|text| text.trim().is_empty())
                {
                    return Err(format!("segments[{index}] {kind} needs non-empty text"));
                }
            }
            other => {
                return Err(format!(
                    "segments[{index}].visual.type `{other}` is unsupported"
                ));
            }
        }
        if let Some(transition) = segment.get("transitionIn").filter(|value| !value.is_null()) {
            let transition = transition.as_object().ok_or_else(|| {
                format!("segments[{index}].transitionIn must be an object or null")
            })?;
            if transition
                .get("effect")
                .and_then(Value::as_str)
                .is_none_or(|effect| effect.trim().is_empty())
            {
                return Err(format!("segments[{index}].transitionIn.effect is required"));
            }
            if let Some(seconds) = transition.get("seconds") {
                let seconds = seconds
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| {
                        format!("segments[{index}].transitionIn.seconds must be finite")
                    })?;
                // The range the schema doc promises: 0.25-1.0, with 0
                // (or absent) a hard cut. The two agree because an agent
                // reads the doc and a validator that contradicts it is a
                // loop of confusing refusals.
                if seconds != 0.0 && !(0.25..=1.0).contains(&seconds) {
                    return Err(format!(
                        "segments[{index}].transitionIn.seconds must be 0.25-1.0 (0 for a hard cut)"
                    ));
                }
            }
        }
        if let Some(sfx) = segment.get("sfx").filter(|value| !value.is_null()) {
            let sfx = sfx
                .as_object()
                .ok_or_else(|| format!("segments[{index}].sfx must be an object or null"))?;
            if sfx
                .get("query")
                .and_then(Value::as_str)
                .is_none_or(|query| query.trim().is_empty())
            {
                return Err(format!("segments[{index}].sfx.query is required"));
            } else if let Some(query) = sfx.get("query").and_then(Value::as_str) {
                sfx_uses.push((query, start));
            }
            if let Some(peak) = sfx.get("peakAt") {
                let peak = peak
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| format!("segments[{index}].sfx.peakAt must be finite"))?;
                if peak < 0.0 || peak > end + 0.05 {
                    return Err(format!(
                        "segments[{index}].sfx.peakAt is outside the planned runtime"
                    ));
                }
            }
            if let Some(gain) = sfx.get("gainDb") {
                let gain = gain
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| format!("segments[{index}].sfx.gainDb must be finite"))?;
                if !(-60.0..=0.0).contains(&gain) {
                    return Err(format!(
                        "segments[{index}].sfx.gainDb must be between -60 and 0 dB"
                    ));
                }
            }
        }
        // Structure gathering: the floors in SKILL.md §3, checked below.
        boundaries.push(start);
        cut_types.push(segment.get("cutType").and_then(Value::as_str).unwrap_or(""));
        match segment.get("in").filter(|value| !value.is_null()) {
            Some(entrance) => {
                if entrance.get("anim") == Some(&Value::String("static".to_owned())) {
                    static_entrances += 1;
                }
            }
            None => missing_entrances += 1,
        }
        if segment
            .get("comp")
            .is_some_and(|comp| !comp.is_null() && comp.as_object().is_some_and(|c| !c.is_empty()))
        {
            comps += 1;
        }
        previous_end = Some(end);
        runtime = end;
    }

    // The director checks: mode, sections, cut variety, entrances, comp
    // density, SFX spacing. Cross-field laws the schema cannot say.
    if let Some(mode) = object.get("mode").and_then(Value::as_str) {
        if mode == "reel" && runtime > 62.0 {
            return Err(format!(
                "a reel is retention engineering under a minute: {runtime:.1}s is longform, change the mode"
            ));
        }
        if mode == "reel" && runtime > 60.0 {
            warnings.push(format!(
                "{runtime:.1}s is past the 60 s reel edge; some platforms will file it as longform"
            ));
        }
        if mode == "longform" && runtime > 90.0 {
            let has_rehook = object
                .get("sections")
                .and_then(Value::as_array)
                .is_some_and(|sections| {
                    sections.iter().any(|section| {
                        section.get("role").and_then(Value::as_str) == Some("rehook")
                    })
                });
            if !has_rehook {
                warnings.push(
                    "longform past 90 s with no rehook section: the 50-60% retention dip goes uncaught"
                        .to_owned(),
                );
            }
        }
    }
    if let Some(sections) = object.get("sections").and_then(Value::as_array) {
        if sections.is_empty() {
            return Err("sections, when supplied, must not be empty".to_owned());
        }
        let rank = |role: &str| match role {
            "hook" => 0,
            "intro" => 1,
            "body" | "chapter" => 2,
            "rehook" => 3,
            "cta" => 4,
            _ => 5, // outro
        };
        let mut previous: Option<(&str, f64, f64)> = None;
        for (index, section) in sections.iter().enumerate() {
            let section = section
                .as_object()
                .ok_or_else(|| format!("sections[{index}] must be an object"))?;
            let role = section
                .get("role")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("sections[{index}].role is required"))?;
            let start = section
                .get("start")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("sections[{index}].start must be finite"))?;
            let end = section
                .get("end")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("sections[{index}].end must be finite"))?;
            if end <= start {
                return Err(format!("sections[{index}] must have end > start"));
            }
            if index == 0 {
                if role != "hook" {
                    return Err(format!(
                        "sections[0] is the {role}; a piece starts with its hook"
                    ));
                }
                if start > 0.05 {
                    return Err(format!(
                        "sections[0] starts at {start:.3}s; the hook starts the plan"
                    ));
                }
            }
            if let Some((previous_role, _, previous_end)) = previous {
                if start - previous_end > 0.05 {
                    return Err(format!(
                        "gap of {:.3}s before sections[{index}]",
                        start - previous_end
                    ));
                }
                if start - previous_end < -0.05 {
                    return Err(format!(
                        "overlap of {:.3}s before sections[{index}]",
                        previous_end - start
                    ));
                }
                if rank(role) < rank(previous_role) {
                    return Err(format!(
                        "sections[{index}] is a {role} after a {previous_role}; the order is hook, intro, body/chapters, rehook, cta, outro"
                    ));
                }
            }
            // A section change is a cut: it should land on a segment
            // boundary, not mid-shot.
            if index > 0
                && !boundaries
                    .iter()
                    .any(|boundary| (boundary - start).abs() <= 0.05)
            {
                warnings.push(format!(
                    "sections[{index}] ({role}) starts at {start:.3}s, mid-segment; section changes land on cuts"
                ));
            }
            if role == "hook" && end > 4.0 {
                warnings.push(format!(
                    "the hook section runs to {end:.1}s; the hook should land inside 3 s"
                ));
            }
            previous = Some((role, start, end));
        }
        if let Some((_, _, last_end)) = previous
            && (last_end - runtime).abs() > 0.05
        {
            return Err(format!(
                "sections end at {last_end:.3}s but the plan runs to {runtime:.3}s; sections cover the whole piece"
            ));
        }
    }
    // The footage slots: each slot's own contract first, then how much of
    // the plan goes shopping for stock at all. A plan without slots is a
    // plan the segments alone describe - the slots are how a director
    // shops, and the share is what keeps stock coverage rather than
    // collage. The band: 30-45% is coverage; under that the picture leans
    // on the A-roll alone; past 45% it leans on the library; past 60% it
    // is stock with gaps, and the plan is refused.
    if let Some(slots) = object.get("slots").and_then(Value::as_array) {
        let mut broll = 0_usize;
        for (index, slot) in slots.iter().enumerate() {
            let slot = slot
                .as_object()
                .ok_or_else(|| format!("slots[{index}] must be an object"))?;
            let description = slot
                .get("description")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("slots[{index}].description is required"))?;
            if description.chars().count() < 40 {
                return Err(format!(
                    "slots[{index}].description is {} characters; it is what CLIP ranks against and must be at least 40",
                    description.chars().count()
                ));
            }
            let treatment = slot
                .get("treatment")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("slots[{index}].treatment is required"))?;
            let queries: Vec<&str> = slot
                .get("queries")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            match treatment {
                "broll" => {
                    broll += 1;
                    if queries.iter().all(|query| query.trim().is_empty()) {
                        return Err(format!(
                            "slots[{index}] is b-roll; it takes at least one non-empty query"
                        ));
                    }
                }
                "hold" | "graphic" | "black" => {
                    if queries.iter().any(|query| !query.trim().is_empty()) {
                        return Err(format!(
                            "slots[{index}] is a {treatment} treatment; the edit draws it itself and takes no search queries"
                        ));
                    }
                }
                other => {
                    return Err(format!("slots[{index}].treatment `{other}` is unsupported"));
                }
            }
        }
        if !slots.is_empty() {
            let share = broll as f64 / slots.len() as f64;
            if share > 0.60 {
                return Err(format!(
                    "b-roll covers {:.0}% of the slots; past 60% the edit is stock with gaps - hold some spans for the A-roll and the graphics",
                    share * 100.0
                ));
            }
            if share > 0.45 {
                warnings.push(format!(
                    "b-roll covers {:.0}% of the slots; the 30-45% band is where stock reads as coverage rather than collage",
                    share * 100.0
                ));
            }
            if share < 0.30 {
                warnings.push(format!(
                    "b-roll covers {:.0}% of the slots; under 30% the picture leans on the A-roll alone",
                    share * 100.0
                ));
            }
        }
    }
    // A run of plain hard cuts is a slideshow, whatever the schema allows.
    let mut hard_run = 0_usize;
    let mut longest_hard_run = 0_usize;
    for cut in &cut_types {
        if *cut == "hard" {
            hard_run += 1;
            longest_hard_run = longest_hard_run.max(hard_run);
        } else {
            hard_run = 0;
        }
    }
    if longest_hard_run >= 6 {
        warnings.push(format!(
            "{longest_hard_run} hard cuts in a row; the catalog has twelve cut types - a run of hards is a slideshow"
        ));
    }
    if missing_entrances > 0 {
        warnings.push(format!(
            "{missing_entrances} segment(s) have no entrance; floor M: every clip enters on purpose (pop, slide, punch, dissolve - or static, once)"
        ));
    }
    if static_entrances > 1 {
        warnings.push(format!(
            "{static_entrances} static entrances; static is deliberate, at most one per piece"
        ));
    }
    let comp_floor = ((runtime / 15.0).ceil() as usize).max(1);
    if comps < comp_floor {
        warnings.push(format!(
            "{comps} designed comp(s) for {runtime:.0}s; the floor is one per section and one per 15 s ({comp_floor} here)"
        ));
    }
    for (left, (left_query, left_at)) in sfx_uses.iter().enumerate() {
        for (right_query, right_at) in sfx_uses.iter().skip(left + 1) {
            if left_query == right_query && (right_at - left_at).abs() < 30.0 {
                warnings.push(format!(
                    "sound `{left_query}` is used at {left_at:.1}s and {right_at:.1}s; never the same sound twice within 30 s"
                ));
            }
        }
    }
    if let Some(captions) = object.get("captions") {
        let captions = captions
            .as_object()
            .ok_or_else(|| "captions must be an object".to_owned())?;
        // Word-synced captions are a motion comp now, not a timeline
        // caption style: captions_animate takes measured word times and
        // bakes the kinetic line sequence. The timeline's own captions
        // stay segment-timed - that is what transcribe provides.
        if captions.contains_key("wordSynced") {
            return Err("the timeline's captions are segment-timed; set wordSynced aside and bake word-timed captions with captions_animate, which takes measured [text, start, end] words".to_owned());
        }
        if let Some(mode) = captions.get("syncMode")
            && mode.as_str() != Some("segment")
        {
            return Err("captions.syncMode must be `segment`; word timing is captions_animate's, not the timeline captions'".to_owned());
        }
    }
    if let Some(bgm) = object
        .get("audio")
        .and_then(|audio| audio.get("bgm"))
        .filter(|value| !value.is_null())
    {
        let bgm = bgm
            .as_object()
            .ok_or_else(|| "audio.bgm must be an object or null".to_owned())?;
        if let Some(duck) = bgm.get("duckDb") {
            let duck = duck
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| "audio.bgm.duckDb must be finite".to_owned())?;
            if !(-60.0..=0.0).contains(&duck) {
                return Err("audio.bgm.duckDb must be between -60 and 0 dB".to_owned());
            }
        }
    }
    if let Some(speed) = object
        .get("audio")
        .and_then(|audio| audio.get("voiceover"))
        .and_then(|voice| voice.get("speed"))
    {
        let speed = speed
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| "audio.voiceover.speed must be finite".to_owned())?;
        if !(0.5..=2.0).contains(&speed) {
            return Err("audio.voiceover.speed must be between 0.5 and 2.0".to_owned());
        }
    }
    match object.get("export") {
        None => warnings.push("export settings are not specified; the plan alone does not guarantee an export profile".to_owned()),
        Some(Value::Object(export)) => {
            // What the export tools themselves will refuse later, said
            // here while the plan is still a plan.
            if let Some(crf) = export.get("crf") {
                let crf = crf
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| "export.crf must be a number".to_owned())?;
                if !(0.0..=51.0).contains(&crf) {
                    return Err("export.crf must be between 0 and 51".to_owned());
                }
            }
            if let Some(codec) = export.get("codec").and_then(Value::as_str)
                && !matches!(codec, "h264" | "hevc" | "av1")
            {
                return Err(format!(
                    "export.codec {codec:?} is not one of h264, hevc, av1"
                ));
            }
        }
        Some(_) => return Err("export must be an object when supplied".to_owned()),
    }
    let creative = creative_gate(plan, segments, runtime, &warnings);
    let signal_log = signal_log_gate(plan, segments, runtime);
    let mut result = json!({
        "valid": true,
        "aspect": aspect,
        "segmentCount": segments.len(),
        "runtimeSeconds": runtime,
        "warnings": warnings,
        "creativeReady": creative["ready"],
        "creativeBlockers": creative["blockers"],
        "craftScore": creative["score"],
        "creativeNotes": creative["notes"],
        "signalLog": signal_log,
        "next": "If creativeReady is false, call creative_brief, fix the blockers, and validate again. If signalDriven is false, read the voiceover with voice_signals and record the signals behind each decision in the plan's signalLog. Structure is not craft.",
    });
    // A plan with a voiceover and no signal log is not creativeReady: it
    // was written without reading the take it cuts.
    if signal_log["driven"].as_bool() == Some(false)
        && plan
            .pointer("/audio/voiceover")
            .is_some_and(|voiceover| !voiceover.is_null())
    {
        let blockers = result["creativeBlockers"]
            .as_array_mut()
            .expect("the creative gate answers blockers");
        blockers.push(json!("the plan has a voiceover but no signalLog: read the take with voice_signals and record which signals drove each cut, caption, graphic, BGM and transition"));
        result["creativeReady"] = json!(false);
    }
    Ok(result)
}

/// The signal vocabulary a log entry may name: the families the
/// voice_signals profile answers and the decision policy reads. A signal
/// matches when it names one, exactly or as its prefix
/// (`pause.class=dead_air` names `pause`).
const SIGNAL_VOCABULARY: &[&str] = &[
    "pause",
    "emphasisWords",
    "speechRate",
    "rhythmLayers",
    "motifs",
    "words",
    "language",
    "sentence",
    "entities",
    "speechActs",
    "claims",
    "forensic",
    "musicSegments",
    "speechRatio",
    "speakerChanges",
    "topics",
    "discourseMarkers",
    "loudness",
    "prosody",
    "fillers",
    "arc",
    "emotion",
    "formants",
    "speaker",
    "noiseFloor",
];

/// The decision log's gate: which signals drove each choice. A plan that
/// carries entries names, for every cut, caption, graphic, BGM and
/// transition decision, the signals from the voice_signals profile
/// behind it - so the reading becomes a decision instead of a data dump,
/// and a choice with no signals behind it is visible as one. Entries are
/// checked against the vocabulary and the plan's runtime; a plan with no
/// log at all reports `driven: false` and the voiceover check above
/// turns that into a creative blocker.
fn signal_log_gate(plan: &Value, segments: &[Value], runtime: f64) -> Value {
    let entries = plan
        .get("signalLog")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut problems: Vec<String> = Vec::new();
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for (at, entry) in entries.iter().enumerate() {
        let decision = entry
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(
            decision,
            "cut" | "caption" | "graphic" | "bgm" | "transition"
        ) {
            problems.push(format!(
                "entry {at}: decision {decision:?} is not one of cut, caption, graphic, bgm, transition"
            ));
            continue;
        }
        if let Some(time) = entry.get("at").and_then(Value::as_f64)
            && !(0.0..=runtime + 0.05).contains(&time)
        {
            problems.push(format!(
                "entry {at}: at {time} lies outside the plan's 0-{runtime} s"
            ));
        }
        let signals = entry
            .get("signals")
            .and_then(Value::as_array)
            .map(|signals| {
                signals
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if signals.is_empty() {
            problems.push(format!(
                "entry {at}: no signals named - a decision the profile did not drive"
            ));
        }
        for signal in &signals {
            if !SIGNAL_VOCABULARY.iter().any(|known| {
                signal == *known
                    || signal.starts_with(&format!("{known}."))
                    || signal.starts_with(&format!("{known}["))
            }) {
                problems.push(format!(
                    "entry {at}: signal {signal:?} is not one the voice_signals profile answers"
                ));
            }
        }
        *counts.entry(decision).or_default() += 1;
    }
    let driven = !entries.is_empty() && problems.is_empty();
    // The decisions a plan this size owes a log for: the cuts are the
    // segment boundaries, and one BGM choice, one caption style and one
    // grade are the video-wide minimum. Entries beyond the minimum are
    // welcome; the coverage names how many there are.
    let expected_cuts = segments.len().saturating_sub(1);
    let logged_cuts = *counts.get("cut").unwrap_or(&0);
    let coverage = if entries.is_empty() {
        0.0
    } else {
        (logged_cuts as f64 / expected_cuts.max(1) as f64).min(1.0)
    };
    let mut per_decision = serde_json::Map::new();
    for decision in ["cut", "caption", "graphic", "bgm", "transition"] {
        per_decision.insert(
            decision.to_owned(),
            json!(*counts.get(decision).unwrap_or(&0)),
        );
    }
    json!({
        "driven": driven,
        "entries": entries.len(),
        "coverage": (coverage * 100.0).round() / 100.0,
        "perDecision": per_decision,
        "problems": problems,
        "how": "voice_signals (action extract) on the take, then record {at, decision, signals, note} for each choice; the profile's decisionPolicy names the signals each decision reads.",
    })
}

/// The creative gate. Structural validity is not the same thing as a good
/// edit: a plan can have perfect continuity and still have no point of view,
/// no emotional arc and sixteen near-identical stock shots. These are the
/// checks that turn the director's "warnings" into something an agent cannot
/// quietly ignore.
fn creative_gate(object: &Value, segments: &[Value], runtime: f64, warnings: &[String]) -> Value {
    let mut blockers: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut score = 10.0_f64;

    let core = [
        ("audience", "who is this for"),
        ("platform", "where it plays"),
        ("goal", "what the viewer should do or feel"),
        ("thesis", "the one sentence the cut argues"),
        ("curiosityGap", "the question opened in the first seconds"),
        ("payoff", "what answers that question"),
        ("emotionalArc", "three or more beats in order"),
    ];
    let brief = object.get("creativeBrief");
    match brief {
        None => {
            blockers.push(
                "no creativeBrief: call creative_brief and set audience, platform, goal, thesis, curiosityGap, payoff and emotionalArc before planning".to_owned(),
            );
            score -= 3.0;
        }
        Some(Value::Object(fields)) => {
            for (key, why) in &core {
                if fields
                    .get(*key)
                    .filter(|v| {
                        !v.is_null() && !matches!(v, Value::String(s) if s.trim().is_empty())
                    })
                    .is_none()
                {
                    blockers.push(format!("creativeBrief.{key} is missing ({why})"));
                    score -= 0.5;
                }
            }
            if let Some(Value::Array(arc)) = fields.get("emotionalArc") {
                if arc.len() < 3 {
                    blockers.push(
                        "creativeBrief.emotionalArc needs at least three distinct beats".to_owned(),
                    );
                } else {
                    let unique: std::collections::BTreeSet<&str> =
                        arc.iter().filter_map(Value::as_str).collect();
                    if unique.len() < 3 {
                        notes.push(
                            "emotionalArc repeats itself; every beat should feel different"
                                .to_owned(),
                        );
                        score -= 0.5;
                    }
                }
            }
            if fields.get("referenceStyle").is_none() {
                notes.push(
                    "no referenceStyle: name a director or editor whose rhythm you are borrowing"
                        .to_owned(),
                );
            }
        }
        Some(_) => blockers.push("creativeBrief must be an object".to_owned()),
    }

    // Sections must take the viewer somewhere, not just exist.
    let sections = object
        .get("sections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let roles: Vec<&str> = sections
        .iter()
        .filter_map(|s| s.get("role").and_then(Value::as_str))
        .collect();
    if !roles.contains(&"hook") {
        blockers.push("the first section must be a hook".to_owned());
        score -= 1.0;
    }
    if runtime > 12.0 && !roles.iter().any(|r| matches!(*r, "cta" | "outro")) {
        blockers.push(
            "a piece over 12 s needs a cta or outro section: the payoff has to land somewhere"
                .to_owned(),
        );
        score -= 0.5;
    }
    if let Some(first) = sections.first() {
        let end = first.get("end").and_then(Value::as_f64).unwrap_or(0.0);
        if end > 4.0 {
            blockers.push(format!(
                "the hook runs to {end:.1}s; open the curiosity gap inside 3 s"
            ));
            score -= 0.5;
        }
    }

    // Visual variety: repeated references are the slideshow signature.
    let visual_key = |segment: &Value| -> String {
        let v = &segment["visual"];
        for key in ["query", "path", "asset", "name", "src"] {
            if let Some(s) = v.get(key).and_then(Value::as_str) {
                return s.trim().to_ascii_lowercase();
            }
        }
        v.get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_ascii_lowercase()
    };
    let keys: Vec<String> = segments.iter().map(visual_key).collect();
    let distinct = keys.iter().collect::<std::collections::BTreeSet<_>>().len();
    if segments.len() >= 4 && distinct * 2 < segments.len() {
        blockers.push(format!(
            "only {distinct} distinct visuals across {} segments: this is a slideshow, not an edit; use stock_contact_sheet to find different shots",
            segments.len()
        ));
        score -= 1.5;
    }
    let mut run = 1usize;
    let mut longest = 1usize;
    for pair in keys.windows(2) {
        if pair[0] == pair[1] {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 1;
        }
    }
    if longest >= 3 {
        blockers.push(format!("{longest} consecutive segments use the same visual reference; vary shot size, angle or source"));
        score -= 1.0;
    }
    let designed = segments
        .iter()
        .filter(|s| {
            s.get("comp")
                .is_some_and(|c| c.as_object().is_some_and(|o| !o.is_empty()))
                || s.get("text").is_some_and(|t| !t.is_null())
        })
        .count();
    if runtime > 15.0 && designed == 0 {
        blockers.push(
            "no designed comp or text card anywhere: the cut has no visual authorship".to_owned(),
        );
        score -= 1.0;
    }
    if object.get("captions").is_none() && object.get("text").is_none() {
        notes.push(
            "no caption plan: call transcribe and captions_animate with measured word times"
                .to_owned(),
        );
    }

    // Escalate the warnings a director would actually refuse to ship.
    for warning in warnings {
        let escalate = warning.contains("hard cuts in a row")
            || warning.contains("have no entrance")
            || warning.contains("static entrances")
            || warning.contains("designed comp(s)")
            || warning.contains("same sound twice");
        if escalate {
            score -= 0.4;
            if warning.contains("hard cuts in a row") || warning.contains("have no entrance") {
                blockers.push(format!("craft: {warning}"));
            }
        }
    }
    let score = score.clamp(0.0, 10.0).round() * 10.0 / 10.0;
    let ready = blockers.is_empty();
    if ready {
        notes.push("creative gate passed; still run watch_render and craft_critique after the first export".to_owned());
    }
    json!({ "ready": ready, "blockers": blockers, "notes": notes, "score": score })
}

/// `export_wait`: the job's status once it has ended, or when the time is
/// up, with progress reported on the way.
fn export_wait(
    mut arguments: Value,
    backend: &mut dyn Backend,
    cancelled: &AtomicBool,
    progress: Option<&dyn Fn(f64, f64, String)>,
) -> Result<Outcome, ApiError> {
    let timeout = match arguments
        .as_object_mut()
        .and_then(|fields| fields.remove("timeoutSeconds"))
    {
        None | Some(Value::Null) => WAIT_DEFAULT,
        Some(value) => value
            .as_f64()
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .ok_or_else(|| {
                ApiError::invalid("invalid arguments: `timeoutSeconds` is a number of seconds")
            })?
            .min(WAIT_LONGEST),
    };
    let status = request("export.status", arguments)?;
    let until = Instant::now() + Duration::from_secs_f64(timeout);
    let mut reported = -1.0_f64;
    loop {
        let mut current = backend.call(status.clone())?;
        let running = current["state"] == "running";
        if let Some(progress) = progress {
            let stage = current["stage"].as_str().unwrap_or_default();
            let place = stage_place(stage) as f64;
            let fraction = current["fraction"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
            let overall = if running {
                ((place + fraction) / STAGES as f64 * 100.0).min(99.9)
            } else {
                100.0
            };
            // A progress value must rise; a stage that starts over at 0
            // says nothing new until it passes where the last one was.
            if overall > reported {
                reported = overall;
                let frames = match (current["frame"].as_i64(), current["total"].as_i64()) {
                    (Some(frame), Some(total)) if total > 0 => format!(" {frame}/{total}"),
                    _ => String::new(),
                };
                let state = current["state"].as_str().unwrap_or_default();
                progress(
                    (overall * 10.0).round() / 10.0,
                    100.0,
                    format!("{state} {stage}{frames}").trim().to_owned(),
                );
            }
        }
        if !running {
            return Ok(Outcome::json(current));
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err(ApiError::new(
                ErrorCode::Cancelled,
                "stopped waiting; the export goes on",
            ));
        }
        if Instant::now() >= until {
            current["note"] = json!("still running: call this tool again, or export_status");
            return Ok(Outcome::json(current));
        }
        std::thread::sleep(POLL);
    }
}

/// `preview_frame`: the picture as an image, or the file written.
fn preview(mut arguments: Value, backend: &mut dyn Backend) -> Result<Outcome, ApiError> {
    let given_width = arguments
        .get("width")
        .filter(|value| !value.is_null())
        .cloned();
    let given_height = arguments.get("height").filter(|v| !v.is_null()).cloned();
    let sized = ["width", "height"]
        .iter()
        .all(|key| arguments.get(*key).is_some_and(|value| !value.is_null()));
    if !sized {
        // The timeline's own frame, fitted within PREVIEW_SIDE - for a
        // file as much as an inline picture: a 4K PNG is bytes an agent
        // never asked for, and one dimension given alone used to be
        // silently overwritten by the fit.
        let path = arguments.get("path").cloned().unwrap_or(Value::Null);
        let view = backend.call(request("project.get", json!({ "path": path }))?)?;
        let width = view
            .pointer("/settings/width")
            .and_then(Value::as_f64)
            .unwrap_or(1920.0);
        let height = view
            .pointer("/settings/height")
            .and_then(Value::as_f64)
            .unwrap_or(1080.0);
        let scale = (PREVIEW_SIDE / width.max(height)).min(1.0);
        if let Some(fields) = arguments.as_object_mut() {
            fields.insert(
                "width".to_owned(),
                json!(((width * scale).round() as u32).max(1)),
            );
            fields.insert(
                "height".to_owned(),
                json!(((height * scale).round() as u32).max(1)),
            );
        }
        // What the caller named outright wins over the fit.
        if let (Some(fields), Some(width)) = (arguments.as_object_mut(), given_width) {
            fields.insert("width".to_owned(), width);
        }
        if let (Some(fields), Some(height)) = (arguments.as_object_mut(), given_height) {
            fields.insert("height".to_owned(), height);
        }
    }
    let time = arguments.get("time").cloned().unwrap_or(Value::Null);
    let result = backend.call(request("preview.frame", arguments)?)?;
    let Some(png) = result.get("png").and_then(Value::as_str) else {
        return Ok(Outcome::json(result));
    };
    Ok(Outcome {
        content: vec![
            json!({ "type": "image", "data": png, "mimeType": "image/png" }),
            json!({
                "type": "text",
                "text": format!("The frame at {time} s, {}×{}.", result["width"], result["height"]),
            }),
        ],
        structured: Some(
            json!({ "width": result["width"], "height": result["height"], "time": time }),
        ),
        is_error: false,
    })
}

/// `collision_check`: every place two clips share a lane, read from the
/// same summary `timeline_read` shows. The placement engine never
/// makes one - `nearest_free_start` and `resolve_moves` see to that -
/// so a collision is a document written by hand or damaged; named,
/// so it can be unwound with the placing tools.
fn collision_report(view: Value) -> Outcome {
    use std::collections::BTreeMap;
    /// How far two spans may run into each other and still count as
    /// touching, as the engine's own placement sees it.
    const TOUCH: f64 = 1e-6;
    let mut by_lane: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for clip in view
        .pointer("/timeline/clips")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(lane) = clip.get("track").and_then(Value::as_str) {
            by_lane
                .entry(lane.to_owned())
                .or_default()
                .push(clip.clone());
        }
    }
    let mut collisions: Vec<Value> = Vec::new();
    for (lane, mut clips) in by_lane {
        clips.sort_by(|left, right| {
            let start = |clip: &Value| clip["start"].as_f64().unwrap_or(0.0);
            start(left).total_cmp(&start(right))
        });
        // Every pair, not only neighbours: a lane may hold a clip
        // running over two others with room for one between them.
        for (index, one) in clips.iter().enumerate() {
            let one_start = one["start"].as_f64().unwrap_or(0.0);
            let one_end = one["end"].as_f64().unwrap_or(0.0);
            for other in clips.iter().skip(index + 1) {
                let other_start = other["start"].as_f64().unwrap_or(0.0);
                let other_end = other["end"].as_f64().unwrap_or(0.0);
                // As the engine's `covers` sees it: sharing more than
                // an edge.
                if one_start < other_end - TOUCH && other_start < one_end - TOUCH {
                    collisions.push(json!({
                        "trackId": lane.clone(),
                        "clips": Value::Array(vec![one.clone(), other.clone()]),
                        "from": other_start,
                        "to": one_end.min(other_end),
                    }));
                }
            }
        }
    }
    let clean = collisions.is_empty();
    Outcome::json(json!({
        "count": collisions.len(),
        "clean": clean,
        "collisions": collisions,
    }))
}

/// `openvid_search_batch`: the same search, once per query, in one call.
/// An agent shopping a plan's B-roll runs one of these where it used to
/// run a row of `openvid_search` round trips; the index it reads is one,
/// so a library that is not prepared fails the whole call as a single
/// search would, not per query. The CLIP rerank rides along exactly as
/// it does in `openvid_search` - once per query, `rerank: false` aside -
/// because a batch is a row of the same search, not a second, lesser
/// search that quietly returns what nobody looked at.
fn openvid_search_batch(arguments: Value, backend: &mut dyn Backend) -> Result<Outcome, ApiError> {
    let invalid = |why: &str| ApiError::invalid(format!("invalid arguments: {why}"));
    let mut shared = match arguments {
        Value::Object(fields) => fields,
        _ => return Err(invalid("the arguments are an object")),
    };
    let queries = match shared.remove("queries") {
        Some(queries) => queries,
        None => return Err(invalid("`queries` is required")),
    };
    // The batch's own flag, consumed here and never forwarded: the API
    // method does not know it.
    let rerank = shared
        .remove("rerank")
        .and_then(|flag| flag.as_bool())
        .unwrap_or(true);
    let Some(queries) = queries.as_array() else {
        return Err(invalid("`queries` is a list of queries"));
    };
    if queries.is_empty() {
        return Err(invalid("`queries` may not be empty"));
    }
    if queries.len() > 8 {
        return Err(invalid("`queries` is at most 8 searches"));
    }
    if queries.iter().any(|query| query.as_str().is_none()) {
        return Err(invalid("every entry of `queries` is a query's words"));
    }
    let queries: Vec<Value> = queries.clone();
    let mut searches = Vec::with_capacity(queries.len());
    for query in queries {
        shared.insert("query".to_owned(), query.clone());
        // The arguments are remembered before `request` stamps the method
        // into them, because the rerank reads the caller's query, sort and
        // offset back out - the same remembering `openvid_search`'s own
        // dispatch arm does.
        let remembered = Value::Object(shared.clone());
        let found = backend.call(request("openvid.search", Value::Object(shared.clone()))?)?;
        let found = if rerank {
            crate::sense::semantic_rerank(&remembered, found, backend)
        } else {
            found
        };
        searches.push(json!({
            "query": query,
            "total": found.get("total").cloned().unwrap_or(Value::Null),
            "clips": found
                .get("clips")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
            "rerank": found.get("rerank").cloned().unwrap_or(Value::Null),
        }));
    }
    Ok(Outcome::json(json!({ "searches": searches })))
}

/// How wide the ranker's page is: the same pool the rerank scores, wide
/// enough that the meaning has room to be on it, and wider than any k a
/// slot asks for.
const RANK_POOL: usize = 50;

/// The page a slot asks for: a hero slot's first glance earns a wider
/// answer. An explicit `k` is the caller's; the hero flag only fills the
/// default - 30 for the shot the thumbnail lives on, 12 for any other.
fn slot_k(explicit: Option<u64>, hero: bool) -> usize {
    explicit
        .map(|k| k.clamp(1, 100) as usize)
        .unwrap_or(if hero { 30 } else { 12 })
}

/// The terms a slot ranks with: the description's and the queries'
/// words, lowercased, filler dropped, deduplicated in order - the tags
/// the captions are measured against.
fn slot_terms(description: &str, queries: &[String]) -> Vec<String> {
    /// The glue words every caption carries, which agree with nothing in
    /// particular: a tag of `the` matches every clip and sorts none.
    const STOPWORDS: [&str; 8] = ["the", "and", "but", "for", "that", "with", "from", "this"];
    let mut terms: Vec<String> = Vec::new();
    let mut push = |text: &str| {
        for word in text.split(|c: char| !c.is_alphanumeric()) {
            let word = word.to_lowercase();
            if word.len() >= 3 && !STOPWORDS.contains(&word.as_str()) && !terms.contains(&word) {
                terms.push(word);
            }
        }
    };
    push(description);
    for query in queries {
        push(query);
    }
    terms
}

/// Jaccard similarity between the slot's terms and a caption's words:
/// 1.0 when the caption carries every term, 0.0 when it shares nothing.
fn tag_similarity(terms: &[String], caption: &str) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let words: std::collections::BTreeSet<String> = caption
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| !word.is_empty())
        .collect();
    let intersection = terms.iter().filter(|term| words.contains(*term)).count();
    let union = terms.len() + words.len() - intersection;
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// `openvid_rank_for_slot`: a page of stock, ranked the way a plan shops.
///
/// The slot's rich description is the query, so BM25 builds the page and
/// the CLIP second pass re-orders it by what the clips SHOW - the same
/// two steps `openvid_search` runs, against better words than a keyword.
/// The slot's fallback queries blend in at `tag_weight` as Jaccard
/// similarity against each caption, so a clip the description's
/// shortlist treats well and the queries agree with wins. The hard
/// filters - `motion_min`, `exclude_ids` - ride the search request and
/// are enforced once more on what lands, because a hard filter is a
/// promise, not a hope: a clip under the floor or already used in this
/// edit is not ranked lower, it is not there.
///
/// An empty or whitespace description is an error, not a quiet fallback
/// to the queries: ranking against the wrong words and saying nothing
/// is how a plan shops badly.
fn openvid_rank_for_slot(arguments: Value, backend: &mut dyn Backend) -> Result<Outcome, ApiError> {
    let invalid = |why: &str| ApiError::invalid(format!("invalid arguments: {why}"));
    let fields = arguments
        .as_object()
        .ok_or_else(|| invalid("the arguments are an object"))?;
    let description = fields
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .ok_or_else(|| {
            invalid(
                "`description` is required and may not be empty or whitespace: it is what CLIP ranks against, and falling back to the queries would rank against the wrong words quietly",
            )
        })?;
    let queries: Vec<String> = fields
        .get("queries")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let hero = fields.get("hero").and_then(Value::as_bool).unwrap_or(false);
    let k = slot_k(fields.get("k").and_then(Value::as_u64), hero);
    let tag_weight = fields
        .get("tag_weight")
        .or_else(|| fields.get("tagWeight"))
        .and_then(Value::as_f64)
        .map(|weight| weight.clamp(0.0, 1.0))
        .unwrap_or(0.3);
    let motion_min = fields
        .get("motion_min")
        .or_else(|| fields.get("motionMin"))
        .and_then(Value::as_f64);
    let exclude_ids: Vec<i64> = fields
        .get("exclude_ids")
        .or_else(|| fields.get("excludeIds"))
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();

    let mut search = json!({
        "query": description,
        "sort": "relevance",
        "limit": RANK_POOL,
    });
    if let Some(min) = motion_min {
        search["minMotion"] = json!(min);
    }
    if !exclude_ids.is_empty() {
        search["excludeIds"] = json!(exclude_ids);
    }
    let remembered = search.clone();
    let found = backend.call(request("openvid.search", search)?)?;
    let found = crate::sense::semantic_rerank(&remembered, found, backend);
    let rerank = found.get("rerank").cloned().unwrap_or(Value::Null);

    // The hard filters, enforced on the page whatever came back: a clip
    // under the floor or already used is not there.
    let excluded: std::collections::HashSet<i64> = exclude_ids.into_iter().collect();
    let page: Vec<Value> = found
        .get("clips")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|clip| {
            let above_floor = motion_min
                .map(|floor| {
                    clip.get("motion")
                        .and_then(Value::as_f64)
                        .is_none_or(|motion| motion >= floor)
                })
                .unwrap_or(true);
            let not_used = clip
                .get("id")
                .and_then(Value::as_i64)
                .is_none_or(|id| !excluded.contains(&id));
            above_floor && not_used
        })
        .collect();

    // The blend: the page's order is the seeing order, the tags are the
    // words' agreement. A rank position scores the seeing - 1.0 at the
    // front of the page, sliding to 0.0 at its end - and the Jaccard
    // similarity scores the words; `tag_weight` says how much the words
    // count. 0.3 by default: the description leads, the queries keep it
    // honest.
    let terms = slot_terms(description, &queries);
    let total = page.len();
    let mut scored: Vec<(f64, Value)> = page
        .into_iter()
        .enumerate()
        .map(|(index, clip)| {
            let rank_score = if total <= 1 {
                1.0
            } else {
                1.0 - index as f64 / (total - 1) as f64
            };
            let caption = clip.get("caption").and_then(Value::as_str).unwrap_or("");
            let score =
                (1.0 - tag_weight) * rank_score + tag_weight * tag_similarity(&terms, caption);
            (score, clip)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let clips: Vec<Value> = scored
        .into_iter()
        .take(k)
        .enumerate()
        .map(|(index, (score, mut clip))| {
            if let Some(slot) = clip.as_object_mut() {
                slot.insert("slotScore".into(), json!((score * 1000.0).round() / 1000.0));
                slot.insert("slotRank".into(), json!(index + 1));
            }
            clip
        })
        .collect();

    Ok(Outcome::json(json!({
        "description": description,
        "hero": hero,
        "k": k,
        "tagWeight": tag_weight,
        "total": found.get("total").cloned().unwrap_or(Value::Null),
        "rerank": rerank,
        "clips": clips,
    })))
}

/// `footage_continuity`: the plan-level continuity gate, as a tool.
///
/// The filled manifest goes in - what each slot got, with the numbers
/// the search measured - and the flags come out: where the cut will
/// jump, which rule caught it, and whether to shop again or let the
/// treatment downgrade. Nothing renders, nothing fetches; the gate is
/// arithmetic over the manifest's own numbers, and a manifest that does
/// not read is refused with words, not guessed at.
fn footage_continuity(arguments: Value) -> Result<Outcome, ApiError> {
    let manifest: concat_motion::continuity::AssetManifest = serde_json::from_value(arguments)
        .map_err(|error| {
            ApiError::invalid(format!(
                "invalid arguments: the manifest does not read: {error}"
            ))
        })?;
    let flags = concat_motion::continuity::check_continuity(&manifest);
    Ok(Outcome::json(json!({
        "count": flags.len(),
        "flags": flags,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::check;

    /// One call each tool may take, as an agent would write it.
    fn samples() -> Vec<(&'static str, Value)> {
        vec![
            ("concat_version", json!({})),
            ("project_list", json!({})),
            (
                "project_create",
                json!({ "location": "/tmp", "name": "Cut", "video": { "width": 1080, "height": 1920, "rateNum": 30, "rateDen": 1 } }),
            ),
            ("project_open", json!({ "path": "/tmp/Cut" })),
            ("project_save", json!({ "path": "/tmp/Cut" })),
            ("project_close", json!({ "path": "/tmp/Cut", "save": true })),
            ("timeline_read", json!({ "path": "/tmp/Cut" })),
            (
                "plan_validate",
                json!({ "plan": {
                    "aspect": "9:16",
                    "mode": "reel",
                    "audio": {},
                    "grade": { "filter": "concat.teal-orange", "intensityKey": "amount", "intensity": 65 },
                    "sections": [
                        { "role": "hook", "start": 0.0, "end": 2.0 },
                        { "role": "body", "start": 2.0, "end": 4.0 }
                    ],
                    "segments": [
                        { "start": 0.0, "end": 2.0, "visual": { "type": "broll", "query": "ocean waves aerial" } },
                        { "start": 2.0, "end": 4.0, "visual": { "type": "textCard", "text": "THE SALT STAYS" } }
                    ]
                } }),
            ),
            ("project_get", json!({ "path": "/tmp/Cut" })),
            (
                "project_set_video",
                json!({ "path": "/tmp/Cut", "video": { "width": 1920, "height": 1080, "rateNum": 30000, "rateDen": 1001 } }),
            ),
            ("media_probe", json!({ "path": "/tmp/a.mp4" })),
            ("geneva_explain", json!({ "code": "E302" })),
            (
                "motion_farm_progress",
                json!({ "farm": "/tmp/nowhere/farm" }),
            ),
            (
                "motion_farm_cloud",
                json!({ "action": "status", "farm": "/tmp/nowhere/farm" }),
            ),
            (
                "media_import",
                json!({ "path": "/tmp/Cut", "file": "/tmp/a.mp4" }),
            ),
            ("openvid_status", json!({})),
            ("openvid_prepare", json!({})),
            (
                "openvid_search",
                json!({ "query": "sunset ocean", "minAesthetic": 5.2, "limit": 10 }),
            ),
            ("openvid_get", json!({ "ids": [820338756299i64] })),
            ("openvid_fetch", json!({ "id": 820338756299i64 })),
            (
                "openvid_import",
                json!({ "path": "/tmp/Cut", "ids": [820338756299i64], "start": 0.0 }),
            ),
            (
                "sounds_search",
                json!({ "query": "cinematic impact", "category": "Impact", "limit": 10 }),
            ),
            (
                "sounds_add",
                json!({ "path": "/tmp/Cut", "id": "impact-glass-heavy", "start": 0.0 }),
            ),
            ("bgm_status", json!({})),
            ("bgm_prepare", json!({})),
            (
                "bgm_search",
                json!({ "query": "city rain", "source": "lofi", "category": "late-night", "limit": 10 }),
            ),
            ("bgm_fetch", json!({ "id": "lofi-0123456789ab" })),
            (
                "bgm_import",
                json!({ "path": "/tmp/Cut", "ids": ["lofi-0123456789ab"], "start": 0.0 }),
            ),
            ("magick_status", json!({})),
            ("magick_prepare", json!({})),
            (
                "magick_search",
                json!({ "query": "red balloon", "source": "magick", "limit": 10 }),
            ),
            ("magick_fetch", json!({ "id": "QlhA9gJbNR" })),
            (
                "magick_add",
                json!({ "path": "/tmp/Cut", "id": "QlhA9gJbNR", "start": 1.0, "duration": 3.0,
                        "scale": 0.3, "offsetX": 0.2, "offsetY": -0.1 }),
            ),
            (
                "edit_apply",
                json!({ "path": "/tmp/Cut", "command": { "op": "addTrack" } }),
            ),
            ("edit_undo", json!({ "path": "/tmp/Cut" })),
            ("edit_redo", json!({ "path": "/tmp/Cut" })),
            (
                "clip_add",
                json!({ "path": "/tmp/Cut", "mediaId": "m1", "trackId": "t1", "start": 2.5 }),
            ),
            (
                "clip_trim",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "edge": "end", "delta": -1.0, "ripple": true }),
            ),
            (
                "clips_split",
                json!({ "path": "/tmp/Cut", "clipIds": ["c1"], "time": 3.0 }),
            ),
            (
                "clips_move",
                json!({ "path": "/tmp/Cut", "moves": [{ "clipId": "c1", "start": 0.0, "trackId": "t2" }] }),
            ),
            (
                "clips_remove",
                json!({ "path": "/tmp/Cut", "clipIds": ["c1"], "ripple": false }),
            ),
            (
                "clip_update",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "patch": { "volume": 0.5, "crop": null, "transitionIn": { "id": "cross-fade", "duration": 0.5 }, "videoEffects": [{ "id": "concat.warm", "params": { "amount": 0.4 } }] } }),
            ),
            (
                "clip_audio_levels_db",
                json!({
                    "path": "/tmp/Cut",
                    "clipId": "c1",
                    "baseDb": -4.0,
                    "replaceAutomation": true,
                    "keyframes": [
                        { "at": 0.0, "gainDb": -12.0, "ease": [0.25, 0.1, 0.25, 1.0] },
                        { "at": 0.8, "gainDb": -4.0 }
                    ]
                }),
            ),
            (
                "clip_transform",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "scale": 1.2, "rotation": 15.0 }),
            ),
            (
                "clip_speed",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "speed": 0.5 }),
            ),
            (
                "title_add",
                json!({ "path": "/tmp/Cut", "start": 1.0, "duration": 3.0, "above": true, "style": { "content": "Hello", "fontSize": 0.08, "align": "left" }, "offsetY": -0.3 }),
            ),
            (
                "title_style_words",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "start": 0, "end": 4, "patch": { "color": "#ffd400", "fontWeight": 800 } }),
            ),
            (
                "title_clear_word_styles",
                json!({ "path": "/tmp/Cut", "clipId": "c1" }),
            ),
            (
                "captions_add",
                json!({ "path": "/tmp/Cut", "captions": [{ "start": 0.0, "end": 1.5, "text": "Hi" }], "style": { "color": "#ffff00" }, "offsetY": 0.3 }),
            ),
            (
                "keyframe_set",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "property": "scale", "at": 0.0, "value": 1.0, "ease": [0.42, 0.0, 0.58, 1.0] }),
            ),
            (
                "keyframe_clear",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "property": "opacity", "at": 1.0 }),
            ),
            (
                "effect_layer_add",
                json!({ "path": "/tmp/Cut", "start": 0.0, "duration": 2.0, "effectId": "concat.warm", "name": "Warm", "trackId": null }),
            ),
            (
                "effect_key_set",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "entry": 0, "key": "amount", "at": 0.5, "value": 0.8 }),
            ),
            ("track_add", json!({ "path": "/tmp/Cut", "bottom": false })),
            (
                "clips_add_sequence",
                json!({ "path": "/tmp/Cut", "mediaIds": ["m1", "m2"], "start": 0.0 }),
            ),
            (
                "clip_mask",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "mask": { "shape": "heart", "width": 0.5, "feather": 0.1, "invert": true } }),
            ),
            (
                "clip_stabilize",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "stabilize": { "smoothness": 0.7, "maxZoom": 1.2, "tripod": false } }),
            ),
            (
                "clip_frame",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "framing": "fill" }),
            ),
            (
                "clip_cut_spans",
                json!({ "path": "/tmp/Cut", "clipId": "c1", "spans": [[1.0, 2.0], [4.0, 5.5]], "ripple": true }),
            ),
            (
                "timeline_background",
                json!({ "path": "/tmp/Cut", "timelineId": "tl1", "background": { "kind": "blur", "amount": 0.5 } }),
            ),
            (
                "timeline_linkage",
                json!({ "path": "/tmp/Cut", "timelineId": "tl1", "on": false }),
            ),
            ("effects_list", json!({ "kind": "transition" })),
            ("template_list", json!({})),
            (
                "template_instantiate",
                json!({ "template": "/t", "location": "/tmp", "name": "From", "fills": [{ "mediaId": "m1", "file": "/tmp/a.mp4" }] }),
            ),
            (
                "export_start",
                json!({ "path": "/tmp/Cut", "output": "/tmp/cut.mp4", "crf": 23, "codec": "h264" }),
            ),
            ("export_status", json!({ "job": "j1" })),
            ("export_wait", json!({ "job": "j1", "timeoutSeconds": 5 })),
            ("export_cancel", json!({ "job": "j1" })),
            (
                "preview_frame",
                json!({ "path": "/tmp/Cut", "time": 1.5, "width": 320, "height": 180 }),
            ),
            (
                "analyze_scenes",
                json!({ "path": "/tmp/a.mp4", "threshold": 3.5 }),
            ),
            ("analyze_beats", json!({ "path": "/tmp/bgm.mp3" })),
            (
                "analyze_silence",
                json!({ "path": "/tmp/vo.wav", "thresholdDb": 35, "minSilenceMs": 500 }),
            ),
            (
                "transcribe",
                json!({ "path": "/tmp/a.mp4", "model": "base.en" }),
            ),
            (
                "tts_generate",
                json!({ "project": "/tmp/Cut", "text": "Hello, agent", "voice": 8, "speed": 1.0 }),
            ),
            ("models_list", json!({})),
            ("motion_presets", json!({})),
            (
                "motion_inspect",
                json!({ "preset": "funcut.lower-third.glass" }),
            ),
            (
                "scene_analysis",
                json!({ "preset": "funcut.lower-third.glass" }),
            ),
            (
                "motion_frame",
                json!({ "project": "/tmp/Cut", "preset": "funcut.lower-third.glass", "at": 1.0 }),
            ),
            (
                "motion_preview",
                json!({ "project": "/tmp/Cut", "preset": "funcut.lower-third.glass", "frames": 4 }),
            ),
            (
                "motion_bake",
                json!({ "project": "/tmp/Cut", "preset": "funcut.chapter.chip", "encode": true }),
            ),
            (
                "captions_animate",
                json!({ "project": "/tmp/Cut", "words": [["Make", 0.0, 0.4], ["it", 0.4, 0.8], ["move.", 0.8, 1.2]] }),
            ),
            (
                "motion_reel",
                json!({ "project": "/tmp/Cut", "name": "opening.mp4", "storyboard": {
                    "scenes": [
                        { "preset": "funcut.sting.wordmark", "hold": 4.0,
                          "grade": { "temperature": "cold" },
                          "layers": [
                              { "asset": "back", "zoom": 0.04 },
                              { "asset": "leak", "blend": "screen", "scale": 0.8,
                                "offset": [-0.25, -0.2], "fade_in": 0.4, "fade_out": 0.4, "above": true }
                          ] }
                    ],
                    "assets": { "back": "assets/back.jpg", "leak": "assets/leak.png" },
                    "audio": [ { "asset": "score", "gain_db": -2.0 } ],
                    "curtain": { "color": "#05060aff", "open": 0.6, "close": 0.8, "dip": 0.2, "depth": 1.0 }
                } }),
            ),
            (
                "motion_probe",
                json!({ "project": "/tmp/Cut", "path": "assets/back.jpg" }),
            ),
            (
                "scene_timeline",
                json!({ "project": "/tmp/Cut", "storyboard": {
                    "scenes": [
                        { "preset": "funcut.sting.wordmark", "hold": 4.0,
                          "grade": { "temperature": "warm", "amount": 0.8 },
                          "style": "punchy",
                          "layers": [
                              { "asset": "back", "zoom": 0.04 },
                              { "asset": "leak", "blend": "screen", "scale": 0.8,
                                "offset": [-0.25, -0.2], "fade_in": 0.4, "fade_out": 0.4, "above": true }
                          ] }
                    ],
                    "assets": { "back": "assets/back.jpg", "leak": "assets/leak.png" }
                } }),
            ),
            (
                "models_download",
                json!({ "kind": "tts", "id": "kokoro-int8-multi-lang-v1_0" }),
            ),
            (
                "analysis_wait",
                json!({ "job": "j1", "timeoutSeconds": 30 }),
            ),
            (
                "watch_render",
                json!({ "project": "/tmp/Cut", "samples": 8 }),
            ),
            (
                "stock_contact_sheet",
                json!({ "query": "rain on a window", "limit": 6, "rank": true }),
            ),
            (
                "craft_critique",
                json!({ "sheet": { "craft": { "samples": 8 } } }),
            ),
            (
                "motion_overlay",
                json!({ "project": "/tmp/Cut", "hash": "abc123", "plate": "/tmp/plate.mp4" }),
            ),
            (
                "suggest_cuts",
                json!({ "path": "/tmp/music.wav", "cuts": 8 }),
            ),
            ("measure_loudness", json!({ "path": "/tmp/render.mp4" })),
            (
                "voice_signals",
                json!({ "audio": "/tmp/voiceover.wav", "script": "You won't believe what they found." }),
            ),
            // The five family doors: one sample each, `action` naming a
            // member the schema's enum accepts.
            (
                "sense_discourse",
                json!({ "action": "analyze_discourse", "script": "Most people think it is willpower. But the data says systems. Because automation saves twice as much." }),
            ),
            (
                "sense_voice_quality",
                json!({ "action": "detect_vocal_events", "audio": "/tmp/voiceover.wav", "script": "So anyway [laughs] we tried it. *sigh* It did not work." }),
            ),
            (
                "sense_prosody_style",
                json!({ "action": "score_emphasis_type", "audio": "/tmp/voiceover.wav", "script": "But THIS is the first thing. Really matters. Second, here today, the numbers tell the story." }),
            ),
            (
                "sense_audio_repair",
                json!({ "action": "enhance_speech", "audio": "/tmp/voiceover.wav", "out": "/tmp/voiceover_enhanced.wav" }),
            ),
            (
                "sense_captions",
                json!({ "action": "seal_voice", "audio": "/tmp/voiceover.wav", "policy": { "allowedUses": ["youtube", "client-work"], "expiry": "2027-01-01" } }),
            ),
            (
                "track_subject",
                json!({ "path": "/tmp/clip.mp4", "aspect": "9:16" }),
            ),
            ("local_vlm", json!({ "action": "status" })),
            (
                "emotion",
                json!({ "script": "You won't believe what they found." }),
            ),
            (
                "creative_brief",
                json!({ "path": "/tmp/Cut", "action": "get" }),
            ),
            ("collision_check", json!({ "path": "/tmp/Cut" })),
            (
                "openvid_search_batch",
                json!({ "queries": ["ocean sunset", "city skyline"], "limit": 5 }),
            ),
            (
                "openvid_rank_for_slot",
                json!({
                    "description": "a slow aerial drift across the city skyline at dusk",
                    "queries": ["city skyline aerial"],
                    "k": 12,
                    "hero": false,
                    "tag_weight": 0.3,
                    "motion_min": 1.5,
                    "exclude_ids": [820_338_756_299i64],
                }),
            ),
            (
                "footage_continuity",
                json!({ "clips": [
                    { "slotId": "hook.1", "clipId": "clip.1", "colorTemperature": 0.3, "motion": 2.0, "dominantDirection": "right", "shotScale": "wide", "category": "city", "motionMin": 1.0 },
                    { "slotId": "setup.1", "clipId": "clip.2", "colorTemperature": 0.1, "motion": 2.5, "dominantDirection": "right", "shotScale": "close_up", "category": "ocean", "motionMin": 1.0 },
                ] }),
            ),
        ]
    }

    #[test]
    fn every_tool_has_a_schema_and_a_sample_and_names_are_unique() {
        let toolbox = Toolbox::new();
        let listed = toolbox.list();
        let samples = samples();
        assert_eq!(listed.len(), samples.len(), "a sample for every tool");
        let mut names: Vec<&str> = listed
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), listed.len(), "unique names");
        let ordered_names: Vec<&str> = listed
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        let timeline = ordered_names
            .iter()
            .position(|name| *name == "timeline_read")
            .expect("timeline_read is listed");
        assert_eq!(ordered_names[timeline + 1], "plan_validate");
        assert_eq!(ordered_names[timeline + 2], "clip_audio_levels_db");
        for tool in &listed {
            let name = tool["name"].as_str().expect("named");
            assert!(
                name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{name}"
            );
            assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
            assert!(
                tool["description"]
                    .as_str()
                    .is_some_and(|text| text.len() > 40),
                "{name}"
            );
        }
    }

    /// The round trip: what each tool's schema accepts is what its Rust
    /// type reads, and what the Rust type writes the schema accepts.
    #[test]
    fn every_sample_fits_its_schema_and_reads_as_its_request() {
        let toolbox = Toolbox::new();
        for (name, sample) in samples() {
            let (tool, schema) = toolbox.find(name).unwrap_or_else(|| panic!("no {name}"));
            check(schema, &sample, schema, name).unwrap_or_else(|why| panic!("{why}"));
            let built = match tool.action {
                Action::Method { method, .. } => request(method, sample.clone()),
                Action::Command { op } => edit(op, sample.clone()),
                Action::Version => request("version", sample.clone()),
                Action::TimelineRead => request("project.get", sample.clone()),
                Action::Captions => captions(sample.clone()),
                Action::AudioLevelsDb => audio_levels_db(sample.clone()),
                Action::PlanValidate => {
                    validate_plan_tool(sample.clone()).expect("valid plan sample");
                    continue;
                }
                Action::ExportWait => {
                    let mut sample = sample.clone();
                    sample
                        .as_object_mut()
                        .map(|fields| fields.remove("timeoutSeconds"));
                    request("export.status", sample)
                }
                Action::PreviewFrame => request("preview.frame", sample.clone()),
                Action::Mask => mask(sample.clone()),
                // The motion tools wrap `concat-motion`, not the API: the
                // two that need no engine and no project run for real,
                // and the three that bake have had their arguments checked
                // against their schemas above - their work needs the
                // sidecar and a project, which a unit test has not.
                Action::MotionPresets => {
                    crate::motion::presets_list().expect("the library lists");
                    continue;
                }
                Action::MotionInspect => {
                    crate::motion::inspect(sample.clone()).expect("the sample inspects");
                    continue;
                }
                Action::SceneAnalysis => {
                    let outcome =
                        crate::motion::scene_analysis(sample.clone()).expect("the sample analyses");
                    // The analysis answers its four numbers and the lint,
                    // all off the document: the census, the timing, the
                    // text, and findings with the lint's own codes.
                    let said = outcome.structured.expect("the analysis is JSON");
                    let analysis = &said["analysis"];
                    assert!(analysis["census"]["nodes"].as_u64().is_some(), "{said}");
                    assert!(analysis["timing"].is_object(), "{said}");
                    assert!(analysis["text"].is_object(), "{said}");
                    assert!(
                        analysis["lint"]
                            .as_array()
                            .is_some_and(|lint| !lint.is_empty()),
                        "{said}"
                    );
                    continue;
                }
                Action::MotionFarmProgress => {
                    let outcome =
                        crate::motion::farm_progress(sample.clone()).expect("the sample reads");
                    // A farm that has said nothing answers a whole farm
                    // pending, live, with no out - the honest shape the
                    // sample can check without a render to watch.
                    assert!(outcome.structured.is_some(), "the progress is JSON");
                    let said = outcome.structured.unwrap();
                    assert_eq!(said["live"], serde_json::json!(true), "{said}");
                    assert!(said.get("out").is_some_and(|out| out.is_null()), "{said}");
                    continue;
                }
                Action::MotionFarmCloud => {
                    // The cloud's honest refusal: the script's ok:false
                    // comes back through run_py as an ApiError, which
                    // the protocol answers as isError naming the
                    // environment to set. A machine with the environment
                    // configured answers the status shape instead; both
                    // are the tool working, so the arm accepts either.
                    match crate::motion::farm_cloud(sample.clone()) {
                        Ok(outcome) => {
                            let said = outcome.structured.expect("the cloud answers JSON");
                            assert!(said["ok"].as_bool().unwrap_or(false), "{said}");
                        }
                        Err(error) => {
                            let text = error.to_string();
                            assert!(
                                text.contains("FUNCUT_FARM_BUCKET"),
                                "the refusal names the environment it wants: {text}"
                            );
                        }
                    }
                    continue;
                }
                Action::MotionFrame
                | Action::MotionPreview
                | Action::MotionBake
                | Action::CaptionsAnimate
                | Action::MotionReel
                | Action::MediaProbe
                | Action::SceneTimeline
                | Action::GenevaExplain => {
                    continue;
                }
                // The sense tools run Python, ffmpeg or a project; a unit
                // test has none of those. Their arguments have already been
                // checked against their schemas above.
                Action::WatchRender
                | Action::StockSheet
                | Action::CraftCritique
                | Action::LocalVlm
                | Action::Emotion
                | Action::VoiceSignals
                | Action::Sense(_)
                | Action::SenseFamily(_)
                | Action::CreativeBrief
                | Action::MotionOverlay
                | Action::MeasureLoudness
                | Action::SuggestCuts
                | Action::TrackSubject => continue,
                Action::CollisionCheck => request("project.get", sample.clone()),
                Action::OpenvidSearchBatch => {
                    // One `openvid.search` per query: the first query's
                    // request stands for the batch, which builds exactly
                    // that call for each entry.
                    let mut sample = sample.clone();
                    if let Some(fields) = sample.as_object_mut() {
                        let query = fields
                            .get("queries")
                            .and_then(Value::as_array)
                            .and_then(|queries| queries.first().cloned())
                            .unwrap_or(Value::Null);
                        fields.remove("queries");
                        fields.insert("query".to_owned(), query);
                    }
                    request("openvid.search", sample)
                }
                Action::OpenvidRankForSlot => {
                    // The page the ranker shops: the description is the
                    // query, relevance the sort, the pool the width, and
                    // the hard filters ride under their API names - the
                    // blend, the k and the hero flag are the reply's
                    // business, not the search's.
                    let mut fields = sample.as_object().cloned().unwrap_or_default();
                    let query = fields.remove("description").unwrap_or(Value::Null);
                    let min_motion = fields.remove("motion_min");
                    let exclude_ids = fields.remove("exclude_ids");
                    let mut search = json!({
                        "query": query,
                        "sort": "relevance",
                        "limit": RANK_POOL,
                    });
                    if let Some(min) = min_motion {
                        search["minMotion"] = min;
                    }
                    if let Some(ids) = exclude_ids {
                        search["excludeIds"] = ids;
                    }
                    request("openvid.search", search)
                }
                Action::FootageContinuity => {
                    // The gate is arithmetic over the manifest's own
                    // numbers, so the sample runs for real - and the
                    // sample is clean: two slots, varied in everything
                    // the gate reads.
                    let manifest: concat_motion::continuity::AssetManifest =
                        serde_json::from_value(sample).expect("the sample reads");
                    let flags = concat_motion::continuity::check_continuity(&manifest);
                    assert!(flags.is_empty(), "{flags:?}");
                    continue;
                }
            };
            let built = built.unwrap_or_else(|error| panic!("{name}: {error}"));
            // Back again: the request as the API writes it fits the API's
            // own schema for requests.
            let written = serde_json::to_value(&built).expect("writes");
            let root = concat_api::request_schema();
            check(&root, &written, &root, name).unwrap_or_else(|why| panic!("{why}"));
        }
    }

    /// A backend whose export reports the statuses it is given, in order,
    /// the last for ever.
    struct Statuses(std::collections::VecDeque<Value>);

    impl Backend for Statuses {
        fn call(&mut self, _request: Request) -> Result<Value, ApiError> {
            let status = if self.0.len() > 1 {
                self.0.pop_front()
            } else {
                self.0.front().cloned()
            };
            Ok(status.expect("a status"))
        }

        fn describe(&self) -> Value {
            json!({ "mode": "test" })
        }
    }

    /// A timeline where two clips share a lane reads as the collision it
    /// is, touching does not count, and a lane of its own never does.
    #[test]
    fn collision_check_names_the_lane_two_clips_share() {
        let view = json!({
            "project": { "name": "P", "width": 1920, "height": 1080, "fps": 30.0 },
            "timeline": {
                "id": "tl1", "name": "One", "duration": 10.0,
                "tracks": [{ "id": "t1", "index": 0 }, { "id": "t2", "index": 1 }],
                "clips": [
                    { "id": "c1", "kind": "video", "track": "t1", "start": 0.0, "duration": 5.0, "end": 5.0 },
                    { "id": "c2", "kind": "video", "track": "t1", "start": 4.0, "duration": 4.0, "end": 8.0 },
                    { "id": "c3", "kind": "video", "track": "t1", "start": 8.0, "duration": 2.0, "end": 10.0 },
                    { "id": "c4", "kind": "text", "track": "t2", "start": 1.0, "duration": 2.0, "end": 3.0 },
                ],
            },
            "media": [],
        });
        let outcome = collision_report(view);
        assert!(!outcome.is_error);
        let reply = outcome.structured.expect("structured");
        assert_eq!(reply["count"], 1);
        assert_eq!(reply["clean"], false);
        let collisions = reply["collisions"].as_array().expect("collisions");
        assert_eq!(collisions.len(), 1);
        let collision = &collisions[0];
        assert_eq!(collision["trackId"], "t1");
        assert_eq!(collision["from"], 4.0);
        assert_eq!(collision["to"], 5.0);
        let ids: Vec<&str> = collision["clips"]
            .as_array()
            .expect("two clips")
            .iter()
            .filter_map(|clip| clip["id"].as_str())
            .collect();
        assert_eq!(ids, ["c1", "c2"], "c3 only touches, c4 has its own lane");
    }

    /// A clean timeline reads as clean.
    #[test]
    fn a_timeline_without_shared_lanes_reads_clean() {
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "kind": "video", "track": "t1", "start": 0.0, "duration": 5.0, "end": 5.0 },
                { "id": "c2", "kind": "video", "track": "t1", "start": 5.0, "duration": 2.0, "end": 7.0 },
            ] },
        });
        let outcome = collision_report(view);
        let reply = outcome.structured.expect("structured");
        assert_eq!(reply["clean"], true);
        assert_eq!(reply["count"], 0);
        assert_eq!(reply["collisions"].as_array().map(Vec::len), Some(0));
    }

    /// A backend whose calls are answered in order, and remembered.
    struct Answers(
        std::cell::RefCell<Vec<Value>>,
        std::collections::VecDeque<Value>,
    );

    impl Backend for Answers {
        fn call(&mut self, request: Request) -> Result<Value, ApiError> {
            self.0
                .borrow_mut()
                .push(serde_json::to_value(&request).expect("writes"));
            let answer = self.1.pop_front().expect("an answer");
            Ok(answer)
        }

        fn describe(&self) -> Value {
            json!({ "mode": "test" })
        }
    }

    /// `openvid_rank_for_slot` refuses to shop for a slot that cannot say
    /// what it wants: an empty, whitespace or absent description is an
    /// error naming itself, not a quiet fallback to the queries.
    #[test]
    fn an_empty_description_is_an_error_not_a_fallback() {
        let mut backend = Answers(
            std::cell::RefCell::new(Vec::new()),
            [json!({ "clips": [], "total": 0 })].into(),
        );
        for description in ["", "   ", "\n\t "] {
            let error = match openvid_rank_for_slot(
                json!({ "description": description, "queries": ["city skyline"] }),
                &mut backend,
            ) {
                Err(error) => error,
                Ok(_) => panic!("an empty description refuses"),
            };
            assert!(error.to_string().contains("description"), "{error}");
        }
        assert!(backend.0.borrow().is_empty(), "no search ever ran");
        let error =
            match openvid_rank_for_slot(json!({ "queries": ["city skyline"] }), &mut backend) {
                Err(error) => error,
                Ok(_) => panic!("no description at all refuses"),
            };
        assert!(error.to_string().contains("description"), "{error}");
    }

    /// A slot that names the clips it has already used never sees them
    /// again: the filter rides the search request, and what the reply
    /// carries anyway is dropped before the ranking sees it. A clip under
    /// the motion floor is dropped the same way.
    #[test]
    fn a_slot_with_excluded_ids_never_returns_an_excluded_clip() {
        let mut backend = Answers(
            std::cell::RefCell::new(Vec::new()),
            [json!({ "total": 3, "clips": [
                { "id": 1, "caption": "a city skyline at dusk", "motion": 2.0 },
                { "id": 2, "caption": "waves on a beach", "motion": 3.0 },
                { "id": 3, "caption": "a city street at night", "motion": 0.5 },
            ] })]
            .into(),
        );
        let outcome = openvid_rank_for_slot(
            json!({
                "description": "a slow aerial drift across the city skyline at dusk",
                "queries": ["city skyline aerial"],
                "exclude_ids": [2],
                "motion_min": 1.5,
            }),
            &mut backend,
        )
        .expect("ranks");
        assert!(!outcome.is_error);
        let reply = outcome.structured.expect("structured");
        let clips = reply["clips"].as_array().expect("clips");
        let ids: Vec<i64> = clips
            .iter()
            .filter_map(|clip| clip["id"].as_i64())
            .collect();
        assert_eq!(
            ids,
            vec![1],
            "the used id and the low-motion id are gone: {clips:?}"
        );

        // And the hard filters rode the request, so a real index does the
        // work before the ranking too.
        let calls = backend.0.borrow();
        assert_eq!(calls[0]["method"], "openvid.search");
        assert_eq!(
            calls[0]["excludeIds"],
            json!([2]),
            "the used ids ride the request"
        );
        assert_eq!(calls[0]["minMotion"], 1.5, "the floor rides the request");
        assert_eq!(
            calls[0]["query"],
            "a slow aerial drift across the city skyline at dusk"
        );
        assert_eq!(calls[0]["sort"], "relevance");
        assert_eq!(calls[0]["limit"], json!(RANK_POOL));
    }

    /// A hero slot asks for thirty, any other slot asks for twelve, and
    /// an explicit k is the caller's own. The reply says which it used.
    #[test]
    fn hero_widens_the_default_k() {
        assert_eq!(slot_k(None, true), 30);
        assert_eq!(slot_k(None, false), 12);
        assert_eq!(slot_k(Some(5), true), 5);
        assert_eq!(slot_k(Some(5), false), 5);

        let clips: Vec<Value> = (1..=6)
            .map(|id| {
                json!({ "id": id, "caption": format!("a city skyline shot {id}"), "motion": 2.0 })
            })
            .collect();
        for (hero, expected) in [(true, 30), (false, 12)] {
            let mut backend = Answers(
                std::cell::RefCell::new(Vec::new()),
                [json!({ "total": 6, "clips": clips.clone() })].into(),
            );
            let outcome = openvid_rank_for_slot(
                json!({ "description": "a city skyline at dusk", "hero": hero }),
                &mut backend,
            )
            .expect("ranks");
            let reply = outcome.structured.expect("structured");
            assert_eq!(
                reply["k"],
                json!(expected),
                "the reply says the page a hero asks for"
            );
            assert_eq!(
                reply["clips"].as_array().map(Vec::len),
                Some(6),
                "a six-clip page is all of it, at either k"
            );
        }

        // An explicit k wins over the hero default.
        let mut backend = Answers(
            std::cell::RefCell::new(Vec::new()),
            [json!({ "total": 6, "clips": clips })].into(),
        );
        let outcome = openvid_rank_for_slot(
            json!({ "description": "a city skyline at dusk", "hero": true, "k": 5 }),
            &mut backend,
        )
        .expect("ranks");
        let reply = outcome.structured.expect("structured");
        assert_eq!(reply["k"], json!(5));
        assert_eq!(reply["clips"].as_array().map(Vec::len), Some(5));
    }

    /// The returned list is ordered by score, numbered by rank, and the
    /// tags moved something: a caption carrying the slot's terms outscores
    /// an equal-position caption sharing none of them.
    #[test]
    fn the_returned_list_is_ordered_by_score() {
        let clips = vec![
            json!({ "id": 1, "caption": "a random assortment of words", "motion": 2.0 }),
            json!({ "id": 2, "caption": "a city skyline at dusk", "motion": 2.0 }),
            json!({ "id": 3, "caption": "something else entirely", "motion": 2.0 }),
        ];
        let mut backend = Answers(
            std::cell::RefCell::new(Vec::new()),
            [json!({ "total": 3, "clips": clips })].into(),
        );
        let outcome = openvid_rank_for_slot(
            json!({
                "description": "a city skyline at dusk",
                "tag_weight": 0.5,
            }),
            &mut backend,
        )
        .expect("ranks");
        let reply = outcome.structured.expect("structured");
        let clips = reply["clips"].as_array().expect("clips");
        let scores: Vec<f64> = clips
            .iter()
            .filter_map(|clip| clip["slotScore"].as_f64())
            .collect();
        assert_eq!(scores.len(), 3, "every clip carries its score");
        for pair in scores.windows(2) {
            assert!(pair[0] >= pair[1], "the scores only go down: {scores:?}");
        }
        let ranks: Vec<u64> = clips
            .iter()
            .filter_map(|clip| clip["slotRank"].as_u64())
            .collect();
        assert_eq!(ranks, vec![1u64, 2, 3], "{clips:?}");
        // The tag blend moved the agreeing clip to the front: the words
        // count, not just the page order.
        assert_eq!(clips[0]["id"], json!(2), "{clips:?}");
    }

    /// The tags are their own arithmetic: terms drop the glue words, and
    /// a caption carrying every term is a perfect agreement.
    #[test]
    fn the_tags_score_the_words_the_slot_asked_for() {
        let terms = slot_terms(
            "a city skyline at dusk",
            &["city skyline aerial".to_owned(), "the".to_owned()],
        );
        assert_eq!(
            terms,
            vec!["city", "skyline", "dusk", "aerial"],
            "{terms:?}"
        );
        // Three of the four terms the caption carries - `aerial` is the
        // query's ask, not the caption's words - is half the union.
        assert_eq!(tag_similarity(&terms, "a city skyline at dusk"), 0.5);
        assert!(tag_similarity(&terms, "waves on a beach") < 0.2);
        assert_eq!(tag_similarity(&[], "anything"), 0.0);
    }

    /// The continuity gate, as the tool: a warm-to-cool pair and a
    /// wide-to-wide pair come back as flags carrying slots, rules and
    /// remedies, a clean manifest answers an empty list, and a manifest
    /// that does not read is refused with words.
    #[test]
    fn footage_continuity_flags_the_jumps_the_cut_will_feel() {
        let outcome = footage_continuity(json!({ "clips": [
            { "slotId": "a", "clipId": "clip.a", "colorTemperature": 0.8, "motion": 2.0, "dominantDirection": "static", "shotScale": "wide", "category": "city", "motionMin": 0.0 },
            { "slotId": "b", "clipId": "clip.b", "colorTemperature": -0.6, "motion": 2.0, "dominantDirection": "static", "shotScale": "wide", "category": "ocean", "motionMin": 0.0 },
        ] }))
        .expect("flags");
        assert!(!outcome.is_error);
        let reply = outcome.structured.expect("structured");
        assert_eq!(reply["count"], 1, "{reply}");
        assert_eq!(reply["flags"][0]["slotId"], "b");
        assert_eq!(reply["flags"][0]["rule"], "colour");
        assert_eq!(
            reply["flags"][0]["remedy"],
            json!({ "replace": { "avoid": "a clip cooler than slot `a`'s" } })
        );

        // Two wides in a row: the scale rule, the second slot's name.
        let outcome = footage_continuity(json!({ "clips": [
            { "slotId": "a", "clipId": "clip.a", "shotScale": "wide", "category": "city" },
            { "slotId": "b", "clipId": "clip.b", "shotScale": "wide", "category": "ocean" },
        ] }))
        .expect("flags");
        let reply = outcome.structured.expect("structured");
        assert_eq!(reply["count"], 1, "{reply}");
        assert_eq!(reply["flags"][0]["rule"], "scale");

        // Clean: two varied slots answer nothing.
        let outcome = footage_continuity(json!({ "clips": [
            { "slotId": "a", "clipId": "clip.a", "colorTemperature": 0.3, "motion": 2.0, "dominantDirection": "right", "shotScale": "wide", "category": "city", "motionMin": 1.0 },
            { "slotId": "b", "clipId": "clip.b", "colorTemperature": 0.1, "motion": 2.5, "dominantDirection": "right", "shotScale": "close_up", "category": "ocean", "motionMin": 1.0 },
        ] }))
        .expect("clean");
        let reply = outcome.structured.expect("structured");
        assert_eq!(reply["count"], 0, "{reply}");
        assert_eq!(reply["flags"], json!([]));

        // And a manifest that does not read is refused, not guessed at.
        let error = match footage_continuity(json!({ "clips": [{ "slotId": 3 }] })) {
            Err(error) => error,
            Ok(_) => panic!("a manifest that does not read refuses"),
        };
        assert!(error.to_string().contains("does not read"), "{error}");
    }

    /// The batch runs one `openvid.search` per query, carrying the shared
    /// filters, and the reply keeps the queries apart. The CLIP rerank
    /// rides along - these pages are tiny, so it is a no-op here, and the
    /// flag is consumed by the batch and never forwarded to the API.
    #[test]
    fn openvid_search_batch_runs_one_search_per_query() {
        let mut backend = Answers(
            std::cell::RefCell::new(Vec::new()),
            [
                json!({ "clips": [{ "id": 1 }], "total": 1 }),
                json!({ "clips": [{ "id": 2 }, { "id": 3 }], "total": 2 }),
                json!({ "clips": [{ "id": 9 }], "total": 1 }),
            ]
            .into(),
        );
        let outcome = openvid_search_batch(
            json!({ "queries": ["ocean sunset", "city skyline"], "limit": 5 }),
            &mut backend,
        )
        .expect("searches");
        assert!(!outcome.is_error);
        let reply = outcome.structured.expect("structured");
        let searches = reply["searches"].as_array().expect("searches");
        assert_eq!(searches.len(), 2);
        assert_eq!(searches[0]["query"], "ocean sunset");
        assert_eq!(searches[0]["total"], 1);
        assert_eq!(searches[0]["clips"].as_array().map(Vec::len), Some(1));
        assert_eq!(searches[1]["query"], "city skyline");
        assert_eq!(searches[1]["total"], 2);
        let calls = backend.0.borrow();
        assert_eq!(calls.len(), 2, "one call a query");
        assert_eq!(calls[0]["method"], "openvid.search");
        assert_eq!(calls[0]["query"], "ocean sunset");
        assert_eq!(calls[0]["limit"], 5, "the shared filters ride along");
        assert_eq!(calls[1]["query"], "city skyline");
        assert!(
            calls[0].get("queries").is_none(),
            "the list is not forwarded"
        );
        assert!(
            calls[0].get("rerank").is_none(),
            "the batch's own flag is not forwarded"
        );
    }

    /// `rerank: false` is the fast sweep: the same searches, the BM25
    /// order, and the flag still never reaching the API method.
    #[test]
    fn the_batch_can_skip_the_clip_rerank_for_a_fast_sweep() {
        let mut backend = Answers(
            std::cell::RefCell::new(Vec::new()),
            [json!({ "clips": [{ "id": 7 }], "total": 1 })].into(),
        );
        let outcome = openvid_search_batch(
            json!({ "queries": ["harbour dawn"], "rerank": false }),
            &mut backend,
        )
        .expect("searches");
        let reply = outcome.structured.expect("structured");
        assert_eq!(reply["searches"][0]["query"], "harbour dawn");
        assert_eq!(reply["searches"][0]["total"], 1);
        let calls = backend.0.borrow();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].get("rerank").is_none());
    }

    /// A key the schema does not name is refused at the door, not dropped
    /// on the way to a half-read edit.
    #[test]
    fn a_misspelled_argument_is_refused_rather_than_dropped() {
        let toolbox = Toolbox::new();
        let mut backend = Statuses(std::collections::VecDeque::new());
        let cancelled = AtomicBool::new(false);
        let outcome = toolbox
            .call(
                "title_add",
                json!({ "path": "/tmp/Cut", "start": 1.0, "style2": { "text": "Hello" } }),
                Context {
                    backend: &mut backend,
                    cancelled: &cancelled,
                    progress: None,
                },
            )
            .expect("the tool exists");
        assert!(outcome.is_error, "the misspelling is refused");
        let text = outcome.content[0]["text"].as_str().expect("says why");
        assert!(text.contains("style2"), "names the key: {text}");
        assert!(backend.0.is_empty(), "nothing reached the backend");
    }

    /// An export that mixes sound and muxes it reports the stages the
    /// exporter names - "rendering", "mixing audio", "muxing" - and the
    /// wait's progress climbs through them to the end.
    #[test]
    fn export_wait_progress_follows_the_stages_the_exporter_reports() {
        let running = |stage: &str, frame: i64, total: i64| {
            json!({ "state": "running", "stage": stage, "frame": frame, "total": total,
                    "fraction": if total > 0 { frame as f64 / total as f64 } else { 0.0 } })
        };
        let mut backend = Statuses(
            [
                running("rendering", 50, 100),
                running("mixing audio", 0, 100),
                running("muxing", 100, 100),
                json!({ "state": "done", "stage": "muxing", "frame": 100, "total": 100, "fraction": 1.0 }),
            ]
            .into(),
        );
        let seen = std::cell::RefCell::new(Vec::new());
        let report =
            |progress: f64, total: f64, _: String| seen.borrow_mut().push((progress, total));
        let cancelled = AtomicBool::new(false);
        let outcome = export_wait(
            json!({ "job": "j1" }),
            &mut backend,
            &cancelled,
            Some(&report),
        )
        .expect("waits");
        assert!(!outcome.is_error);
        assert_eq!(
            *seen.borrow(),
            vec![(50.0, 100.0), (60.0, 100.0), (99.9, 100.0), (100.0, 100.0)]
        );
    }

    #[test]
    fn decibel_audio_tool_converts_absolute_targets_to_linear_key_values() {
        let request = audio_levels_db(json!({
            "path": "/p",
            "clipId": "c1",
            "baseDb": -4.0,
            "replaceAutomation": true,
            "keyframes": [{ "at": 0.5, "gainDb": -12.0 }]
        }))
        .expect("valid dB levels");
        let request = serde_json::to_value(request).expect("serializes");
        let commands = request.pointer("/command/commands").expect("batch");
        let base = commands[1]["patch"]["volume"].as_f64().expect("base gain");
        let duck = commands[2]["value"].as_f64().expect("relative key gain");
        assert!((base - 10.0_f64.powf(-4.0 / 20.0)).abs() < 1e-12);
        assert!((duck - 10.0_f64.powf(-12.0 / 20.0)).abs() < 1e-12);
        assert_eq!(commands[0]["op"], "clearClipKeys");
        assert_eq!(commands[2]["property"], "volume");
    }

    #[test]
    fn d_b_audio_levels_accept_absolute_automation_and_reject_invalid_ranges() {
        assert!(
            audio_levels_db(json!({
                "path": "/p", "clipId": "c1", "baseDb": -60.0,
                "keyframes": [{ "at": 0.5, "gainDb": -12.0 }]
            }))
            .is_ok()
        );
        assert!(
            audio_levels_db(json!({
                "path": "/p", "clipId": "c1", "baseDb": 25.0
            }))
            .is_err()
        );
        // Two keys a hair apart are a replace, not a refusal - the rule
        // `keyframe_set` applies, so re-sending a duck curve never errors
        // on its own keys; the second key wins in the same batch.
        let replaced = audio_levels_db(json!({
            "path": "/p", "clipId": "c1", "baseDb": -4.0,
            "keyframes": [
                { "at": 0.5, "gainDb": -12.0 },
                { "at": 0.501, "gainDb": -8.0 }
            ]
        }))
        .expect("a hair apart is a replace");
        let whole = serde_json::to_value(replaced).expect("JSON");
        assert_eq!(whole["command"]["op"], "batch");
        assert!(
            audio_levels_db(json!({
                "path": "/p", "clipId": "c1", "baseDb": -4.0,
                "keyframes": [{ "at": 0.5, "gainDb": -12.0, "ease": [0.2, 0.3] }]
            }))
            .is_err()
        );
        assert_eq!(
            gain_from_db(-60.0),
            0.0,
            "-60 dB is the editor's silence floor"
        );
        assert!((gain_from_db(-59.99) - 10.0_f64.powf(-59.99 / 20.0)).abs() < 1e-12);
    }

    #[test]
    fn director_example_fits_the_tool_schema_and_validates() {
        let schema = plan_validate_schema().expect("canonical plan schema");
        let plan: Value = serde_json::from_str(include_str!(
            "../../../../.claude/skills/funcut-director/templates/edit_plan.example.json"
        ))
        .expect("valid example JSON");
        let arguments = json!({ "plan": plan });
        crate::schema::check(&schema, &arguments, &schema, "plan_validate")
            .expect("example fits tool schema");
        let result = validate_plan_tool(arguments).expect("example passes validation");
        assert_eq!(result["valid"], true);
        assert_eq!(result["segmentCount"], 5);
        assert_eq!(result["runtimeSeconds"], 24.0);
    }

    #[test]
    fn plan_validator_rejects_gaps_and_unsupported_word_sync() {
        let base = json!({
            "aspect": "9:16",
            "audio": {},
            "segments": [
                { "start": 0.0, "end": 2.0, "visual": { "type": "broll", "query": "ocean" } },
                { "start": 2.2, "end": 4.0, "visual": { "type": "textCard", "text": "SALT" } }
            ]
        });
        assert!(validate_edit_plan(&base).unwrap_err().contains("gap"));
        let mut no_gap = base;
        no_gap["segments"][1]["start"] = json!(2.0);
        no_gap["captions"] = json!({ "wordSynced": true });
        assert!(
            validate_edit_plan(&no_gap)
                .unwrap_err()
                .contains("captions_animate")
        );
    }

    /// A plan's slots are validated with its segments: the description is
    /// what CLIP ranks against and cannot be a keyword in disguise, and
    /// the treatments keep their own queries law - b-roll shops, the
    /// drawn treatments do not.
    #[test]
    fn plan_validator_rejects_a_slot_with_a_thin_description() {
        let slot = |slot_id: &str, treatment: &str, queries: Value, description: &str| {
            json!({
                "slotId": slot_id,
                "role": "setup",
                "treatment": treatment,
                "job": "illustrate",
                "entry": "on_emphasis",
                "exit": "on_beat",
                "description": description,
                "queries": queries,
                "hero": false,
                "motionMin": 1.0,
                "durationS": 2.5
            })
        };
        let base = json!({
            "aspect": "9:16",
            "audio": {},
            "segments": [
                { "start": 0.0, "end": 2.0, "visual": { "type": "broll", "query": "ocean" } },
                { "start": 2.0, "end": 4.0, "visual": { "type": "textCard", "text": "SALT" } }
            ]
        });

        // An empty description is refused, and so is a keyword wearing a
        // description's clothes.
        let mut thin = base.clone();
        thin["slots"] = json!([slot("hook.1", "broll", json!(["city"]), "city dusk")]);
        assert!(
            validate_edit_plan(&thin)
                .unwrap_err()
                .contains("at least 40")
        );
        let mut empty = base.clone();
        empty["slots"] = json!([slot("hook.1", "broll", json!(["city"]), "")]);
        assert!(
            validate_edit_plan(&empty)
                .unwrap_err()
                .contains("0 characters")
        );

        // A drawn treatment carrying search queries is refused: the edit
        // draws it itself.
        let mut drawn = base.clone();
        drawn["slots"] = json!([slot(
            "hook.1",
            "hold",
            json!(["city skyline"]),
            "a slow aerial drift across the city skyline at dusk, camera gliding left"
        )]);
        assert!(
            validate_edit_plan(&drawn)
                .unwrap_err()
                .contains("no search queries")
        );
        // And b-roll with nothing to search for is refused.
        let mut starving = base.clone();
        starving["slots"] = json!([slot(
            "hook.1",
            "broll",
            json!([]),
            "a slow aerial drift across the city skyline at dusk, camera gliding left"
        )]);
        assert!(
            validate_edit_plan(&starving)
                .unwrap_err()
                .contains("at least one non-empty query")
        );

        // Slots that keep their own contract pass clean: a b-roll and a
        // hold together - half the share, inside the band, and the hold
        // carries no queries.
        let mut valid = base.clone();
        valid["slots"] = json!([
            slot(
                "hook.1",
                "broll",
                json!(["city skyline aerial"]),
                "a slow aerial drift across the city skyline at dusk, camera gliding left"
            ),
            slot(
                "setup.1",
                "hold",
                json!([]),
                "the host's hands turn the page of a leather notebook on a wooden desk"
            ),
        ]);
        assert!(validate_edit_plan(&valid).is_ok(), "valid slots pass");
        // And a plan without slots is a plan the segments alone describe.
        assert!(validate_edit_plan(&base).is_ok(), "no slots, no slot gate");
    }

    /// The b-roll share keeps the plan inside the 30-45% band: past 60%
    /// the plan is refused outright, 45% and under-30% speak up as
    /// warnings, and a plan shopping 35% passes without a word.
    #[test]
    fn plan_validator_keeps_the_broll_share_inside_the_band() {
        let slot = |index: usize, treatment: &str| {
            json!({
                "slotId": format!("slot.{index}"),
                "role": "setup",
                "treatment": treatment,
                "job": "illustrate",
                "entry": "on_emphasis",
                "exit": "on_beat",
                "description": "a slow aerial drift across the city skyline at dusk, camera gliding left",
                "queries": if treatment == "broll" { json!(["city skyline aerial"]) } else { json!([]) },
                "hero": false,
                "motionMin": 1.0,
                "durationS": 2.5
            })
        };
        let base = json!({
            "aspect": "9:16",
            "audio": {},
            "segments": [
                { "start": 0.0, "end": 2.0, "visual": { "type": "broll", "query": "ocean" } },
                { "start": 2.0, "end": 4.0, "visual": { "type": "textCard", "text": "SALT" } }
            ]
        });
        let slots = |broll: usize, total: usize| -> Vec<Value> {
            (0..total)
                .map(|index| {
                    if index < broll {
                        slot(index, "broll")
                    } else {
                        slot(index, "hold")
                    }
                })
                .collect()
        };

        // Seventy percent stock is stock with gaps: refused.
        let mut shopping = base.clone();
        shopping["slots"] = Value::Array(slots(7, 10));
        assert!(
            validate_edit_plan(&shopping)
                .unwrap_err()
                .contains("past 60%")
        );

        // Fifty percent speaks up as a warning, not a refusal.
        let mut leaning = base.clone();
        leaning["slots"] = Value::Array(slots(5, 10));
        let result = validate_edit_plan(&leaning).expect("a warning, not a refusal");
        let warnings = result["warnings"].as_array().expect("warnings");
        assert!(
            warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|text| text.contains("covers 50%"))),
            "{warnings:?}"
        );

        // Thirty-five percent is inside the band: no share warning.
        let mut coverage = base.clone();
        coverage["slots"] = Value::Array(slots(7, 20));
        let result = validate_edit_plan(&coverage).expect("a valid plan");
        let warnings = result["warnings"].as_array().expect("warnings");
        assert!(
            warnings.iter().all(|warning| warning
                .as_str()
                .is_none_or(|text| !text.contains("b-roll covers"))),
            "35% is coverage: {warnings:?}"
        );

        // Under thirty percent, the picture leans on the A-roll alone.
        let mut a_roll = base.clone();
        a_roll["slots"] = Value::Array(slots(2, 10));
        let result = validate_edit_plan(&a_roll).expect("a warning, not a refusal");
        let warnings = result["warnings"].as_array().expect("warnings");
        assert!(
            warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|text| text.contains("covers 20%"))),
            "{warnings:?}"
        );
    }

    /// A director plan's structure is checked, not just its timing: the
    /// hook leads, sections cover the runtime in order, a reel stays a
    /// reel, and the creative floors speak up as warnings while the plan
    /// is still a plan.
    #[test]
    fn the_plan_validator_directs() {
        let cut = |start: f64, end: f64, cut_type: &str, entrance: bool| {
            json!({
                "start": start,
                "end": end,
                "visual": { "type": "broll", "query": "ocean" },
                "cutType": cut_type,
                "in": if entrance { json!({ "anim": "pop", "seconds": 0.15 }) } else { Value::Null },
            })
        };
        let base = json!({
            "aspect": "9:16",
            "mode": "reel",
            "audio": {},
            "grade": { "filter": "concat.teal-orange", "intensityKey": "amount", "intensity": 65 },
            "sections": [
                { "role": "hook", "start": 0.0, "end": 2.0 },
                { "role": "body", "start": 2.0, "end": 8.0 },
                { "role": "cta", "start": 8.0, "end": 10.0 }
            ],
            "segments": [
                cut(0.0, 2.0, "hard", true),
                cut(2.0, 3.5, "hard", true),
                cut(3.5, 5.0, "hard", true),
                cut(5.0, 6.5, "hard", true),
                cut(6.5, 8.0, "hard", true),
                cut(8.0, 10.0, "hard", true)
            ]
        });
        let result = validate_edit_plan(&base).expect("a valid plan");
        let warnings = result["warnings"].as_array().expect("warnings");
        assert!(
            warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|text| text.contains("hard cuts in a row"))),
            "six hards is a slideshow: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.as_str().is_some_and(|text| text.contains("comp"))),
            "no designed comps anywhere: {warnings:?}"
        );

        // A reel that is not a reel is refused outright.
        let mut long = base.clone();
        long["segments"] = json!([cut(0.0, 70.0, "beat", true)]);
        long["sections"] = json!([{ "role": "hook", "start": 0.0, "end": 70.0 }]);
        assert!(validate_edit_plan(&long).unwrap_err().contains("longform"));

        // Sections must start with the hook and cover the runtime, in
        // order, without slipping off the cuts.
        let mut starts_with_body = base.clone();
        starts_with_body["sections"] = json!([
            { "role": "body", "start": 0.0, "end": 10.0 }
        ]);
        assert!(
            validate_edit_plan(&starts_with_body)
                .unwrap_err()
                .contains("hook")
        );
        let mut short_cover = base.clone();
        short_cover["sections"] = json!([
            { "role": "hook", "start": 0.0, "end": 2.0 },
            { "role": "body", "start": 2.0, "end": 6.0 }
        ]);
        assert!(
            validate_edit_plan(&short_cover)
                .unwrap_err()
                .contains("cover the whole piece")
        );
        let mut off_the_cut = base.clone();
        off_the_cut["sections"] = json!([
            { "role": "hook", "start": 0.0, "end": 2.5 },
            { "role": "body", "start": 2.5, "end": 8.0 },
            { "role": "cta", "start": 8.0, "end": 10.0 }
        ]);
        let result = validate_edit_plan(&off_the_cut).expect("contiguous but off the cut");
        assert!(
            result["warnings"]
                .as_array()
                .expect("warnings")
                .iter()
                .any(|warning| warning
                    .as_str()
                    .is_some_and(|text| text.contains("mid-segment"))),
            "a section change off the cut is named: {result}"
        );

        // The same sound twice inside 30 s, and a static entrance twice,
        // are warnings - the anti-repetition laws have a voice.
        let mut repetitive = base.clone();
        repetitive["segments"][0]["sfx"] =
            json!({ "query": "Impact deep boom", "peakAt": 0.0, "gainDb": -8.0 });
        repetitive["segments"][3]["sfx"] =
            json!({ "query": "Impact deep boom", "peakAt": 6.0, "gainDb": -8.0 });
        repetitive["segments"][1]["in"] = json!({ "anim": "static" });
        repetitive["segments"][2]["in"] = json!({ "anim": "static" });
        let result = validate_edit_plan(&repetitive).expect("warnings, not refusals");
        let warnings = result["warnings"].as_array().expect("warnings");
        assert!(warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("Impact deep boom"))
        }));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.as_str().is_some_and(|text| text.contains("static")))
        );
    }

    /// The decision log: a plan with a voiceover and no signalLog is not
    /// creativeReady; a log that names the profile's signals drives the
    /// plan; a log citing signals the profile does not answer is a named
    /// problem.
    #[test]
    fn the_signal_log_gates_the_plan() {
        let cut = |start: f64, end: f64| {
            json!({
                "start": start,
                "end": end,
                "visual": { "type": "broll", "query": "ocean" },
                "cutType": "hard",
            })
        };
        let base = json!({
            "aspect": "9:16",
            "mode": "reel",
            "audio": { "voiceover": { "source": "tts", "voice": 2 } },
            "grade": { "filter": "concat.teal-orange", "intensityKey": "amount", "intensity": 65 },
            "sections": [
                { "role": "hook", "start": 0.0, "end": 2.0 },
                { "role": "body", "start": 2.0, "end": 8.0 },
                { "role": "cta", "start": 8.0, "end": 10.0 }
            ],
            "segments": [
                cut(0.0, 2.0),
                cut(2.0, 3.5),
                cut(3.5, 5.0),
                cut(5.0, 6.5),
                cut(6.5, 8.0),
                cut(8.0, 10.0)
            ]
        });
        // No voiceover, no log needed: a music-only plan is not blocked.
        let mut quiet = base.clone();
        quiet["audio"] = json!({});
        let result = validate_edit_plan(&quiet).expect("valid");
        assert_eq!(result["signalLog"]["driven"], json!(false), "{result}");
        // A voiceover with no log is a blocker: the take was never read.
        let result = validate_edit_plan(&base).expect("valid");
        assert_eq!(result["signalLog"]["driven"], json!(false));
        assert_eq!(result["creativeReady"], json!(false), "{result}");
        assert!(
            result["creativeBlockers"]
                .as_array()
                .expect("blockers")
                .iter()
                .any(|blocker| blocker
                    .as_str()
                    .is_some_and(|text| text.contains("signalLog"))),
            "the blocker names the signalLog: {result}"
        );
        // A log that names real signals drives the plan.
        let mut driven = base.clone();
        driven["signalLog"] = json!([
            { "at": 2.0, "decision": "cut", "signals": ["pause.class=dead_air", "emphasisWords"],
              "note": "dead air before the emphasis word" },
            { "at": 3.5, "decision": "cut", "signals": ["speechRate.pace"] },
            { "at": 5.0, "decision": "cut", "signals": ["rhythmLayers"] },
            { "at": 6.5, "decision": "cut", "signals": ["motifs"] },
            { "at": 8.0, "decision": "cut", "signals": ["pause.class=dead_air"] },
            { "at": 0.0, "decision": "bgm", "signals": ["forensic.noiseFloorDbfs", "speechRatio"],
              "note": "dry take: bed it" },
            { "at": 2.0, "decision": "caption", "signals": ["emphasisWords", "words[].confidence"] },
            { "at": 4.0, "decision": "graphic", "signals": ["entities", "sentence.type"],
              "note": "the money line gets a stat card" },
            { "at": 8.0, "decision": "transition", "signals": ["speakerChanges", "topics"] }
        ]);
        let result = validate_edit_plan(&driven).expect("valid");
        assert_eq!(result["signalLog"]["driven"], json!(true), "{result}");
        assert_eq!(result["signalLog"]["entries"], json!(9));
        assert!(
            result["creativeBlockers"]
                .as_array()
                .expect("blockers")
                .is_empty()
                || !result["creativeBlockers"]
                    .as_array()
                    .expect("blockers")
                    .iter()
                    .any(|blocker| blocker
                        .as_str()
                        .is_some_and(|text| text.contains("signalLog"))),
            "the signalLog blocker is gone: {result}"
        );
        // A signal the profile does not answer is a named problem.
        let mut invented = base.clone();
        invented["signalLog"] = json!([
            { "at": 2.0, "decision": "cut", "signals": ["the_vibe"] }
        ]);
        let result = validate_edit_plan(&invented).expect("valid");
        assert_eq!(result["signalLog"]["driven"], json!(false));
        assert!(
            result["signalLog"]["problems"]
                .as_array()
                .expect("problems")
                .iter()
                .any(|problem| problem
                    .as_str()
                    .is_some_and(|text| text.contains("the_vibe"))),
            "the invented signal is named: {result}"
        );
        // A time outside the plan is a named problem too.
        let mut late = base.clone();
        late["signalLog"] = json!([
            { "at": 99.0, "decision": "cut", "signals": ["pause"] }
        ]);
        let result = validate_edit_plan(&late).expect("valid");
        assert!(
            result["signalLog"]["problems"]
                .as_array()
                .expect("problems")
                .iter()
                .any(|problem| problem.as_str().is_some_and(|text| text.contains("99"))),
            "the out-of-plan time is named: {result}"
        );
    }

    #[test]
    fn a_wrong_argument_is_refused_by_the_schema_and_by_the_type_alike() {
        let toolbox = Toolbox::new();
        let (_, schema) = toolbox.find("clip_speed").expect("a tool");
        let wrong = json!({ "path": "/p", "clipId": "c1", "speed": "fast" });
        assert!(check(schema, &wrong, schema, "clip_speed").is_err());
        let error = edit("setClipSpeed", wrong).expect_err("refused");
        assert_eq!(error.code, ErrorCode::Invalid);
        let missing = request("project.open", json!({}));
        assert!(missing.expect_err("refused").message.contains("path"));
        assert!(
            captions(json!({ "path": "/p", "captions": [{ "start": 2, "end": 1, "text": "x" }] }))
                .is_err()
        );
    }

    /// The repository root, by the one file only it carries. `None`
    /// when the crate is built outside a checkout - published alone,
    /// vendored - where the documents are not there to be read.
    fn repository() -> Option<std::path::PathBuf> {
        let mut dir = std::env::current_dir().ok()?;
        loop {
            if dir.join(".forge").join("mcp.toml").is_file() {
                return Some(dir);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    /// The number of tools this server carries, as every document that
    /// states one is held to it. The count is computed from
    /// [`catalogue`] itself - the only source that cannot drift - and the
    /// README, FEATURES, FUNCUT, the Forge architecture note and the
    /// Forge allowlist are read from the repository this test runs in,
    /// so a tool added without its numbers caught up fails here rather
    /// than shipping a wrong claim. This is the guard the count's
    /// history asked for: 87 became 88, 88 became 89, 89 became 90, each
    /// change landing with the documents behind it and none of them
    /// noticed until the count was made a test - and 91 shipped with a
    /// stale "instead of 87" in FUNCUT.md and an "88 of them" in the
    /// architecture note, because the guard read only three documents.
    /// Every document that states the count now states it in the
    /// badge's own bold shape, and every one of them is held here.
    #[test]
    fn the_documents_tool_count_is_the_catalogues_own() {
        let Some(root) = repository() else {
            // Not a checkout: nothing to hold. CI and every clone carry
            // the marker file, so this is the standalone-crate case.
            return;
        };
        let count = catalogue().len();
        let read = |relative: &str| {
            std::fs::read_to_string(root.join(relative))
                .unwrap_or_else(|why| panic!("reading {relative}: {why}"))
        };

        // Every bold "**N tools**" a document states, in the shape the
        // README's badge made canonical: wrapped over as many lines as
        // the sentence needs, the digits the number and only they are.
        // A bold phrase that merely ends in " tools" - "**The tools**"
        // as a label, say - carries no digit and is not a count, so it
        // is left alone rather than read as one.
        fn bold_counts(text: &str) -> Vec<usize> {
            text.split("**")
                .filter(|piece| {
                    piece.trim_end().ends_with(" tools")
                        && piece.chars().any(|c| c.is_ascii_digit())
                })
                .map(|piece| {
                    let digits: String = piece.chars().filter(|c| c.is_ascii_digit()).collect();
                    digits.parse().unwrap_or(usize::MAX)
                })
                .collect()
        }

        // The README's badge line: "**91 tools**" is the shape it takes,
        // wrapped over as many lines as the sentence needs.
        let readme = read("README.md");
        let stated = bold_counts(&readme);
        assert!(
            !stated.is_empty(),
            "the README states no tool count; it should, as **{count} tools**"
        );
        for said in stated {
            assert_eq!(
                said, count,
                "the README says {said} tools; catalogue() carries {count}"
            );
        }

        // FEATURES.md: the inventory table's row, the prose's bold count
        // and the crate map's bracket.
        let features = read("docs/FEATURES.md");
        for said in bold_counts(&features) {
            assert_eq!(
                said, count,
                "FEATURES.md says {said} tools; catalogue() carries {count}"
            );
        }
        for line in features.lines() {
            if let Some(rest) = line.strip_prefix("| MCP tools | **") {
                let said: &str = rest.split("**").next().unwrap_or("");
                let said: usize = said.trim().parse().unwrap_or(usize::MAX);
                assert_eq!(
                    said, count,
                    "FEATURES.md's inventory table says {said}; catalogue() carries {count}"
                );
            }
            if let Some(before) = line.split(" tools, senses").next()
                && before != line
            {
                let said: usize = before
                    .rsplit(|c: char| !c.is_ascii_digit())
                    .find(|word| !word.is_empty())
                    .and_then(|word| word.parse().ok())
                    .unwrap_or(usize::MAX);
                assert_eq!(
                    said, count,
                    "FEATURES.md's crate map says {said} tools; catalogue() carries {count}"
                );
            }
        }

        // FUNCUT.md and the architecture note: the two a stale count
        // shipped in at 91 - "42 MCP tools instead of 87" in the one,
        // "88 of them" in the other - because neither was read here.
        // Both now state the count in the badge's own bold shape, and
        // both are held: every "**N tools**" either carries is the
        // catalogue's own.
        for document in ["docs/FUNCUT.md", "docs/forge/ARCHITECTURE.md"] {
            let stated = bold_counts(&read(document));
            assert!(
                !stated.is_empty(),
                "{document} states no tool count; it should, as **{count} tools**"
            );
            for said in stated {
                assert_eq!(
                    said, count,
                    "{document} says {said} tools; catalogue() carries {count}"
                );
            }
        }

        // The Forge allowlist, held to the director-surface policy: the
        // file's LAST paste-ready list is the whole catalogue, comma for
        // comma - so a tool added without its paste-ready line caught up
        // fails here rather than shipping advice the full surface does
        // not follow. The director default itself (the env table's
        // active list and the first paste-ready list) is held by its own
        // test below, `the_forge_allowlist_serves_the_directors_surface`.
        let forge = read(".forge/mcp.toml");
        let paste_ready: Vec<&str> = forge
            .lines()
            .filter_map(|line| line.strip_prefix("# CONCAT_MCP_ALLOW="))
            .next_back()
            .expect("the paste-ready whole-catalogue allowlist")
            .split(',')
            .map(str::trim)
            .collect();
        let mut names: Vec<&str> = catalogue().iter().map(|tool| tool.name).collect();
        names.sort_unstable();
        let mut pasted = paste_ready.clone();
        pasted.sort_unstable();
        assert_eq!(
            pasted, names,
            "the paste-ready whole-catalogue list is the catalogue, comma for comma"
        );
        // And the count the comment claims: "95 of the 105 tools".
        if let Some(rest) = forge.split(" of the ").nth(1) {
            let said: usize = rest
                .split(|c: char| !c.is_ascii_digit())
                .find(|word| !word.is_empty())
                .and_then(|word| word.parse().ok())
                .unwrap_or(usize::MAX);
            assert_eq!(
                said, count,
                "the Forge allowlist says the catalogue carries {said}; it carries {count}"
            );
        }
    }

    /// The allowlist the Forge server this repository declares runs
    /// behind, held to the director-surface policy. The list is not a
    /// comment's advice but the env the server starts with - an
    /// assignment in the `[servers.transport.env]` table - and it must
    /// name only tools the catalogue carries, leaving out exactly the
    /// ten the director never calls by surprise (the farm pair, the
    /// specialist motion reads, track_subject, geneva_explain, and the
    /// two a harness must never call: project_close and export_cancel).
    /// The first paste-ready comment must agree with it, or the two
    /// lists drift: a tool added to one and not the other ships advice
    /// the active server does not follow (or the reverse). The refusal
    /// itself is proven where it happens, in `protocol`'s own tests.
    #[test]
    fn the_forge_allowlist_serves_the_directors_surface() {
        let Some(root) = repository() else {
            // Not a checkout: nothing to hold. CI and every clone carry
            // the marker file, so this is the standalone-crate case.
            return;
        };
        let forge = std::fs::read_to_string(root.join(".forge/mcp.toml"))
            .unwrap_or_else(|why| panic!("reading .forge/mcp.toml: {why}"));

        // The active list: an assignment in the env table, found from
        // the line's start so the paste-ready comments cannot stand in
        // for the real one.
        let active = forge
            .lines()
            .find(|line| line.starts_with("CONCAT_MCP_ALLOW"))
            .expect("the env table carries the active CONCAT_MCP_ALLOW");
        let list = active
            .split_once('"')
            .and_then(|(_, rest)| rest.split_once('"'))
            .expect("the active list is one quoted value")
            .0;
        let allow: Vec<&str> = list.split(',').map(str::trim).collect();
        let names: Vec<&str> = catalogue().iter().map(|tool| tool.name).collect();
        for name in &allow {
            assert!(
                names.contains(name),
                "the active list names {name}, which catalogue() does not carry"
            );
        }
        // Exactly the ten the policy names are left off - no more, so a
        // specialist tool cannot sneak onto the default surface, and no
        // fewer, so the director's documented loop never meets a
        // refusal it did not earn.
        let mut left_off: Vec<&str> = names
            .iter()
            .filter(|name| !allow.contains(name))
            .copied()
            .collect();
        left_off.sort_unstable();
        assert_eq!(
            left_off,
            [
                "export_cancel",
                "geneva_explain",
                "motion_farm_cloud",
                "motion_farm_progress",
                "motion_preview",
                "motion_probe",
                "project_close",
                "scene_analysis",
                "scene_timeline",
                "track_subject",
            ],
            "the active allowlist leaves out exactly the ten the policy names"
        );
        assert_eq!(
            allow.len() + left_off.len(),
            names.len(),
            "the active list carries no name twice"
        );

        // The FIRST paste-ready comment says the same list, comma for
        // comma - the whole-catalogue one further down is the other
        // test's business.
        let paste_ready = forge
            .lines()
            .filter_map(|line| line.strip_prefix("# CONCAT_MCP_ALLOW="))
            .next()
            .expect("the paste-ready director allowlist");
        assert_eq!(
            paste_ready.split(',').map(str::trim).collect::<Vec<_>>(),
            allow,
            "the active list and the paste-ready comment must agree"
        );
    }
}
