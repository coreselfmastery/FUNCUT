// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! What crosses the API boundary: a [`Request`] in, a [`Response`] out,
//! and [`Event`]s along the way.
//!
//! All three are serde types with a fixed JSON shape, and that shape is the
//! contract: a transport is free to carry it over stdin, a socket or a
//! function call, but never to reinterpret it. Requests are tagged by
//! `method`, events by `event`, and a response is `{"result": ...}` or
//! `{"error": {"code": ..., "message": ...}}`. Fields are camelCase,
//! matching the document and the command layer, so a caller learns one
//! spelling. On a line or a socket the three travel inside the JSON-RPC 2.0
//! envelope [`crate::rpc`] describes, which adds an id to tie a response to
//! its request and nothing else.
//!
//! A method that takes longer than a caller should wait - an export - is a
//! job: the response names it at once, and everything that happens to it
//! comes back as [`Event`]s carrying that name, ending in `export.done` or
//! `export.failed`. A refusal is an [`ApiError`]: a [`ErrorCode`] a program
//! can branch on and the sentence a person would be shown.
//!
//! The edit vocabulary is not redefined here. [`Request::EditApply`] carries
//! a `concat_project` [`Command`] as it is, so every operation the window
//! can perform is one an API caller can perform, with the same clamps and
//! the same refusals, and a new command needs nothing added here.

use concat_host::export::Progress;
use concat_host::media::MediaSummary;
use concat_host::session::EditorView;
use concat_host::templates::TemplateInfo;
use concat_host::{AppDirs, ProjectInfo};
use concat_project::Command;
use concat_project::model::VideoSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version of this contract. Bumped when a method's shape changes in a
/// way a caller written against the previous one would misread; adding a
/// method or an optional field does not bump it.
pub const API_VERSION: &str = "0.2";

/// What a caller asks for. One variant per method, named `area.verb`.
///
/// Every method that touches a project names it by its folder, the same
/// path [`Request::ProjectOpen`] took; the API keeps one session per folder
/// and a method on a folder that is not open is an error, not a silent
/// open, so a caller always knows what it is editing.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all_fields = "camelCase")]
pub enum Request {
    /// The API's version, the build behind it, and what that build
    /// serves. The one method to call first: its reply's `capabilities`
    /// names the transports and optional features this server has, so a
    /// caller knows what to ask for before it asks.
    #[serde(rename = "version")]
    Version,

    /// Creates a project folder under `location`, named `name`, and opens
    /// it. Refuses a folder that already holds a project.
    #[serde(rename = "project.create")]
    ProjectCreate {
        /// The directory the project folder is made in.
        location: String,
        /// The project's name; the folder is named after it.
        name: String,
        /// Frame and rate; 1080p at 30 when absent.
        #[serde(default)]
        video: Option<VideoSettings>,
    },
    /// Opens a project folder. Opening one already open returns its state
    /// as it stands, edits and all.
    #[serde(rename = "project.open")]
    ProjectOpen {
        /// The project folder.
        path: String,
    },
    /// Closes an open project, saving first when asked. Unsaved edits are
    /// dropped otherwise.
    #[serde(rename = "project.close")]
    ProjectClose {
        /// The project folder.
        path: String,
        /// Whether to write the document before closing.
        #[serde(default)]
        save: bool,
    },
    /// The projects this machine opened most recently, newest first.
    #[serde(rename = "project.list")]
    ProjectList,
    /// The state of an open project.
    #[serde(rename = "project.get")]
    ProjectGet {
        /// The project folder.
        path: String,
    },
    /// The document exactly as a save would write it.
    #[serde(rename = "project.document")]
    ProjectDocument {
        /// The project folder.
        path: String,
    },
    /// Writes the document to the project folder.
    #[serde(rename = "project.save")]
    ProjectSave {
        /// The project folder.
        path: String,
        /// A new name for the project, when renaming.
        #[serde(default)]
        name: Option<String>,
    },
    /// Sets the active timeline's frame and rate, as an undoable edit.
    #[serde(rename = "project.setVideo")]
    ProjectSetVideo {
        /// The project folder.
        path: String,
        /// The frame and rate.
        video: VideoSettings,
    },

    /// Applies one edit command. The command's own notes say what each
    /// does; a refusal comes back as the sentence the window would show.
    #[serde(rename = "edit.apply")]
    EditApply {
        /// The project folder.
        path: String,
        /// The edit. Boxed because a command is the largest thing a
        /// request carries; the JSON is the command as it is.
        command: Box<Command>,
    },
    /// Steps the history back one edit.
    #[serde(rename = "edit.undo")]
    EditUndo {
        /// The project folder.
        path: String,
    },
    /// Steps the history forward one edit.
    #[serde(rename = "edit.redo")]
    EditRedo {
        /// The project folder.
        path: String,
    },

    /// Reports what is inside a media file without touching any project.
    #[serde(rename = "media.probe")]
    MediaProbe {
        /// The file.
        path: String,
    },
    /// Probes a file and adds it to a project's bin: what dropping a file
    /// on the window does. A path already in the bin is a no-op.
    #[serde(rename = "media.import")]
    MediaImport {
        /// The project folder.
        path: String,
        /// The file to import.
        file: String,
    },

