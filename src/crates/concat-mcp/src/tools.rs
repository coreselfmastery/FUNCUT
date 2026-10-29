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
/// How long `export_wait` waits when told nothing, and at most.
const WAIT_DEFAULT: f64 = 60.0;
const WAIT_LONGEST: f64 = 600.0;
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
    /// One frame of a comp, drawn and written under the project.
    MotionFrame,
    /// A comp baked, and encoded when asked.
    MotionBake,
    /// A word-timed transcript as one caption video.
    CaptionsAnimate,
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
    /// Read or write the project's persisted creative brief.
    CreativeBrief,
    /// Composite a transparent motion bake over real footage.
    MotionOverlay,
    /// Measure an exported mix's integrated LUFS and true peak.
    MeasureLoudness,
    /// Beat-aligned cut times from the soundtrack, no Python extras.
    SuggestCuts,
    /// `project.get`, read for every place two clips share a lane.
    CollisionCheck,
    /// `openvid.search` once per query, one reply.
    OpenvidSearchBatch,
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
            description: "What this Concat speaks: the API version, the build, its capabilities, and how this MCP server reaches it (`mcp.mode`: \"window\" when it goes through the open Concat window's server, \"headless\" when it runs the editor engine itself). Call it first if unsure what is available.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::Version,
        },
        method(
            "project_list",
            "Recent projects",
            "The projects this machine opened most recently, newest first: each one's folder `path`, name, frame size and rate. A project is a folder; every other project tool names it by that `path`.",
            "project.list",
            Raw,
        )
        .reading(),
        method(
            "project_create",
            "Create a project",
            "Creates a project folder named `name` inside the existing directory `location`, and opens it. `video` sets the frame and rate (1920×1080 at 30 fps when absent). Returns the project summary; its folder is `location/name`, which is the `path` later tools take. Refuses a folder that already holds a project.",
            "project.create",
            Summary,
        ),
        method(
            "project_open",
            "Open a project",
            "Opens a project folder so it can be read and edited. Opening one already open returns it as it stands, unsaved edits included. Refused when the Concat window has that project open. Returns the project summary.",
            "project.open",
            Summary,
        )
        .idempotent(),
        method(
            "project_save",
            "Save a project",
            "Writes an open project's document to its folder; `name` renames the project as it saves. Edits autosave as they happen (the document is written after every applied edit), so this is rarely needed - a save now is for renaming or tidiness, not for keeping work.",
            "project.save",
            Raw,
        )
        .idempotent(),
        method(
            "project_close",
            "Close a project",
            "Closes an open project. Edits autosave as they happen, so nothing is lost either way; `save: true` writes the document once more before it goes.",
            "project.close",
            Raw,
        )
        .destroying(),
        Tool {
            name: "timeline_read",
            title: "Read the timeline",
            description: "An open project's active timeline as a compact summary: frame size and fps, the lanes (`tracks`, top to bottom, with their ids), every clip in time order with its id, kind, track, start, duration and end in seconds and whatever is not at its default (speed, volume, opacity, transform, effects, keyframed properties, text), and the media bin with each item's id. Read this to learn the ids other tools need.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::TimelineRead,
        },
        Tool {
            name: "plan_validate",
            title: "Validate an edit plan",
            description: "Checks a director plan's canonical schema, cross-field constraints AND its creative readiness before edits. Returns valid (structure), creativeReady (craft gate), creativeBlockers (what to fix), craftScore 0-10, warnings and notes. A plan with no creativeBrief, no emotionalArc, a hook over 3 s, repeated visuals, no designed comp, or a run of plain hard cuts is valid but NOT creativeReady - fix the blockers and validate again. Read-only; asset availability and the final render still need stock_contact_sheet, watch_render and craft_critique.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::PlanValidate,
        },
        Tool {
            name: "clip_audio_levels_db",
            title: "Set clip audio levels in dB",
            description: "Sets base gain and optional automation in dB relative to source unity (0 dB = unchanged; −60 dB = silence floor; range −60 to +24). Converts to linear multipliers. Keyframe `gainDb` targets are absolute to unity; `baseDb` applies outside keyed regions. `replaceAutomation: true` clears existing volume keys in the same undoable batch.",
            read_only: false,
            destructive: true,
            idempotent: true,
            action: Action::AudioLevelsDb,
        },
        Tool {
            name: "collision_check",
            title: "Check for clips sharing a lane",
            description: "Reads an open project's active timeline and reports every place two clips share one lane: each collision's `trackId`, the two clips' ids, starts, durations and ends, and the span they fight over (`from`, `to`). The placing tools never make one - a clip that would land on another lands at the nearest free start, and a move that cannot land does not move - so a collision here means a document written by hand or damaged; `timeline_read` and the placing tools are how one is unwound. `clean` is true when no lane is shared. Read-only.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::CollisionCheck,
        },
        method(
            "project_get",
            "Get the whole project",
            "An open project's full state: every timeline, clip field, media item and setting, as the editor holds it. Large; `timeline_read` is usually enough.",
            "project.get",
            Raw,
        )
        .reading(),
        method(
            "project_set_video",
            "Set frame size and rate",
            "Sets the active timeline's frame (width, height in pixels) and rate (rateNum/rateDen frames a second, e.g. 30000/1001 for 29.97) as one undoable edit.",
            "project.setVideo",
            Summary,
        ),
        method(
            "media_probe",
            "Probe a media file",
            "Reports what is inside a video, audio or image file - duration in seconds, streams, frame size and rate, codecs - without changing any project.",
            "media.probe",
            Raw,
        )
        .reading(),
        method(
            "media_import",
            "Import media",
            "Adds a file (an absolute path) to an open project's media bin, as dropping it on the window does. The new item's id is the summary's `createdId`; place it on the timeline with `clip_add`. Importing a path already in the bin changes nothing.",
            "media.import",
            Summary,
        )
        .idempotent(),
        method(
            "openvid_status",
            "OpenVid library status",
            "Where the OpenVid stock library stands: `state` is \"missing\" (its local index is not on this machine yet - call `openvid_prepare` once, ~435 MB, then poll this until `ready`), \"downloading\" (with `percent`), \"ready\" or \"failed\" (with `error`). When ready, `rows` is how many clips it holds (937,957: stock footage with captions and quality scores) and `measured` how many have had their frame size read so far.",
            "openvid.status",
            Raw,
        )
        .reading(),
        method(
            "openvid_prepare",
            "Download the OpenVid index",
            "Starts the one-time download of the OpenVid library's local index and returns at once; poll `openvid_status` until `state` is \"ready\". Nothing else downloads anything: every other `openvid` tool refuses until this has finished.",
            "openvid.prepare",
            Raw,
        )
        .idempotent(),
        method(
            "openvid_search",
            "Search stock clips",
            "Searches 937,957 stock clips (the Pixabay, Pexels and CelebV shelves cut to 512x512, and the WebVid shelf of 720p and 1080p landscape footage) by what their captions say and by their quality numbers, without watching anything: each match carries the caption, `aesthetic` (0-6), `motion`, `temporal` consistency (0-1), `camera` motion, `fps`, `seconds`, `bytes` (the fetch cost), its shelf and its measured `dims`. Every filter is optional: `query` words are ANDed and prefix-matched (\"ocean sunset\"), `minAesthetic`/`minMotion`/`maxMotion`/`minTemporal` set the quality floor, `minSeconds`/`maxSeconds`/`minFps` the shape, `camera` takes a motion exactly as the library names it, `source` is pixabay/pexels/celebv/other, `orientation` is vertical/horizontal/square (measured from each clip's own MP4 header: the first shape search takes a one-time census of every shelf, a few seconds, after which square finds the three 512x512 shelves and horizontal the WebVid shelf's landscape footage, and nothing vertical has been measured in it), and `sort` is relevance/aesthetic/motion/temporal/shortest/longest/surprise. Returns up to `limit` (default 20, at most 100) from `offset`, with `total` saying how many match in all. A relevance search of more than ten matches then gets a semantic second pass: a local MobileCLIP2-S0 ONNX model looks at each candidate's real frames (already-fetched clips are read as they are, the rest are fetched once and cached, under a time budget) and re-orders the page by what the clips SHOW, not what their captions say - \"the moment a decision goes wrong\" reaches worried faces and hesitation, not every caption carrying \"decision\". Missing models or wheels leave the BM25 order untouched; the reply's `rerank` says whether it ran (`applied`, `scored`, `ms`) and why not when it did not. The models are two ONNX exports of MobileCLIP2-S0 found under `$FUNCUT_MOBILECLIP` or the app data dir's models/mobileclip2-s0/; the wheels are `pip install onnxruntime numpy transformers`.",
            "openvid.search",
            Raw,
        )
        .reading(),
        Tool {
            name: "openvid_search_batch",
            title: "Search stock clips, several queries at once",
            description: "The `openvid_search` search run once per entry of `queries` - two or three words each, a noun a camera could film, never a sentence - in one call, every entry with the same optional floors and filters: `minAesthetic`/`minMotion`/`maxMotion`/`minTemporal`, `minSeconds`/`maxSeconds`/`minFps`, `camera`, `source`, `orientation`, `sort`, `limit` (per query, default 20) and `offset`. The reply's `searches` carry one entry per query - its `query`, `total` and `clips` - in the order given; the tool fails as `openvid_search` would (the OpenVid index must be prepared first). Shop a whole plan's B-roll in one round trip, then look before choosing with `stock_contact_sheet`.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::OpenvidSearchBatch,
        },
        method(
            "openvid_get",
            "Read clips in full",
            "Everything about up to 20 clips by id, as `openvid_search` gave the ids: the full caption, every score, and `dims` - the frame size read from the clip's own MP4 header, measured on this first ask (one or two small network reads a clip, remembered after). Use it to check a candidate before committing to the fetch; captions and scores are words, not pictures - `stock_contact_sheet` (its `ids` name these same clips) is how a clip is SEEN before it is fetched.",
            "openvid.get",
            Raw,
        )
        .reading(),
        method(
            "openvid_fetch",
            "Fetch a clip",
            "Fetches one clip's MP4 from the Hub into the app's own cache - `clip.bytes` of bandwidth, nothing else - and returns the clip with `localPath`, its absolute path on this machine. Adds nothing to any project; `openvid_import` or `media_import` is what puts it in an edit.",
            "openvid.fetch",
            Raw,
        )
        .idempotent(),
        method(
            "openvid_import",
            "Import stock clips",
            "Fetches clips and adds them to an open project's media bin, each copied into the project's own `openvid` folder first so the project owns its files. With `start` (seconds) they are also laid on one lane in the order given, as one undoable step - `track` names the lane, or the first with room when absent. Returns `mediaIds` and the project summary; without `start`, place them with `clips_add_sequence` or `clip_add`. At most 20 clips a call.",
            "openvid.import",
            Raw,
        ),
        method(
            "sounds_search",
            "Search the sound library",
            "Searches the built-in CC0 sound library - 530 sounds: whooshes, impacts, risers, ambiences, music beds and logo stings, UI clicks, motion-design pops and transforms, notifications, keyboards, digital glitches and data sounds, foley and writing, plus a 100-sound trailer-grade Hollywood pack (cinematic slams, sub drops, braams, reverse sucks, flybys, glitch zaps, portal and magic reveals, drones, and logo stingers) - and returns the matches best first, without hearing anything: each carries its `id`, `name`, `category`, `tags`, `duration` in seconds, `bytes` (the download cost), `description` (what the sound is, a sentence), `useFor` (what it is for, as short lines), `syncAt` (seconds in where its peak lands, for placing it by the moment it is for) and `localPath` when the file is already on this machine. `query` words are ANDed and prefix-matched against the name, id, category, tags, use-for lines and description, ranked by where they matched - the name counts most, then the tags, then the description - so \"fast whoosh\" puts a *Whoosh, fast* ahead of a whoosh merely tagged fast, and \"card transition\" finds a swipe its use-for lines suggest; `category` narrows to Whoosh, Impact, Riser, Ambience, Music, UI, Motion, Notification, Keyboard, Digital, Foley or Writing. `sounds_add` places one by its id. Returns up to `limit` (default 20, at most 100) from `offset`, with `total` saying how many match in all.",
            "sounds.search",
            Raw,
        )
        .reading(),
        method(
            "sounds_add",
            "Add a sound",
            "Adds one built-in sound (`id`, as `sounds_search` gave it) to an open project: downloaded into the app's cache when it is not there yet and checked against its digest, copied into the project's own `audio` folder, and imported into the bin - with `start` (seconds) also laid on one lane as one undoable step, `track` naming the lane or the first with room when absent. Returns `mediaId`, `clipId` when a clip was laid, and the project summary; without `start`, place it with `clip_add`.",
            "sounds.add",
            Raw,
        ),
        method(
            "bgm_status",
            "Music library status",
            "Where the background-music library stands, shelf by shelf: `freepd` (the Internet Archive's mirror of Kevin MacLeod's CC0 library) and `lofi` (Moby's MobyGratis instrumentals, as the OpenLo-Fi pack) are each \"missing\" (their list is not on this machine yet - call `bgm_prepare` once, a few MB, then poll this until \"ready\"), \"loading\" (with `percent`: the one-time measuring of the Lo-Fi zip, a few seconds), \"ready\" (with `freepdTracks`/`lofiTracks`, how many each holds: 1,025 and 166) or \"failed\" (with `error`). 1,191 whole CC0 tracks in all; none of them in the repository - a track's bytes move only when it is picked.",
            "bgm.status",
            Raw,
        )
        .reading(),
        method(
            "bgm_prepare",
            "Load the music library",
            "Starts the shelves' one-time catalog fetches and returns at once; poll `bgm_status` until the shelf you read is \"ready\". Optional `source` (\"freepd\" or \"lofi\") narrows it to one shelf - worth it when the other is down. The FreePD list is the Archive item's own file listing (~1 MB) and the Lo-Fi list is the pack's manifest, beside which every track's place and length in the pack's 554 MB zip is measured once with one small ranged read a track (the `percent` you watch). Nothing else downloads anything: every other `bgm` tool refuses until the shelf it reads is ready.",
            "bgm.prepare",
            Raw,
        )
        .idempotent(),
        method(
            "bgm_search",
            "Search music",
            "Searches 1,191 whole CC0 background-music tracks by what their titles, artists and categories say, without hearing anything: each match carries the title, the artist (Kevin MacLeod, Moby, Rafael Krux, ...), the category (FreePD's own folders - upbeat, epic, comedy, scoring, world, ... - and the Lo-Fi pack's - chillhop, late-night, ambient-lofi, ...), `seconds`, `bytes` (the fetch cost), whether it is already on this machine (`localPath`), and which shelf it is on (`source`: freepd or lofi). Every filter is optional: `query` words are ANDed and prefix-matched (\"city rain\"), `source` is freepd/lofi, `category` takes a shelf's own spelling, `minSeconds`/`maxSeconds` set the length (a track whose length is unknown is passed by rather than guessed at), and `sort` is relevance/shortest/longest/surprise. Returns up to `limit` (default 20, at most 100) from `offset`, with `total` saying how many match in all.",
            "bgm.search",
            Raw,
        )
        .reading(),
        method(
            "bgm_fetch",
            "Fetch a track",
            "Fetches one track into the app's own cache - FreePD as its own file from the Archive, Lo-Fi as one ranged read of exactly that track's span out of the pack's 554 MB zip, inflated and checked against the CRC the zip itself carries - and returns the track with `localPath`, its absolute path on this machine. `track.bytes` of bandwidth, nothing else. Adds nothing to any project; `bgm_import` or `media_import` is what puts it in an edit.",
            "bgm.fetch",
            Raw,
        )
        .idempotent(),
        method(
            "bgm_import",
            "Import music",
            "Fetches tracks and adds them to an open project's media bin, each copied into the project's own `bgm` folder first so the project owns its files. With `start` (seconds) they are also laid on one lane in the order given, as one undoable step - `track` names the lane, or the first with room when absent. Returns `mediaIds` and the project summary; without `start`, place them with `clips_add_sequence` or `clip_add`. At most 20 tracks a call.",
            "bgm.import",
            Raw,
        ),
        method(
            "magick_status",
            "Sticker library status",
            "Where the sticker library stands, shelf by shelf: `magick` (MAGICK: 145,193 captioned AI-generated transparent PNGs cut out of their backgrounds - people, animals, objects, gestures) and `openmoji` (4,565 transparent emoji, CC-BY 4.0) are each \"missing\" (call `magick_prepare` once, ~18 MB, then poll this), \"downloading\" (with `percent`), \"ready\" (with `magickRows`/`openmojiRows`) or \"failed\" (with `error`). 149,758 overlays in all; none in the repository - a picture's PNG moves only when picked.",
            "magick.status",
            Raw,
        )
        .reading(),
        method(
            "magick_prepare",
            "Load the sticker library",
            "Starts both shelves' one-time index downloads (~16 MB MAGICK, digest-checked; ~2 MB OpenMoji) and returns at once; poll `magick_status` until both are \"ready\". Nothing else downloads anything: every other `magick` tool refuses until the shelf it reads is ready.",
            "magick.prepare",
            Raw,
        )
        .idempotent(),
        method(
            "magick_search",
            "Search stickers",
            "Searches 149,758 transparent-PNG overlays - MAGICK's 145,193 captioned AI-generated pictures and OpenMoji's 4,565 emoji - by what their captions say: each match carries `id`, `source` (magick/openmoji), `subject` (the caption) and `localPath` when the PNG is already on this machine. `query` words are ANDed and prefix-matched - the emoji read their words and tags both - ranked by where and how rarely the words matched, the whole phrase as one run counting most: \"red balloon\" finds a red balloon, \"party\" the popper emoji. `source` narrows the shelf, `favourites` the heart's list, `limit` (1-100, default 20) and `offset` page. `magick_add` places one by id.",
            "magick.search",
            Raw,
        )
        .reading(),
        method(
            "magick_fetch",
            "Fetch a sticker",
            "Fetches one picture's PNG into the app's cache - one plain GET (~1 MB MAGICK, a few KB emoji), checked to be a PNG that can hold transparency - and returns it with `localPath`, its absolute path here. Adds nothing to any project; `magick_add` is what puts one in an edit.",
            "magick.fetch",
            Raw,
        )
        .idempotent(),
        method(
            "magick_add",
            "Add a sticker",
            "Adds one overlay (`id`, as `magick_search` gave it) to an open project: fetched, copied into the project's own `magick` folder, imported - with `start` (seconds) also laid *over* the picture, never beneath it: the first free lane above the highest occupied, as a title lands. Optional placement, one undoable step: `duration` (seconds), `track` (a lane by name, taken as `clip_add` takes one), `scale` (0.05-8, 1 fills the frame; a sticker usually wants ~0.2-0.4), `offsetX`/`offsetY` (frame fractions from centre, positive right/down), `rotation` (degrees). Returns `mediaId`, `clipId` when laid, and the summary.",
            "magick.add",
            Raw,
        ),
        method(
            "edit_apply",
            "Apply an edit",
            "Applies any one edit command to an open project, as one undoable step. `command` is an object tagged by `op` - every edit the window can make is one, with the same limits and refusals; the typed tools below are the common ones. `{\"op\": \"batch\", \"commands\": [...]}` applies several commands as one atomic, undoable step: all land or none does, and `createdId` is the last id minted inside. A refusal comes back as the sentence the window would show, with nothing changed. Returns the project summary; a command that creates something puts its id in `createdId`.",
            "edit.apply",
            Summary,
        ),
        method(
            "edit_undo",
            "Undo",
            "Steps an open project's history back one edit (a batch is one edit).",
            "edit.undo",
            Summary,
        ),
        method(
            "edit_redo",
            "Redo",
            "Steps an open project's history forward one edit.",
            "edit.redo",
            Summary,
        ),
        command(
            "clip_add",
            "Place a clip",
            "Places a clip of a media item (`mediaId`, from the bin) on a lane (`trackId`, from `timeline_read`) starting at `start` seconds - or, where another clip already sits, at the nearest free start, which the reply's summary carries. Its length is the media's (5 s for a still). `ripple: true` pushes later clips on that lane right instead of finding a gap. The new clip's id is `createdId`.",
            "addClip",
        ),
        command(
            "clips_add_sequence",
            "Place clips end to end",
            "Lays several media items (`mediaIds`, from the bin, in order) end to end starting at `start` seconds, each its media's length (5 s for a still), in one undoable step. With no `trackId` they go on the lowest lane free for the whole run; on a named lane, `ripple: true` makes room as a drop does while the plain form lands the run at the nearest gap that holds it. `createdId` is the last clip's id; `timeline_read` lists the rest, and `collision_check` confirms no lane was shared.",
            "addClipsInSequence",
        ),
        command(
            "clip_trim",
            "Trim a clip",
            "Moves one edge of a clip by `delta` seconds of timeline: `edge` \"start\" with a positive delta shortens it from the head (the picture does not slide), \"end\" with a positive delta lengthens it. A clip is never shorter than 1/60 s or longer than its media allows. `ripple: true` closes or opens the gap behind it on that lane.",
            "trimClip",
        ),
        command(
            "clips_split",
            "Split clips",
            "Cuts each named clip in two at `time` seconds on the timeline. The first part keeps the id; the second gets a new one. Clips the time does not fall inside are left alone.",
            "splitClips",
        ),
        command(
            "clip_cut_spans",
            "Cut spans out of a clip",
            "Cuts spans out of the middle of a clip: `spans` is a list of [from, to] pairs in seconds from the clip's start, in any order; overlapping ones merge. What is left is re-cut into source-continuous pieces that each keep the clip's looks, the first keeping the clip's id (`createdId`); a piece too short to keep folds into the span beside it, and spans that cover the whole clip remove it. `ripple: true` closes the lane behind every cut, as a delete does. This is what the voiceover panel's pause removal does.",
            "removeSpans",
        ),
        command(
            "clips_move",
            "Move clips",
            "Moves clips in one undoable step: each to a new `start` (seconds, at least 0) and `trackId`. A move that would land the group on clips that stay put lands it instead at the nearest free place, the whole group keeping its shape; a move whose own clips would cover each other does not happen and the reply says nothing was applied. Unknown ids are skipped.",
            "moveClips",
        ),
        command(
            "clips_remove",
            "Delete clips",
            "Deletes clips from the active timeline. `ripple: true` closes the gaps they leave on their lanes; false leaves the gaps.",
            "removeClips",
        )
        .destroying(),
        command(
            "clip_update",
            "Change a clip",
            "Changes only the fields given in `patch`: volume (linear gain multiplier, 1 is unchanged, may exceed 1; not dB—use `clip_audio_levels_db` for decibel levels), fadeIn/fadeOut (seconds), opacity (0-1), muted, flipH/flipV, blend, crop (fractions off each edge, null to remove), videoEffects and filters (the whole chain: a list of {id, params}; ids from `effects_list`), transitionIn ({id, duration} on the cut into the clip, null to remove), text (a title's style, null to remove), mask (see `clip_mask`), background (the clip's own background, null to follow the timeline's) and audioStream.",
            "updateClip",
        ),
        Tool {
            name: "clip_mask",
            title: "Mask a clip",
            description: "Keeps part of a clip's picture with a shape, or with `mask: null` takes the mask off. `shape` is linear, mirror, circle, rectangle, heart or star; `x`/`y` move its centre as fractions of the picture's width/height (positive right/down); `width`/`height` are fractions of the picture's shorter side; `rotation` in degrees; `roundness` (rectangle corners) and `feather` (edge softness) 0-1; `invert` keeps the outside. Fields left out take the default circle's. Animate it with `keyframe_set` on maskX, maskY, maskWidth, maskHeight, maskRotation, maskRoundness or maskFeather.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::Mask,
        },
        command(
            "clip_stabilize",
            "Stabilize a clip",
            "Takes the camera's shake out of a video clip, or with `stabilize: null` stops. `smoothness` 0-1 (low keeps pans and removes jitter, high leaves only the slowest moves), `maxZoom` (at least 1) is how far the picture may be enlarged to hide the moving edges, `tripod: true` locks the frame entirely. The shake is measured when the clip is previewed or exported.",
            "setClipStabilize",
        ),
        command(
            "clip_frame",
            "Fit or fill the frame",
            "Sizes a picture to the frame in one step: `framing` \"fit\" shows all of it (bars where the shapes differ), \"fill\" covers the frame and crops the overhang. Where the clip's scale is keyframed, the key goes at `at` (a fraction of the clip, 0-1).",
            "frameClip",
        ),
        command(
            "timeline_background",
            "Set the background",
            "Sets what shows behind pictures that do not cover the frame, on a whole timeline (`timelineId` from `timeline_read`): `{\"kind\": \"none\"}` (black), `{\"kind\": \"blur\", \"amount\": 0-1}` (the clip itself, enlarged and blurred), `{\"kind\": \"color\", \"color\": \"#rrggbb\"}` or `{\"kind\": \"image\", \"mediaId\": ...}`. One clip's own background is `clip_update`'s `background`.",
            "setTimelineBackground",
        ),
        command(
            "clip_transform",
            "Place, scale or rotate a clip",
            "Sets a picture's placement; only the fields given change. scale 0.05-8 (1 fits the frame), offsetX/offsetY as fractions of the frame's width/height from the centre (positive is right/down, within ±3), rotation in degrees, stretchX/stretchY 0.1-10.",
            "setClipTransform",
        ),
        command(
            "clip_speed",
            "Change a clip's speed",
            "Plays a clip faster or slower: `speed` from 0.0625 to 16 (2 is double speed). The same stretch of source is kept, so the clip's length on the timeline changes.",
            "setClipSpeed",
        ),
        command(
            "title_add",
            "Add a title",
            "Places a title (text over the picture) at `start` seconds for `duration` seconds (4 s when absent). `style` sets the words and look - `content`, `fontFamily`, `fontSize` (fraction of frame height, 0.09 by default), `fontWeight` (100-900), `color` (\"#rrggbb\"), `align`, `background`, `strokeWidth`... - and needs only the fields that differ from the default. `offsetY` places it vertically as a fraction of frame height from the centre (positive is down). With no `trackId` and `above: true` it lands on a lane above the picture. The new clip's id is `createdId`.",
            "addTextClip",
        ),
        Tool {
            name: "captions_add",
            title: "Add captions",
            description: "Places a run of captions in one undoable step: one title per entry, from `start` to `end` seconds, each on a lane above the picture. `style` applies to all of them (its `content` is ignored); without one they look like the window's captions - Hanken Grotesk, weight 600, 0.04 of the frame high. `offsetY` is where they sit, as a fraction of frame height from the centre: 0.35 (the default) is near the bottom, 0 the middle, -0.35 the top.",
            read_only: false,
            destructive: false,
            idempotent: false,
            action: Action::Captions,
        },
        command(
            "keyframe_set",
            "Set a keyframe",
            "Animates a clip: puts a key on `property` (scale, offsetX, offsetY, rotation, opacity, volume, or a mask's maskX, maskY, maskWidth, maskHeight, maskRotation, maskRoundness, maskFeather) at `at`, a fraction of the clip's length from 0 (its start) to 1 (its end). `value` uses the property's stored units; notably `volume` is a linear gain multiplier (1 = unchanged), not dB. Use `clip_audio_levels_db` for absolute dB levels. A key already within a hair of `at` is replaced. `ease` is how the value is approached: [x1, y1, x2, y2] like a CSS cubic-bezier, linear when absent.",
            "setClipKey",
        ),
        command(
            "keyframe_clear",
            "Remove a keyframe",
            "Removes the key on `property` at `at` (a fraction of the clip's length), if there is one.",
            "clearClipKey",
        )
        .destroying(),
        command(
            "effect_layer_add",
            "Add an effect layer",
            "Places an effect or look as a layer over the timeline: from `start` for `duration` seconds (5 s when absent), it treats everything beneath it. `effectId` is a package id from `effects_list`; the layer's opacity (via `clip_update`) is how strongly it applies. To put an effect on one clip instead, set that clip's `videoEffects` with `clip_update`.",
            "addLayerClip",
        ),
        command(
            "effect_key_set",
            "Animate an effect parameter",
            "Puts a key on one parameter (`key`, from `effects_list`) of one effect in a clip's chain (`entry`, its index in the clip's videoEffects, from 0) at `at` (a fraction of the clip's length, 0-1), holding `value` in the parameter's own units. Keep the value within the range `effects_list` gives.",
            "setEffectKey",
        ),
        command(
            "track_add",
            "Add a lane",
            "Adds a lane to the active timeline: on top, or with `bottom: true` under every other. Its id is `createdId`.",
            "addTrack",
        ),
        method(
            "effects_list",
            "List effects",
            "Every effect package this build has - id, name, kind (\"effect\", \"filter\", \"audio\", \"transition\" or \"generator\"), category and each parameter's key, range, default and unit - so an effect chain, a layer or a transition can be built correctly. `kind` narrows the list.",
            "catalogue.list",
            Raw,
        )
        .reading(),
        method(
            "template_list",
            "List templates",
            "The template library: each template's folder, name and the slots (placeholder media ids) a project made from it must fill.",
            "template.list",
            Raw,
        )
        .reading(),
        method(
            "template_instantiate",
            "Make a project from a template",
            "Makes a project from a template - `template` (its folder), `location` and `name` as for `project_create` - with every slot filled by a file (`fills`: [{mediaId, file}]). Opens it and returns its summary.",
            "template.instantiate",
            Summary,
        ),
        method(
            "export_start",
            "Start an export",
            "Starts rendering an open project's active timeline to `output` (an .mp4 path), as the window's Export does. Returns at once with a `job` id; follow it with `export_wait` or `export_status`. Optional: crf (quality, lower is better, 20 by default), preset (x264 speed, \"medium\"), width/height, rateNum/rateDen, codec (\"h264\", \"hevc\" or \"av1\"), tenBit, colorRange, hdr. One export runs at a time; another is refused as busy.",
            "export.run",
            Raw,
        ),
        method(
            "export_status",
            "Export status",
            "How an export job stands, at once: `state` (\"running\", \"done\", \"cancelled\" or \"failed\"), `stage` (\"cutout\", \"stabilize\", \"rendering\", \"mixing audio\" or \"muxing\"), frames done of total, `fraction` of the stage (0-1), and once done the file and its size, or the error.",
            "export.status",
            Raw,
        )
        .reading(),
        Tool {
            name: "export_wait",
            title: "Wait for an export",
            description: "Waits for an export job to end, up to `timeoutSeconds` (60 by default, 600 at most), reporting progress as it goes, and returns its status as `export_status` does. If it is still running when the time is up, call it again.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::ExportWait,
        },
        method(
            "export_cancel",
            "Cancel an export",
            "Stops a running export job at its next frame; it then ends as \"cancelled\".",
            "export.cancel",
            Raw,
        )
        .destroying(),
        Tool {
            name: "preview_frame",
            title: "Preview a frame",
            description: "Renders the true composited frame at `time` seconds on an open project's timeline and returns it as an image to look at - titles, effects and transforms included. Without `width` and `height` it fits within 1280 pixels, a file as much as an inline picture; give both for another size (8192 at most). With `output` (a .png path) the picture is written there instead - a file write, held to this session's write roots, which is why the tool is not marked read-only.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::PreviewFrame,
        },
        method(
            "analyze_scenes",
            "Find scene cuts",
            "Finds where one shot becomes another in a video: a job returning {start, end} scenes in seconds (PySceneDetect). Wait with analysis_wait; the result is the status's `output`. Optional: threshold (3.0 default), detector (\"adaptive\" default), minSceneLen seconds (0.6 default). The reply says the pip line when the Python extras are missing.",
            "analysis.scenes",
            Raw,
        ),
        method(
            "analyze_beats",
            "Find the beat grid",
            "Measures a piece of music on a job: bpm, the beat grid and onsets in milliseconds (beatsMs, downbeatsMs, onsetsMs), the numbers a beat-aligned cut is cut by. Wait with analysis_wait; the result is the status's `output`.",
            "analysis.beats",
            Raw,
        ),
        method(
            "analyze_silence",
            "Find dead air",
            "Finds a recording's silent spans on a job: spans and their complement speech, in seconds - the shape edit_apply's removeSpans cuts by. Wait with analysis_wait; the result is the status's `output`. Optional: thresholdDb (35 default), minSilenceMs (500 default).",
            "analysis.silence",
            Raw,
        ),
        method(
            "transcribe",
            "Transcribe a recording",
            "Transcribes a media file on a job; output timed segments (`{start,end,text}`), each carrying its `words` as measured `{text,start,end}` triples from whisper's token timestamps - the word times `captions_animate` needs (never estimate words from segment times). `language` is a Whisper hint (ISO code/BCP-47); use a multilingual model for non-English (`base.en` default is English-only). Wait with analysis_wait; the result is in status.output.",
            "speech.transcribe",
            Raw,
        ),
        method(
            "tts_generate",
            "Speak a voiceover",
            "Speaks `text` with a `voice` as a WAV in an open project's own audio folder - a voiceover in one call, ready for media_import. Wait with analysis_wait; the status's `output` is the WAV's path and length. Voices are the Settings > Speech ids (Kokoro 0-52); a missing model is fetched headless with models_download. DELIVERY: give `delivery` (neutral, conversational, urgent, excited, calm, awed, somber, authoritative) to set a director's pacing - rate AND the breath between sentences - instead of a flat read; `speed` (0.5-2) overrides its rate and `pauses` (0-1) overrides its breath. `steps` (1-16) is Pocket's quality dial. Match the delivery to the emotionalArc: urgent for the hook, calm or awed for the reveal, authoritative or warm for the CTA.",
            "speech.generate",
            Raw,
        ),
        method(
            "models_list",
            "List speech models",
            "Every voice (tts) and transcriber (whisper) model this build knows, with each one's download size and whether it is on disk - the ids are what models_download takes and what tts_generate's and transcribe's model arguments accept.",
            "models.list",
            Raw,
        ),
        method(
            "models_download",
            "Download a speech model",
            "Downloads one model from Concat's mirror on a job - the headless way to a voice or a transcriber, no window needed. kind is \"tts\" or \"whisper\"; ids come from models_list. A model already on disk ends at once. Wait with analysis_wait; the status's `output` confirms what landed.",
            "models.download",
            Raw,
        ),
        Tool {
            name: "motion_presets",
            title: "List the motion library",
            description: "Every motion-graphics preset - lower thirds, kinetic captions, stat cards, wordmark stings, pull quotes, chapter chips, subscribe cards, terminals, bar charts, split compares, counting numbers, mesh backdrops - with its category, frame size, duration, slots to fill and the nodes it animates, plus whether the Geneva engine is on this machine to bake it. The vocabulary Phase 5b of the director skill designs with; `motion_inspect` sees inside one.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::MotionPresets,
        },
        Tool {
            name: "motion_inspect",
            title: "Inspect a motion comp",
            description: "What is in a motion comp before it is touched: every node with its kind, span, animated properties and first words, the slots it can be filled by, validation problems, and whether it needs the markup engine. Takes `preset` (an id from motion_presets) or `comp` (a file under the project's comps/), and applies an optional `ops` batch first - the same operations motion_bake applies - so an edit can be checked before it bakes.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::MotionInspect,
        },
        Tool {
            name: "motion_frame",
            title: "Draw one motion frame",
            description: "One frame of a motion comp at `at` seconds - tens of milliseconds, no bake - returned as an INLINE IMAGE you can actually look at, and also written under the project's motion/. The look-before-you-bake of the design loop: mid-entrance, settled, mid-exit. Check legibility, contrast, safe margins and timing before baking. Takes `preset` or `comp` (+ `project`), optional `ops`, and `width`/`height` to draw at another size.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionFrame,
        },
        Tool {
            name: "motion_bake",
            title: "Bake a motion comp",
            description: "Bakes a motion comp - `preset` or `comp`, edited by an optional `ops` batch (set-slot, set-text, set-words, set-track, slide-to, pop, fade-in, add-text, ...) - into PNG frames under the project's cache/comps/, content-addressed by the comp's hash, and with `encode: true` writes one video file under motion/ ready for media_import and clip_add (`name` picks its file name, with the comp's hash prepended when missing so export can find it; .webm is VP9, .mov ProRes 4444, otherwise H.264 - none carry alpha, which the PNG sequence keeps). For a transparent comp, take `dir` and `hash` from this reply to motion_overlay, which composites the RGBA frames over your real footage with the alpha honoured. Optional width/height - the project's own frame, read from its document, when absent, so a vertical project bakes vertical; clamped to 4096 for safety - fps, crf (0-51). A bake whose manifest already matches the comp, engine, size and rate is returned from the cache without re-rendering (cached: true in the reply).",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionBake,
        },
        Tool {
            name: "captions_animate",
            title: "Caption a transcript word by word",
            description: "A word-timed transcript in, one caption video out: words as [text, start, end] triples (or objects), broken into lines by the caption rules (word count, sentence ends, silences), each line one kinetic caption comp, the whole thing baked and encoded under the project's motion/ ready for media_import and clip_add at the voiceover's start. Word times must be measured (whisper segments with per-word estimates will not do); 1080x1920 30fps by default. Optional: preset (default funcut.caption.kinetic), wordsPerLine, maxLineSeconds, gapSeconds, leadSeconds, tailSeconds, width, height, fps, plate (a still the captions render over), name, curtain (true default; false gives hard cuts between lines, which read better when the reel sits over a voiceover).",
            read_only: false,
            destructive: false,
            idempotent: false,
            action: Action::CaptionsAnimate,
        },
        Tool {
            name: "analysis_wait",
            title: "Wait for a job",
            description: "Waits for any job - an analysis, a transcription, a voice, an export - to end, up to `timeoutSeconds` (60 by default, 600 at most), and returns its status; a finished analysis carries its JSON result in the status's `output`. If the job is still running when the time is up, call it again.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::ExportWait,
        },
        Tool {
            name: "watch_render",
            title: "Watch the cut",
            description: "LOOK at the edit. Samples true composited frames across the open project's timeline (or a rendered movie at `path`) and returns ONE labelled contact sheet as an inline image, with measured craft facts: black frames, letterbox bars, flat/low-contrast shots, overexposed shots, duplicate shots (full-frame comparison), static transitions, per-shot colour saturation, per-shot energy-centroid cropHint, and an audio energy envelope when a movie is given. Call this after every render, read the notes, fix the edit, and call it again. The sheet is also written under the project's cache/sense/ and its path returned as pngPath, which is how craft_critique sees the same frames. Optional: samples (12 default, 24 max), times[] (explicit sample times), duration, cols, tileWidth, tileHeight, audio, out (write the PNG there). Writes the sheet, so not read-only; every write is held to this session's roots.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::WatchRender,
        },
        Tool {
            name: "stock_contact_sheet",
            title: "Look at B-roll candidates",
            description: "SEE stock footage before choosing it. Give a `query` (openvid_search's words), `ids` from openvid_search, or local `paths`; each candidate's real first frame is decoded into one labelled contact sheet returned as an inline image, with per-candidate pixel metrics, a cropHint and a ready-to-use reframeHint.clipTransform offset pair for the target aspect. With `rank: true` a local vision model re-ranks the candidates against `brief` (or the project's creative_brief) and suggests better queries. Never choose B-roll from captions alone again. Optional: limit (8 default, 12 max), minAesthetic, minMotion, orientation, source, sort, cols, tileWidth, out.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::StockSheet,
        },
        Tool {
            name: "craft_critique",
            title: "Score the cut",
            description: "The measured creative critic. Takes the `sheet` JSON returned by watch_render (its pngPath is how the critic sees the frames; a raw `pngB64` also works) and a `brief`, and returns six honest 0-10 scores - hook, story, rhythm, shotChoice, design, sound - plus notes and concrete revisions. Baseline is REAL pixel/audio measurement: black frames, letterbox bars, flat and overexposed shots, duplicates, static transitions, opening-frame hook check, colour saturation (engine 'heuristic', confidence 0.5). With the bundled 0.5B local model it adds bounded visible-defect flags (engine 'local-vlm-flags', confidence 0.58, at most 1 point per axis: black frames, letterbox bars, readable text) - it CANNOT judge taste. Full six-axis VLM scoring (engine 'local-vlm', confidence 0.78) needs a stronger critic: local_vlm {action:'configure', provider, apiKey} with a free-tier provider (groq, gemini, zai, openrouter), or FUNCUT_VLM_ENDPOINT pointing at any OpenAI-compatible vision server. With `project` it journals every pass to craft-history.json and returns pass, previousOverall, delta, improved and trend - a file write, so the tool is not read-only. Call it after EVERY watch_render. Optional: autoSetup, profile.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::CraftCritique,
        },
        Tool {
            name: "suggest_cuts",
            title: "Find the musical cut points",
            description: "Cut on energy, not on a fixed grid. Decodes the soundtrack with ffmpeg (NO librosa/python extras needed, unlike analyze_beats), estimates BPM from the energy-onset function, and returns beat-aligned cutTimes spread across the WHOLE piece plus an energy curve. Optional: path (music or voiceover file), cuts (12 default, 64 max), minGap seconds (0.35), bpm (a hint when the estimate is wrong), autoSetup. Use the times as section boundaries in plan_validate; keep a few cuts deliberately off-beat for surprise. Honest approximation, not librosa-grade beat tracking. Not read-only: it runs a decoder, and with autoSetup (the default) it downloads a pinned static ffmpeg into the app's own data folder when no decoder is on the machine.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::SuggestCuts,
        },
        Tool {
            name: "measure_loudness",
            title: "Measure the mix",
            description: "PROVE loudness compliance instead of guessing. Runs FFmpeg's EBU R128 meter on `path` (an exported movie or any media with audio) and returns integrated LUFS, true peak dBTP, loudness range LU, the error against the platform target, whether the target was met, and specific mix notes. export_wait reports no loudness, so this is the headless way to check. `targetLufs` is -14 by default (short-form platforms; -23 is broadcast EBU R128) and `targetTruePeakDbtp` is -1. Fix with clip_audio_levels_db and measure again. Not read-only: it runs a decoder, and with autoSetup (the default) it downloads a pinned static ffmpeg into the app's own data folder when no decoder is on the machine.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MeasureLoudness,
        },
        Tool {
            name: "motion_overlay",
            title: "Composite motion over footage",
            description: "Fixes the opaque-caption-reel problem. Takes a baked RGBA PNG sequence (`frames`, or `hash` + `project` to find it) and composites it OVER real footage (`plate`, a video path) at `at` seconds, honouring the comp's transparency, and writes an ordinary H.264 clip ready for media_import and clip_add. Without a plate it composites over a solid `background` at the given width/height. The sidecar's video codecs cannot carry alpha and FFmpeg's WebM muxer does not reliably write VP9 AlphaMode, so this is the honest alpha path: transparency is honoured at the compositing step, not faked in an encoder. Optional: fps (30), width, height, at (0), crf (18), background, x, y, out (defaults to <project>/motion/overlay.mp4).",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::MotionOverlay,
        },
        Tool {
            name: "local_vlm",
            title: "Local or hosted vision critic",
            description: "The vision critic behind craft_critique, on any of three rungs. `action: \"status\"` reports what is reachable right now. `action: \"setup\"` downloads, once and into the app's own data folder (no system install, no pip), the 0.5B SmolVLM2 model + projector (~546 MB) and a pinned llama.cpp CPU server - a bounded flag detector, not a taste judge. `action: \"providers\"` lists the free-tier hosted vision APIs (groq, gemini, zai, openrouter) with their endpoints, default model ids and honest free-tier notes. `action: \"configure\"` with `provider` + `apiKey` wires one up in a single call (key stored in vlm.json in the app's data dir, masked in every reply; HOSTED means contact-sheet frames go to that provider's API - the local paths never leave the machine) and unlocks full six-axis VLM scoring in craft_critique. `action: \"clear\"` removes the saved config. FUNCUT_VLM_ENDPOINT / FUNCUT_VLM_API_KEY / FUNCUT_VLM_MODEL env vars do the same without a config file; any OpenAI-compatible server works.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::LocalVlm,
        },
        Tool {
            name: "emotion",
            title: "Read what the voiceover feels",
            description: "The emotional read of the voiceover that drives every layer. `action: \"extract\"` (default) takes `transcript` (a transcribe job's output), `lines` [{text,start,end}] or `script` text and returns PER LINE: one of 7 emotions with intensity, the act (hook/setup/tension/turn/reveal/proof/cta), a hormone shorthand, the TTS `delivery` and `speed`, the easing curve as exact concat-motion authoring JSON (tense lines ease in, warm ones ease out, a burst pops), colour temperature with a grade hint, the SFX register, the BGM duck depth in dB, pacing and 2-3 word stock `queries` - plus the whole piece's arc (shape, score, warnings) checked against the brief's emotionalArc. Engines: a stem lexicon always (confidence 0.55, English-only), or tabularisai/multilingual-emotion-classification - XLM-RoBERTa across 23 languages, its 11 labels folded onto the edit's 7 - on HuggingFace's free Inference API when FUNCUT_EMOTION_API_KEY / HF_TOKEN is set (75/25 blend, confidence 0.85; a non-English script without a key is answered with an honest note, not a silent neutral). `project` loads the creative brief automatically. `action: \"gate\"` checks stock candidates (openvid_search / stock_contact_sheet results) against a beat's `segment` {emotion, act} and `brand`: 0-1 failed checks passes, 2+ REJECTS the candidate, every candidate rejected = DROP THE BEAT - never pad with a bad match. `action: \"brand\"` validates a brand guide (`action: \"template\"` returns a ready-to-edit example) and applies it to lines. `action: \"verify\"` aligns the edit's words (`expected`) against the re-transcribed export (`actual`): what a render loses, duplicates or shifts (toleranceMs, default 150) is a defect the eye cannot see.",
            read_only: true,
            destructive: false,
            idempotent: true,
            action: Action::Emotion,
        },
        Tool {
            name: "creative_brief",
            title: "Set the creative intent",
            description: "Persist the WHY of the edit in the project folder so every turn works from the same intent. `action: \"set\"` merges `brief` into creative-brief.json; `action: \"get\"` reads it. Fields: audience, platform, goal, thesis, curiosityGap, payoff, emotionalArc, referenceStyle, tone, durationSeconds, brandKit, constraints, avoid. Returns a ready-to-use directorPrompt to give the AI director, and lists missingCoreFields. An edit without these fields is an edit without a point of view.",
            read_only: false,
            destructive: false,
            idempotent: true,
            action: Action::CreativeBrief,
        },
    ]
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
            Some(json!({
                "type": "object",
                "properties": properties,
                "required": ["queries"],
                "additionalProperties": false,
            }))
        }
        Action::MotionPresets
        | Action::MotionInspect
        | Action::MotionFrame
        | Action::MotionBake
        | Action::CaptionsAnimate => motion_schema(tool.name),
        Action::WatchRender
        | Action::StockSheet
        | Action::CraftCritique
        | Action::LocalVlm
        | Action::Emotion
        | Action::CreativeBrief
        | Action::MotionOverlay
        | Action::MeasureLoudness
        | Action::SuggestCuts => sense_schema(tool.name),
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
            "sheet": { "type": "object", "description": "The JSON watch_render or stock_contact_sheet returned; its pngPath is the image to judge." },
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
            "autoSetup": { "type": "boolean", "default": true },
        }),
        "measure_loudness" => json!({
            "path": string("The media file with an audio stream to measure."),
            "targetLufs": { "type": "number", "default": -14, "minimum": -70, "maximum": 0 },
            "targetTruePeakDbtp": { "type": "number", "default": -1, "minimum": -20, "maximum": 6 },
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
             set-text, set-words, move, retime, set-track, set-track-points, fade-in, \
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
                    ("name", json!({ "type": "string" })),
                ],
            ),
            &["project", "words"],
        ),
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
        Action::MotionBake => crate::motion::bake(arguments, backend),
        Action::CaptionsAnimate => crate::motion::captions_animate(arguments, backend),
        Action::WatchRender => crate::sense::watch_render(arguments, backend),
        Action::StockSheet => crate::sense::stock_contact_sheet(arguments, backend),
        Action::CraftCritique => crate::sense::craft_critique(arguments, backend),
        Action::LocalVlm => crate::sense::local_vlm(arguments),
        Action::Emotion => crate::sense::emotion(arguments),
        Action::CreativeBrief => crate::sense::creative_brief(arguments, backend),
        Action::MotionOverlay => crate::sense::motion_overlay(arguments, backend),
        Action::MeasureLoudness => crate::sense::measure_loudness(arguments),
        Action::SuggestCuts => crate::sense::suggest_cuts(arguments),
        Action::CollisionCheck => {
            let result = backend.call(request("project.get", arguments)?)?;
            Ok(collision_report(summary::view(&result)))
        }
        Action::OpenvidSearchBatch => openvid_search_batch(arguments, backend),
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
        Some(Value::Object(style)) => Value::Object(style),
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
    Ok(json!({
        "valid": true,
        "aspect": aspect,
        "segmentCount": segments.len(),
        "runtimeSeconds": runtime,
        "warnings": warnings,
        "creativeReady": creative["ready"],
        "creativeBlockers": creative["blockers"],
        "craftScore": creative["score"],
        "creativeNotes": creative["notes"],
        "next": "If creativeReady is false, call creative_brief, fix the blockers, and validate again. Structure is not craft.",
    }))
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
/// search would, not per query.
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
        let found = backend.call(request("openvid.search", Value::Object(shared.clone()))?)?;
        searches.push(json!({
            "query": query,
            "total": found.get("total").cloned().unwrap_or(Value::Null),
            "clips": found
                .get("clips")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        }));
    }
    Ok(Outcome::json(json!({ "searches": searches })))
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
                "motion_frame",
                json!({ "project": "/tmp/Cut", "preset": "funcut.lower-third.glass", "at": 1.0 }),
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
                Action::MotionFrame | Action::MotionBake | Action::CaptionsAnimate => {
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
                | Action::CreativeBrief
                | Action::MotionOverlay
                | Action::MeasureLoudness
                | Action::SuggestCuts => continue,
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

    /// The batch runs one `openvid.search` per query, carrying the shared
    /// filters, and the reply keeps the queries apart.
    #[test]
    fn openvid_search_batch_runs_one_search_per_query() {
        let mut backend = Answers(
            std::cell::RefCell::new(Vec::new()),
            [
                json!({ "clips": [{ "id": 1 }], "total": 1 }),
                json!({ "clips": [{ "id": 2 }, { "id": 3 }], "total": 2 }),
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
}