    /// The OpenVid stock library's state: whether its local index is on
    /// this machine yet, how far along it is while it comes down, and how
    /// many clips it holds when it is ready. The one `openvid` method that
    /// works before the index is down; every other one refuses until it
    /// is, with words that say so.
    #[serde(rename = "openvid.status")]
    OpenvidStatus,

    /// Starts the OpenVid index's one-time download in the background and
    /// returns at once; `openvid.status` reports how far it has got. The
    /// index is ~435 MB, checked against a digest on arrival. Nothing is
    /// downloaded by any other method: they refuse until this has finished.
    #[serde(rename = "openvid.prepare")]
    OpenvidPrepare,

    /// Searches the OpenVid stock library - 937,957 stock clips with
    /// captions and quality scores - and returns the best matches with
    /// everything an agent needs to pick one without seeing it: the
    /// caption, the scores, the camera motion, the duration, the fetch
    /// cost in bytes. Every filter is optional; a search with none of them
    /// set is the library as it comes, which is also what `sort` is for.
    #[serde(rename = "openvid.search")]
    OpenvidSearch {
        /// Words to find in captions. ANDed and prefix-matched:
        /// "ocean sunset" finds captions holding both a word starting
        /// "ocean" and one starting "sunset", best matches first.
        #[serde(default)]
        query: Option<String>,
        /// Aesthetic score (0-6, higher is better-looking) at least this.
        #[serde(default)]
        min_aesthetic: Option<f64>,
        /// Motion score at least this (0-131 in this library).
        #[serde(default)]
        min_motion: Option<f64>,
        /// Motion score at most this.
        #[serde(default)]
        max_motion: Option<f64>,
        /// Temporal consistency (0-1, higher is steadier) at least this.
        #[serde(default)]
        min_temporal: Option<f64>,
        /// Seconds at least this.
        #[serde(default)]
        min_seconds: Option<f64>,
        /// Seconds at most this.
        #[serde(default)]
        max_seconds: Option<f64>,
        /// Frames a second at least this.
        #[serde(default)]
        min_fps: Option<f64>,
        /// Only this camera motion, exactly as the library names it:
        /// "static", "zoom_in", "pan_left", "tilt_up",
        /// "pan_left+tilt_up", "Undetermined", ...
        #[serde(default)]
        camera: Option<String>,
        /// Only this shelf: "pixabay", "pexels", "celebv" or "other".
        #[serde(default)]
        source: Option<String>,
        /// Only clips whose measured frame size is this shape:
        /// "vertical", "horizontal" or "square". A frame size is measured
        /// by reading the clip's own MP4 header the first time it is
        /// asked for, so this filter narrows to the clips measured so far;
        /// `openvid.status` says how many that is. What the measurements
        /// say: the pixabay, pexels and celebv shelves are 512x512
        /// squares, the other (WebVid) shelf is 720p and 1080p landscape,
        /// so "horizontal" finds that one and "vertical" nothing yet -
        /// each answer honest rather than everything returned quietly.
        #[serde(default)]
        orientation: Option<String>,
        /// How to order: "relevance" (the default with `query`),
        /// "aesthetic", "motion", "temporal", "shortest", "longest" or
        /// "surprise" (a different order every call).
        #[serde(default)]
        sort: Option<String>,
        /// At most this many clips, 1-100; 20 when absent.
        #[serde(default)]
        limit: Option<u32>,
        /// Skip this many from the front, for the next page.
        #[serde(default)]
        offset: Option<u32>,
    },

    /// Everything about one or more clips by id, including the frame size
    /// each clip's own MP4 header names - measured on first ask, one small
    /// network read a clip, and remembered after. At most 20 ids a call.
    #[serde(rename = "openvid.get")]
    OpenvidGet {
        /// The clip ids, as `openvid.search` gave them.
        ids: Vec<i64>,
    },

    /// Fetches one clip from the Hub into the app's own cache - a single
    /// ranged read of the clip's own bytes, `clip.bytes` of them - and
    /// says where it landed. Nothing is added to any project; import the
    /// file with `openvid.import` (or `media.import` on the path) when it
    /// is wanted on a timeline.
    #[serde(rename = "openvid.fetch")]
    OpenvidFetch {
        /// The clip's id.
        id: i64,
    },

    /// Fetches clips from the Hub and adds them to an open project's bin,
    /// each copied into the project's own `openvid` folder first so the
    /// project owns its files. With `start` they are also laid on one
    /// lane in the order given, as one undoable step; without it they
    /// wait in the bin for `clips_add_sequence` or `clip_add`. Returns
    /// the media ids they were given.
    #[serde(rename = "openvid.import")]
    OpenvidImport {
        /// The project folder.
        path: String,
        /// The clip ids, in the order they should play.
        ids: Vec<i64>,
        /// Lay them on the timeline from this moment, in sequence.
        #[serde(default)]
        start: Option<f64>,
        /// The lane to lay them on; the first lane with room when absent.
        #[serde(default)]
        track: Option<String>,
    },

    /// Searches the built-in sound library - whooshes, impacts, risers,
    /// ambiences, music and UI sounds, every one CC0 - and returns the
    /// matches with everything an agent needs to pick one without hearing
    /// it: the name, the category, the duration, the fetch cost in bytes,
    /// and whether the file is already on this machine. `query` words are
    /// ANDed and prefix-matched against the name, the id, the category and
    /// each sound's tags, so "cinematic impact" finds an impact tagged
    /// cinematic; `category` is one of Whoosh, Impact, Riser, Ambience,
    /// Music or UI, case-insensitive; `limit` (1-100, 20 when absent) and
    /// `offset` page through them. With nothing set, the whole library in
    /// manifest order.
    #[serde(rename = "sounds.search")]
    SoundsSearch {
        /// Words to find, ANDed and prefix-matched against each sound's
        /// name, id, category and tags.
        #[serde(default)]
        query: Option<String>,
        /// Only this category, case-insensitive: Whoosh, Impact, Riser,
        /// Ambience, Music or UI.
        #[serde(default)]
        category: Option<String>,
        /// At most this many sounds, 1-100; 20 when absent.
        #[serde(default)]
        limit: Option<u32>,
        /// Skip this many from the front, for the next page.
        #[serde(default)]
        offset: Option<u32>,
    },

    /// Adds a built-in sound to an open project: downloaded into the
    /// app's cache when it is not there yet (checked against the
    /// manifest's digest), copied into the project's own `audio` folder so
    /// the project owns its files, and imported into the bin. With `start`
    /// it is also laid on a lane as one undoable step - `track` names the
    /// lane, or the first with room when absent; without it, it waits in
    /// the bin for `clip_add`. Returns the media id, the clip id when one
    /// was laid, and where the copy landed.
    #[serde(rename = "sounds.add")]
    SoundsAdd {
        /// The project folder.
        path: String,
        /// The sound's id, as `sounds.search` gave it.
        id: String,
        /// Lay it on the timeline at this moment.
        #[serde(default)]
        start: Option<f64>,
        /// The lane to lay it on; the first lane with room when absent.
        #[serde(default)]
        track: Option<String>,
    },

    /// Every effect package the build knows, with its parameters, so a
    /// The background-music library's state: both shelves - FreePD (the
    /// Internet Archive's mirror of Kevin MacLeod's CC0 library, ~1,000
    /// tracks) and Lo-Fi (Moby's MobyGratis instrumentals as the OpenLo-Fi
    /// pack, 166 tracks) - each "missing" (its list is not on this machine
    /// yet; `bgm.prepare` brings it, a few MB and a one-time measuring of
    /// the Lo-Fi zip, then poll this until "ready"), "loading" (with
    /// `percent`, the Lo-Fi census), "ready" (with `tracks`, how many it
    /// holds) or "failed" (with `error"). The one `bgm` method that works
    /// before a shelf is ready.
    #[serde(rename = "bgm.status")]
    BgmStatus,

    /// Starts both shelves' one-time catalog fetches in the background and
    /// returns at once; `bgm.status` reports how far they have got. The
    /// lists are a few MB and are kept, so a second call on a warm machine
    /// does nothing; every other `bgm` method refuses until the shelf it
    /// reads is ready, with words that say so.
    #[serde(rename = "bgm.prepare")]
    BgmPrepare,

    /// Searches the background-music library - 1,191 whole CC0 tracks
    /// across both shelves, with titles, artists, categories and lengths -
    /// and returns the best matches with everything a caller needs to pick
    /// one without hearing it: the artist, the grouping, the seconds, the
    /// fetch cost in bytes, whether it is already on this machine. Every
    /// filter is optional; a search with none of them set is the library
    /// as it comes, which is also what `sort` is for.
    #[serde(rename = "bgm.search")]
    BgmSearch {
        /// Words to find in titles, artists and categories. ANDed and
        /// prefix-matched: "city rain" finds tracks holding both a word
        /// starting "city" and one starting "rain", best matches first.
        #[serde(default)]
        query: Option<String>,
        /// One shelf only: "freepd" or "lofi"; both when absent.
        #[serde(default)]
        source: Option<String>,
        /// One category only, as the shelf spells it: FreePD's folders
        /// ("upbeat", "epic", "comedy", "scoring", "world", …) or the
        /// Lo-Fi pack's own ("chillhop", "late-night",
        /// "ambient-lofi", …); any when absent.
        #[serde(default)]
        category: Option<String>,
        /// Seconds at least this.
        #[serde(default)]
        min_seconds: Option<f64>,
        /// Seconds at most this.
        #[serde(default)]
        max_seconds: Option<f64>,
        /// How to order: "relevance" (the default with `query`),
        /// "shortest", "longest" or "surprise" (a different order every
        /// call). A track whose length is unknown goes last, either way
        /// round, rather than first.
        #[serde(default)]
        sort: Option<String>,
        /// At most this many tracks, 1-100; 20 when absent.
        #[serde(default)]
        limit: Option<u32>,
        /// Skip this many from the front, for the next page.
        #[serde(default)]
        offset: Option<u32>,
    },

    /// Fetches one track into the app's own cache - FreePD as its own
    /// file from the Archive, Lo-Fi as one ranged read of exactly that
    /// track's span out of the pack's 554 MB zip, inflated and checked
    /// against the CRC the zip itself carries - and says where it landed.
    /// `track.bytes` of bandwidth, nothing else. Nothing is added to any
    /// project; import the file with `bgm.import` (or `media.import` on
    /// the path) when it is wanted on a timeline.
    #[serde(rename = "bgm.fetch")]
    BgmFetch {
        /// The track's id, as `bgm.search` gave it.
        id: String,
    },

    /// Fetches tracks and adds them to an open project's bin, each copied
    /// into the project's own `bgm` folder first so the project owns its
    /// files. With `start` they are also laid on one lane in the order
    /// given, as one undoable step; without it they wait in the bin for
    /// `clips_add_sequence` or `clip_add`. Returns the media ids they were
    /// given. At most 20 tracks a call.
    #[serde(rename = "bgm.import")]
    BgmImport {
        /// The project folder.
        path: String,
        /// The track ids, in the order they should play.
        ids: Vec<String>,
        /// Lay them on the timeline from this moment, in sequence.
        #[serde(default)]
        start: Option<f64>,
        /// The lane to lay them on; the first lane with room when absent.
        #[serde(default)]
        track: Option<String>,
    },

    /// Every effect package the build knows, with its parameters, so a
    /// caller can build a valid chain without reading a manifest.
    #[serde(rename = "catalogue.list")]
    CatalogueList {
        /// Restrict to one kind: "effect", "filter", "audio", "transition"
        /// or "generator".
        #[serde(default)]
        kind: Option<String>,
    },

    /// The template library.
    #[serde(rename = "template.list")]
    TemplateList,
    /// Makes a project from a template with every slot filled, and opens
    /// it. Every slot must be filled; a set that leaves one empty makes
    /// nothing.
    #[serde(rename = "template.instantiate")]
    TemplateInstantiate {
        /// The template bundle folder.
        template: String,
        /// The directory the project folder is made in.
        location: String,
        /// The project's name.
        name: String,
        /// The file for each slot.
        fills: Vec<Fill>,
    },
    /// Packs an open project into a new template bundle.
    #[serde(rename = "template.save")]
    TemplateSave {
        /// The project folder.
        path: String,
        /// The template's name.
        name: String,
    },

    /// Renders an open project's active timeline to a file, exactly as
    /// the window's Export does: cutouts analysed, stabilized clips'
    /// camera shake measured, titles painted, then the
    /// frame loop and the mix. Returns at once with the job's name; the
    /// render runs on its own thread and reports through [`Event`]s, ending
    /// in [`Event::ExportDone`] or [`Event::ExportFailed`]. One export runs
    /// at a time; a second is refused as [`ErrorCode::Busy`].
    #[serde(rename = "export.run")]
    ExportRun {
        /// The project folder.
        path: String,
        /// How to render it.
        #[serde(flatten)]
        spec: ExportSpec,
    },
    /// Stops a running export at its next frame. The job then ends with
    /// [`Event::ExportFailed`] carrying [`ErrorCode::Cancelled`].
    #[serde(rename = "export.cancel")]
    ExportCancel {
        /// The job [`Started`] named.
        job: String,
    },
    /// How a job stands: running and how far along, or how it ended. The
    /// same news the events carry, for a caller that was not listening
    /// when it came; the last few finished jobs are remembered.
    #[serde(rename = "export.status")]
    ExportStatus {
        /// The job [`Started`] named.
        job: String,
    },

    /// Composites the true frame at one instant. Written as a PNG to
    /// `output` when there is one, and handed back inline as a
    /// [`Picture`] otherwise, for a caller on the far side of a socket.
    #[serde(rename = "preview.frame")]
    PreviewFrame {
        /// The project folder.
        path: String,
        /// The timeline instant, in seconds.
        time: f64,
        /// The file to write; absent means the picture comes back inline.
        #[serde(default)]
        output: Option<String>,
        /// Frame width; the timeline's when absent.
        #[serde(default)]
        width: Option<u32>,
        /// Frame height; the timeline's when absent.
        #[serde(default)]
        height: Option<u32>,
    },
}

/// One template slot and the file that takes it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    /// The placeholder's media id in the template.
    pub media_id: String,
    /// The file to put there; probed on the way in.
    pub file: String,
}

/// How an export is rendered. Everything but the output is optional and
/// defaults to what the window's sheet defaults to.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSpec {
    /// The file to write.
    pub output: String,
    /// Constant rate factor; lower is better and bigger. 20 when absent.
    #[serde(default)]
    pub crf: Option<u8>,
    /// The x264 preset name. "medium" when absent.
    #[serde(default)]
    pub preset: Option<String>,
    /// Output width; the timeline's when absent.
    #[serde(default)]
    pub width: Option<u32>,
    /// Output height; the timeline's when absent.
    #[serde(default)]
    pub height: Option<u32>,
    /// Frame rate numerator; the timeline's when absent.
    #[serde(default)]
    pub rate_num: Option<i64>,
    /// Frame rate denominator; the timeline's when absent.
    #[serde(default)]
    pub rate_den: Option<i64>,
    /// "h264", "hevc" or "av1". H.264 when absent - HEVC for an HDR file,
    /// which H.264 does not carry.
    #[serde(default)]
    pub codec: Option<String>,
    /// Ten bits a channel. Eight when absent.
    #[serde(default)]
    pub ten_bit: Option<bool>,
    /// "limited" (16-235, what every player and YouTube expect) or "full"
    /// (0-255, for screen content bound for a PC player that reads the
    /// tag). Limited when absent.
    /// https://github.com/jub0t/Concat/issues/103
    #[serde(default)]
    pub color_range: Option<String>,
    /// For an HDR timeline: written as HDR, its own HLG or PQ in ten bits
    /// (true, the default), or tone-mapped to SDR (false). Nothing for an
    /// SDR timeline.
    #[serde(default)]
    pub hdr: Option<bool>,
}

/// What a request hands back. Serialised as the payload alone: the variant
/// is implied by the method, so a caller that sent `project.get` reads an
/// editor view and nothing wraps it.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum Reply {
    /// [`Request::Version`].
    Version(VersionInfo),
    /// The project after a change, or as it stands.
    View(Box<EditorView>),
    /// [`Request::ProjectList`].
    Projects(Vec<ProjectInfo>),
    /// [`Request::ProjectDocument`].
    Document(Value),
    /// [`Request::MediaProbe`].
    Media(MediaSummary),
    /// [`Request::OpenvidStatus`] and [`Request::OpenvidPrepare`].
    Openvid(OpenvidState),
    /// [`Request::OpenvidSearch`].
    OpenvidFound(OpenvidFound),
    /// [`Request::OpenvidGet`] and [`Request::OpenvidFetch`].
    OpenvidClips(Vec<OpenvidClip>),
    /// [`Request::OpenvidImport`].
    OpenvidImported(OpenvidImport),
    /// [`Request::SoundsSearch`].
    SoundsFound(SoundsFound),
    /// [`Request::SoundsAdd`].
    SoundsAdded(SoundsAdded),
    /// [`Request::BgmStatus`] and [`Request::BgmPrepare`].
    Bgm(BgmState),
    /// [`Request::BgmSearch`].
    BgmFound(BgmFound),
    /// [`Request::BgmFetch`].
    BgmTrack(BgmTrack),
    /// [`Request::BgmImport`].
    BgmImported(BgmImport),
    /// [`Request::CatalogueList`].
    Packages(Vec<PackageInfo>),
    /// [`Request::TemplateList`].
    Templates(Vec<TemplateInfo>),
    /// [`Request::TemplateSave`].
    Template(TemplateInfo),
    /// [`Request::ExportRun`]: the job that is now running.
    Started(Started),
    /// [`Request::ExportStatus`].
    Job(JobStatus),
    /// [`Request::PreviewFrame`] with an output: the file written.
    Written(Written),
    /// [`Request::PreviewFrame`] without one: the picture itself.
    Picture(Picture),
    /// Methods with nothing to say beyond having worked.
    Done(Done),
}

/// The OpenVid library's state, as `openvid.status` reports it and
/// `openvid.prepare` starts it towards.
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenvidState {
    /// "missing" (the index is not on this machine yet; `openvid.prepare`
    /// brings it), "downloading", "ready", or "failed" (with `error`).
    pub state: String,
    /// How far the download has got, 0-100.
    pub percent: u32,
    /// How many clips the library holds, when it is ready.
    pub rows: Option<u64>,
    /// How many clips' frame sizes have been measured so far - the honest
    /// number behind the orientation filter.
    pub measured: u64,
    /// Why the library is not usable, when it is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `openvid.search` found.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenvidFound {
    /// The clips, `limit` of them from `offset` in the order asked for.
    pub clips: Vec<OpenvidClip>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// One clip of the OpenVid library, as every `openvid` method hands it
/// back. Everything but `localPath` comes from the library's index;
/// `localPath` says where the clip's MP4 was fetched to, when it has been.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenvidClip {
    /// The clip's id in the library, and the id every verb takes back.
    pub id: i64,
    /// The clip's file name, without its extension.
    pub name: String,
    /// What the clip shows, in the dataset's own words.
    pub caption: String,
    /// Visual quality, 0-6, higher better.
    pub aesthetic: f64,
    /// How much moves, higher more.
    pub motion: f64,
    /// Frame-to-frame stability, 0-1, higher steadier.
    pub temporal: f64,
    /// "static", "zoom_in", "pan_left+tilt_up", ... .
    pub camera: String,
    /// Frames a second.
    pub fps: f64,
    /// Seconds.
    pub seconds: f64,
    /// The MP4's size in bytes: what fetching the clip costs.
    pub bytes: u64,
    /// The shelf the clip came from: "pixabay", "pexels", "celebv" or
    /// "other".
    pub source: String,
    /// Width and height once read from the clip's own MP4 header, and not
    /// before: `[512, 512]`, or absent when not yet measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dims: Option<(u32, u32)>,
    /// Where the clip's MP4 is on this machine, once fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
}

impl OpenvidClip {
    /// The library's own clip, as the API hands it back: the same
    /// numbers, the shelf and the frame size as words, and - when the
    /// clip's bytes are on this machine - where they are.
    pub fn of(
        clip: &concat_host::openvid::Clip,
        dims: Option<(u32, u32)>,
        local: Option<&std::path::Path>,
    ) -> OpenvidClip {
        OpenvidClip {
            id: clip.id,
            name: clip.name.clone(),
            caption: clip.caption.clone(),
            aesthetic: clip.aesthetic,
            motion: clip.motion,
            temporal: clip.temporal,
            camera: clip.camera.clone(),
            fps: clip.fps,
            seconds: clip.seconds,
            bytes: clip.bytes,
            source: match clip.source {
                concat_host::openvid::Source::Pixabay => "pixabay",
                concat_host::openvid::Source::Pexels => "pexels",
                concat_host::openvid::Source::CelebV => "celebv",
                concat_host::openvid::Source::Other => "other",
            }
            .to_owned(),
            dims,
            local_path: local.map(|path| path.to_string_lossy().into_owned()),
        }
    }
}

/// What `openvid.import` did.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenvidImport {
    /// The media ids the clips were given in the project's bin, in the
    /// order the ids were given in.
    pub media_ids: Vec<String>,
    /// The project as it now stands.
    pub view: Box<EditorView>,
}

/// What `sounds.search` found.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundsFound {
    /// The sounds, `limit` of them from `offset`, in manifest order.
    pub sounds: Vec<SoundEntry>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// One sound of the built-in library, as every `sounds` method hands it
/// back. Everything but `localPath` comes from the manifest; `localPath`
/// says where the sound's file is on this machine, when it has been
/// downloaded already.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundEntry {
    /// The sound's id in the library, and the id `sounds.add` takes back.
    pub id: String,
    /// What the shelf's row shows.
    pub name: String,
    /// Whoosh, Impact, Riser, Ambience, Music or UI.
    pub category: String,
    /// Words the search narrows by, beyond the name.
    pub tags: Vec<String>,
    /// Seconds.
    pub duration: f64,
    /// The file's size in bytes: what downloading the sound costs.
    pub bytes: u64,
    /// CC0-1.0: the licence every sound in the library carries.
    pub license: String,
    /// Who made it.
    pub author: String,
    /// Where the sound's file is on this machine, once downloaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
}

/// What `sounds.add` did.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundsAdded {
    /// The media id the sound was given in the project's bin.
    pub media_id: String,
    /// The clip's id, when the sound was laid on a lane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_id: Option<String>,
    /// Where the sound's copy landed inside the project.
    pub file: String,
    /// The project as it now stands.
    pub view: Box<EditorView>,
}

/// The background-music library's state, as `bgm.status` reports it and
/// `bgm.prepare` starts it towards. Both shelves, because a search reads
/// both.
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgmState {
    /// The FreePD shelf (the Internet Archive's mirror of Kevin MacLeod's
    /// CC0 library): "missing", "loading" (with that shelf's `percent`),
    /// "ready" or "failed" (with `freepdError`).
    pub freepd: String,
    /// The Lo-Fi shelf (Moby's MobyGratis instrumentals, as the OpenLo-Fi
    /// pack): "missing", "loading" (with that shelf's `percent`), "ready"
    /// or "failed" (with `lofiError`).
    pub lofi: String,
    /// How far the slower shelf's one-time work has got, 0-100: the Lo-Fi
    /// census, which measures every track's place and length out of the
    /// pack's zip with one small ranged read a track.
    pub percent: u32,
    /// How many tracks the FreePD shelf holds, when it is ready.
    pub freepd_tracks: Option<u64>,
    /// How many tracks the Lo-Fi shelf holds, when it is ready.
    pub lofi_tracks: Option<u64>,
    /// Why a shelf is not usable, when it is not. The shelf's own name
    /// prefixes the words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `bgm.search` found.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgmFound {
    /// The tracks, `limit` of them from `offset` in the order asked for.
    pub tracks: Vec<BgmTrack>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// One track of the background-music library, as every `bgm` method hands
/// it back. Everything but `localPath` comes from the shelf's catalog; the
/// bytes arrive only when the track is fetched.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgmTrack {
    /// The track's id, and the id every verb takes back.
    pub id: String,
    /// Which shelf the track is on: "freepd" or "lofi".
    pub source: String,
    /// What the card says.
    pub title: String,
    /// Who made it, when the catalog says.
    pub artist: String,
    /// The shelf's own grouping, lowercase: "upbeat", "comedy",
    /// "late-night", ... .
    pub category: String,
    /// Seconds, as the catalog gives them; `0` when the Lo-Fi census
    /// could not measure a track, and a duration filter passes such a
    /// track by rather than guessing at it.
    pub seconds: f64,
    /// The track's size in bytes: what fetching it costs.
    pub bytes: u64,
    /// Where the track's file is on this machine, once fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
}

impl BgmTrack {
    /// The library's own track, as the API hands it back: the same
    /// numbers, the shelf as a word, and - when the track's bytes are on
    /// this machine - where they are.
    pub fn of(track: &concat_host::bgm::Track, local: Option<&std::path::Path>) -> BgmTrack {
        BgmTrack {
            id: track.id.clone(),
            source: match track.source {
                concat_host::bgm::Source::FreePd => "freepd",
                concat_host::bgm::Source::Lofi => "lofi",
            }
            .to_owned(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            category: track.category.clone(),
            seconds: track.duration,
            bytes: track.bytes,
            local_path: local.map(|path| path.to_string_lossy().into_owned()),
        }
    }
}

/// What `bgm.import` did.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgmImport {
    /// The media ids the tracks were given in the project's bin, in the
    /// order the ids were given in.
    pub media_ids: Vec<String>,
    /// The project as it now stands.
    pub view: Box<EditorView>,
}

/// The build and the contract it speaks.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// [`API_VERSION`].
    pub api_version: String,
    /// The Concat build.
    pub concat: String,
    /// Where this machine keeps recents, templates and models.
    pub dirs: Dirs,
    /// What this build serves, as names a caller tests for membership:
    /// what the API itself provides in every build, `events`, and what
    /// only some builds do, `gpu` when frames composite on one; then what
    /// the transport around it adds, `json-rpc`, `unix-socket` and `grpc`
    /// for each of the socket transports listening. A name absent from
    /// the list is not served, whatever the build; a name added later is
    /// not a version bump, so a caller ignores names it does not know.
    pub capabilities: Vec<String>,
}

/// The app's directories, as paths.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dirs {
    /// Small state: recents, settings, the template library.
    pub config: String,
    /// Large state: downloaded models, painted titles.
    pub data: String,
}

impl From<&AppDirs> for Dirs {
    fn from(dirs: &AppDirs) -> Self {
        Dirs {
            config: dirs.config.to_string_lossy().into_owned(),
            data: dirs.data.to_string_lossy().into_owned(),
        }
    }
}

/// A file the API wrote.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    /// The file's path.
    pub path: String,
    /// Its picture size.
    pub width: u32,
    /// Its picture size.
    pub height: u32,
}

/// A job the API has begun. Every event about it carries `job`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The job's name, unique for the life of the API: "j1", "j2", ...
    pub job: String,
    /// The project it works on.
    pub path: String,
    /// The file it will write.
    pub output: String,
}

/// Where a job is in its life.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    /// Still working.
    Running,
    /// Wrote its file.
    Done,
    /// Stopped by [`Request::ExportCancel`].
    Cancelled,
    /// Ended without its file; `error` says why.
    Failed,
}

/// How one job stands: [`Request::ExportStatus`]'s reply.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    /// The job's name.
    pub job: String,
    /// The project it works on.
    pub path: String,
    /// The file it writes.
    pub output: String,
    /// Running, or how it ended.
    pub state: JobState,
    /// What it is doing: "cutout" while masks are found, "stabilize" while
    /// shake is measured, then "rendering", "mixing audio" and "muxing".
    /// Absent until it has said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// Frames rendered, in the video stage.
    pub frame: i64,
    /// Frames in all, in the video stage; 0 until known.
    pub total: i64,
    /// How far along the current stage is, `0..=1`; 1 once done.
    pub fraction: f64,
    /// The written file's picture size, once done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// The written file's picture size, once done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Why it failed, or that it was cancelled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

impl JobStatus {
    /// A job that has just begun.
    pub fn running(job: &str, path: &str, output: &str) -> JobStatus {
        JobStatus {
            job: job.to_owned(),
            path: path.to_owned(),
            output: output.to_owned(),
            state: JobState::Running,
            stage: None,
            frame: 0,
            total: 0,
            fraction: 0.0,
            width: None,
            height: None,
            error: None,
        }
    }
}

/// A frame handed back without touching the disk.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    /// Its picture size.
    pub width: u32,
    /// Its picture size.
    pub height: u32,
    /// The PNG, base64 in the standard alphabet with padding.
    pub png: String,
}

/// The empty reply, an object so every reply is one.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Done {}

/// One effect package, as a caller building a chain needs it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    /// The id a clip's chain stores, `author.name`.
    pub id: String,
    /// What the catalogue card says.
    pub name: String,
    /// "effect", "filter", "audio", "transition" or "generator".
    pub kind: String,
    /// The shelf the card sits on.
    pub category: String,
    /// A sentence about it.
    pub description: String,
    /// The parameter the simple view shows as the one slider, if any.
    pub intensity: Option<String>,
    /// The knobs, in the order the inspector shows them.
    pub params: Vec<ParamInfo>,
}

/// One parameter of a package.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamInfo {
    /// The key a chain entry's `params` stores it under.
    pub key: String,
    /// What the control is labelled.
    pub label: String,
    /// "float", "int", "bool", "enum", "color" or "point".
    #[serde(rename = "type")]
    pub kind: String,
    /// Lowest value.
    pub min: f64,
    /// Highest value.
    pub max: f64,
    /// The value an untouched control means.
    pub default: f64,
    /// Slider increment; 0 means continuous.
    pub step: f64,
    /// Displayed after the number.
    pub unit: String,
    /// Whether the control can carry keyframes.
    pub animate: bool,
    /// For "enum": the values the document may hold.
    pub values: Vec<f64>,
    /// For "enum": what each value is called, in `values` order.
    pub labels: Vec<String>,
}

/// Why a request has no reply, as a program reads it. The JSON-RPC number
/// each maps to is [`ErrorCode::number`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The line was not JSON.
    Parse,
    /// The JSON was not a request: an unknown method, a missing or
    /// ill-typed field, a value out of range.
    Invalid,
    /// The project named is not open; open it first.
    NotOpen,
    /// A job, template or package named does not exist.
    NotFound,
    /// The edit layer said no. The message is the sentence the window
    /// would show, and the state is as it was.
    Refused,
    /// A one-at-a-time job is already running.
    Busy,
    /// The job was stopped by [`Request::ExportCancel`].
    Cancelled,
    /// The transport wanted a token and did not get the right one. Never
    /// raised by the API itself.
    Unauthorized,
    /// Everything else: a file that could not be read or written, a decode
    /// that failed, a model that did not download.
    Failed,
}

impl ErrorCode {
    /// The code's JSON-RPC 2.0 number: the standard ones for the standard
    /// meanings, and server-defined ones (-32000 downwards) for the rest.
    pub fn number(self) -> i64 {
        match self {
            ErrorCode::Parse => -32700,
            ErrorCode::Invalid => -32600,
            ErrorCode::Failed => -32000,
            ErrorCode::NotOpen => -32001,
            ErrorCode::NotFound => -32002,
            ErrorCode::Refused => -32003,
            ErrorCode::Busy => -32004,
            ErrorCode::Cancelled => -32005,
            ErrorCode::Unauthorized => -32006,
        }
    }
}

/// A refusal: what kind, and the sentence a person would be shown.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    /// What kind of refusal.
    pub code: ErrorCode,
    /// The sentence.
    pub message: String,
}

impl ApiError {
    /// An error of `code` saying `message`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ApiError {
        ApiError {
            code,
            message: message.into(),
        }
    }

    /// [`ErrorCode::Invalid`].
    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::Invalid, message)
    }

    /// [`ErrorCode::Failed`].
    pub fn failed(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::Failed, message)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

/// A request's outcome. `{"result": ...}` when it worked, `{"error":
/// {"code": ..., "message": ...}}` when it did not.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum Response {
    /// The reply.
    Result(Reply),
    /// Why there is none.
    Error(ApiError),
}

impl From<Result<Reply, ApiError>> for Response {
    fn from(result: Result<Reply, ApiError>) -> Self {
        match result {
            Ok(reply) => Response::Result(reply),
            Err(error) => Response::Error(error),
        }
    }
}

/// Something that happened to a job. A transport forwards these to every
/// caller as they come; each names its job and its project, so a caller
/// keeps the ones it asked for and ignores the rest.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all_fields = "camelCase")]
pub enum Event {
    /// A cutout's masks are being found ahead of an export.
    #[serde(rename = "cutout.progress")]
    CutoutProgress {
        /// The job.
        job: String,
        /// The project folder.
        path: String,
        /// The media being analysed.
        media_id: String,
        /// True while a model downloads, false while it runs.
        fetching: bool,
        /// How far along, `0..=1`.
        fraction: f32,
    },
    /// A stabilized clip's media is being measured ahead of an export.
    #[serde(rename = "stabilize.progress")]
    StabilizeProgress {
        /// The job.
        job: String,
        /// The project folder.
        path: String,
        /// The media being measured.
        media_id: String,
        /// How far along, `0..=1`.
        fraction: f32,
    },
    /// The export moved on.
    #[serde(rename = "export.progress")]
    ExportProgress {
        /// The job.
        job: String,
        /// The project folder.
        path: String,
        /// Frames done.
        frame: i64,
        /// Frames in total.
        total: i64,
        /// "video", "audio" or "mux".
        stage: String,
    },
    /// The export wrote its file. The job is over.
    #[serde(rename = "export.done")]
    ExportDone {
        /// The job.
        job: String,
        /// The project folder.
        path: String,
        /// The file written.
        output: String,
        /// Its picture size.
        width: u32,
        /// Its picture size.
        height: u32,
    },
    /// The export did not write its file. The job is over.
    #[serde(rename = "export.failed")]
    ExportFailed {
        /// The job.
        job: String,
        /// The project folder.
        path: String,
        /// Why: [`ErrorCode::Cancelled`] when asked to stop, otherwise what
        /// went wrong.
        error: ApiError,
    },
}

impl Event {
    /// [`Event::ExportProgress`] for one report from the renderer.
    pub fn progress(job: &str, path: &str, progress: Progress) -> Event {
        Event::ExportProgress {
            job: job.to_owned(),
            path: path.to_owned(),
            frame: progress.frame,
            total: progress.total,
            stage: progress.stage.to_owned(),
        }
    }

    /// The job this event is about.
    pub fn job(&self) -> &str {
        match self {
            Event::CutoutProgress { job, .. }
            | Event::StabilizeProgress { job, .. }
            | Event::ExportProgress { job, .. }
            | Event::ExportDone { job, .. }
            | Event::ExportFailed { job, .. } => job,
        }
    }
}
