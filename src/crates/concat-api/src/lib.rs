// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The Concat API: the one dispatcher every way of driving the editor
//! without its window goes through.
//!
//! The command line, a daemon on a socket, an MCP server, a plugin: each is
//! a transport, and a transport is a loop that reads a [`Request`], hands
//! it to [`Api::dispatch`], and writes the [`Response`] back, forwarding
//! the [`Event`]s the API's jobs raise as they come. Nothing about what a
//! request *means* lives in a transport, so two of them cannot disagree,
//! and a method added here reaches all of them. What a line transport puts
//! around the three is the JSON-RPC 2.0 envelope in [`rpc`].
//!
//! The crate decides nothing about the edit either. Edits are
//! `concat_project` [`Command`]s carried as they are; projects, media,
//! templates, titles, cutouts and exports are `concat_host`'s. What this
//! crate owns is the choreography the window performs by hand - probe then
//! add, find masks and paint titles before rendering, save through the
//! session - stated once so a file exported here is the file the window
//! would have written.
//!
//! One [`Api`] holds one session per open project folder. It is not
//! thread-safe by design: a transport that serves several callers owns the
//! one `Api` and serialises through it, the way the window's event loop
//! does. A method that would keep everyone waiting - an export - runs as a
//! job on its own thread instead: [`Api::dispatch`] returns its name at
//! once and the job reports through the [`EventSink`] the API was made
//! with, from that thread, which is why the sink must be `Send + Sync`.

mod analysis;
mod job;
pub mod message;
pub mod rpc;
#[cfg(feature = "speech")]
mod speech;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use base64::Engine as _;
use concat_effects::Catalogue;
use concat_effects::manifest::Kind;
use concat_export::{ExportClip, ExportRequest, engine_range};
pub use concat_host::AppDirs;
use concat_host::bgm::{self as bgm_library, Query as BgmQuery, Sort as BgmSort};
use concat_host::cutout::{self, AnalyseRequest, Cutouts};
use concat_host::export::{self, Exporter};
use concat_host::magick as sticker_library;
use concat_host::openvid::{self, Orientation, Query, Sort, Source};
use concat_host::preview::{FrameSpec, Monitor};
use concat_host::session::EditorView;
use concat_host::sounds;
use concat_host::stabilize::{StabilizeRequest, Stabilizers};
use concat_host::stock::{self as live_stock, Provider};
use concat_host::templates::{self, SlotFill};
use concat_host::{ProjectInfo, Session, Titles, lock, media, projects};
use concat_project::Command;
use concat_project::commands::TrimEdge;
use concat_project::model::{MediaKind, VideoSettings};

pub use message::{
    API_VERSION, ApiError, BgmFound, BgmImport, BgmLicense, BgmState, BgmTrack, Dirs, Done,
    ErrorCode, Event, ExportSpec, Fill, JobState, JobStatus, MagickAdd, MagickEntry, MagickFound,
    MagickState, OpenvidClip, OpenvidFound, OpenvidImport, OpenvidRerank, OpenvidState,
    PackageInfo, ParamInfo, Picture, Reply, Request, Response, SoundEntry, SoundsAdded,
    SoundsFound, Started, StockFetched, StockFound, StockHit, StockImported, VersionInfo,
    WindowSought, Written,
};

/// The export sheet's middle quality, and what an export gets when the
/// caller says nothing.
const DEFAULT_CRF: u8 = 20;
/// The x264 preset every export uses unless told otherwise.
const DEFAULT_PRESET: &str = "medium";

/// The window's playhead, when a window embeds this API.
///
/// The window installs one - see [`Api::share_playhead`] - so a caller
/// over a socket can scrub the timeline the window is showing: the
/// `window.seek` method hands the time here, and the control moves the
/// playhead on the window's event loop and answers with the project it
/// moved on. A control that cannot reach the window - it closed, its loop
/// is gone - answers with the honest error, so a caller never believes a
/// scrub that did not happen. Cheap to clone; every clone is the same
/// control.
#[derive(Clone)]
pub struct PlayheadControl(PlayheadSeek);

/// What [`PlayheadControl`] holds: the project folder the caller means,
/// if it named one, and a time in seconds, to the window's answer - or
/// the sentence that says why the playhead did not move.
type PlayheadSeek = Arc<dyn Fn(Option<&str>, f64) -> Result<WindowSought, String> + Send + Sync>;

impl PlayheadControl {
    /// Makes a control around `seek`: given the project folder the caller
    /// means, if it named one, and a time in seconds, move the playhead
    /// and answer with where it is, or the sentence that says why not.
    pub fn new(
        seek: impl Fn(Option<&str>, f64) -> Result<WindowSought, String> + Send + Sync + 'static,
    ) -> Self {
        PlayheadControl(Arc::new(seek))
    }

    /// Moves the playhead, as the window was asked to.
    pub fn seek(&self, path: Option<&str>, time: f64) -> Result<WindowSought, String> {
        (self.0)(path, time)
    }
}

/// What every build of the API serves, before what is decided by the
/// build or the embedder is added; see [`VersionInfo::capabilities`].
const CAPABILITIES: &[&str] = &["events"];

/// The JSON Schema of [`Request`]: one alternative per method, each the
/// object its params are with `method` naming it, and every type a request
/// carries - an edit [`Command`] and all it holds - under `$defs`. The
/// descriptions are this crate's and `concat_project`'s doc comments, so
/// what a caller is told about a field is what its maintainer wrote next
/// to it. Generated, never written by hand: a method or a command field
/// added to the types is in the schema on the next build.
#[cfg(feature = "schema")]
pub fn request_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Request)).unwrap_or_default()
}

/// Where a job's events go. Called from the job's thread, so a transport
/// that writes them to a caller locks its writer inside.
pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

/// The largest frame a preview or an export may ask for, a side: 8K, the
/// engine's own ceiling. A caller with the token is trusted to edit, not
/// to ask the machine for seventeen gigabytes of pixels (audit 2026-09-23,
/// #4).
pub const MAX_SIDE: u32 = concat_core::frame::MAX_SIDE;
/// The highest constant rate factor any codec here takes.
const MAX_CRF: u8 = 63;
/// The fastest frame rate an export may ask for.
const MAX_RATE: f64 = 240.0;

/// Who holds a project folder open on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// The window, on screen.
    Window,
    /// The API, for a caller on a socket.
    Api,
}

impl std::fmt::Display for Holder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Holder::Window => "the window",
            Holder::Api => "the API",
        })
    }
}

/// Why a project folder could not be claimed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The other side of this process has it: the window, or the API it
    /// serves.
    Here(Holder),
    /// Another process has it - a second window, `concat --mcp`, the CLI -
    /// as its lock file says.
    Elsewhere(lock::Owner),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Here(holder) => write!(f, "open in {holder}"),
            Refusal::Elsewhere(owner) => {
                let what = if owner.role == "window" {
                    "a Concat window"
                } else {
                    "another Concat process"
                };
                match owner.pid {
                    Some(pid) => write!(f, "open in {what} (process {pid})"),
                    None => write!(f, "open in {what}"),
                }
            }
        }
    }
}

/// One claimed folder: who has it in this process, and the lock that
/// keeps every other process off it.
struct Claim {
    holder: Holder,
    /// `None` where the folder would not take a lock file - one that
    /// cannot be written - which costs the guard, not the open.
    _lock: Option<lock::ProjectLock>,
}

/// The project folders open on this machine and who has each, shared
/// between a window and the API it embeds, so neither opens a folder the
/// other is editing and saves over its work (audit 2026-09-23, #5). Cheap
/// to clone; every clone is the same register.
///
/// Each claimed folder is also locked against every other process
/// ([`lock::ProjectLock`]): a second window, `concat --mcp` working on its
/// own, the CLI. The lock goes with the claim, and with the process if it
/// dies, so a crash leaves a folder the next opener takes over.
#[derive(Clone, Default)]
pub struct OpenProjects(Arc<Mutex<BTreeMap<String, Claim>>>);

impl OpenProjects {
    /// Claims `path` for `holder`. Refused, naming who has it, when the
    /// other side of this process does or another process does; claiming
    /// again what one already holds is fine.
    pub fn claim(&self, path: &str, holder: Holder) -> Result<(), Refusal> {
        let key = key_of(path);
        let mut open = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match open.get(&key) {
            Some(claim) if claim.holder != holder => Err(Refusal::Here(claim.holder)),
            Some(_) => Ok(()),
            None => {
                let role = match holder {
                    Holder::Window => "window",
                    Holder::Api => "api",
                };
                let lock = match lock::ProjectLock::acquire(Path::new(path), role) {
                    Ok(lock) => Some(lock),
                    Err(lock::LockError::Held(owner)) => return Err(Refusal::Elsewhere(owner)),
                    // A folder that will not take the file: a read-only
                    // one, which nobody can save over anyway.
                    Err(lock::LockError::Io(_)) => None,
                };
                open.insert(
                    key,
                    Claim {
                        holder,
                        _lock: lock,
                    },
                );
                Ok(())
            }
        }
    }

    /// Gives `path` back, whoever had it.
    pub fn release(&self, path: &str) {
        let key = key_of(path);
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key);
    }

    /// Who has `path` open, if anyone.
    pub fn holder(&self, path: &str) -> Option<Holder> {
        let key = key_of(path);
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .map(|claim| claim.holder)
    }
}

/// The dispatcher: the open sessions and the services behind them.
pub struct Api {
    dirs: AppDirs,
    /// Open projects by canonical folder path.
    sessions: BTreeMap<String, Session>,
    /// The answers [`Request::EditApply`] already gave, by the caller's
    /// own idempotency key and the project it named (IDEM): a re-sent
    /// key is answered without a second edit. Keyed by
    /// `"{path}\n{key}"` so two projects' keys never collide. The
    /// answers are bounded (F-05): every key keeps the hash of the view
    /// its edit produced, only the most recent keep the view itself,
    /// and the whole ring is capped - a long-running API's memory no
    /// longer grows with every keyed edit. The keys themselves ride in
    /// `<project>/.idempotency-ledger.json` (F-06), so a restart's
    /// re-sent keys land once too.
    applied: Idempotency,
    /// Where this API may write: a created project, an instantiated
    /// template, an export or a preview file lands under one of these.
    /// Empty means anywhere, for the process's own owner at a terminal.
    roots: Vec<PathBuf>,
    /// Which project folders are open on this machine, across this API
    /// and a window it is embedded in.
    open: OpenProjects,
    /// The window's playhead, when a window embeds this API. `None` - a
    /// headless API - makes `window.seek` refuse with the reason.
    playhead: Option<PlayheadControl>,
    titles: Titles,
    cutouts: Arc<Cutouts>,
    stabilizers: Arc<Stabilizers>,
    monitor: Monitor,
    exporter: Exporter,
    events: EventSink,
    jobs: Jobs,
    /// Every job's last known state and each running one's stop flag, fed
    /// by the events on their way to `events`.
    book: Arc<job::Book>,
    /// [`VersionInfo::capabilities`], in the order they were added.
    capabilities: Vec<String>,
    /// The ear this API shares with the window's captions, when this build
    /// speaks.
    #[cfg(feature = "speech")]
    transcriber: Arc<concat_speech::Transcriber>,
    /// The voice, when this build speaks.
    #[cfg(feature = "speech")]
    speech: Arc<concat_speech::Speech>,
}

/// A way to stop the jobs of one [`Api`] from any thread, without waiting
/// for its thread to be free: what a server that is shutting down calls
/// before it waits for that thread. Cheap to clone.
#[derive(Clone)]
pub struct Stopper {
    book: Arc<job::Book>,
    cutouts: Arc<Cutouts>,
}

/// How many of the most recent idempotency keys keep the recorded view
/// for the re-send answer (F-05). A resumed run re-asks the placements
/// it just made; a run from long before re-asks keys whose view the
/// ring no longer owes - the edit still lands once, answered with the
/// project as it stands.
const KEYED_VIEWS_KEPT: usize = 64;

/// How many idempotency keys are remembered at all, in memory and in the
/// project's ledger file (F-05, F-06). A key past the cap is forgotten
/// - its re-send applies again - which is the documented degradation a
///   bounded ledger owes, against the OOM an unbounded one promised.
const LEDGER_KEYS_KEPT: usize = 4096;

/// The durable ledger's name, inside the project it belongs to (F-06):
/// `<project>/.idempotency-ledger.json`, read when the project opens,
/// pruned when it is written.
const IDEMPOTENCY_LEDGER: &str = ".idempotency-ledger.json";

/// One answered idempotency key (F-05): the hash of the view the key's
/// edit produced - small, and all the durable ledger owes - and, for
/// the most recent keys, the recorded view itself, so a re-sent batch
/// is answered with the reply it got.
struct AppliedKey {
    /// The applied view's own hash, of its serialized whole.
    hash: u64,
    /// The recorded view, while the ring still keeps it: `None` for
    /// keys adopted from the durable ledger (a restart answered from
    /// the project's current state) and for keys past the view cap.
    view: Option<EditorView>,
}

/// The idempotency answers, bounded on both of its floors (F-05): a
/// hash per key forever-ish, a view per key for the recent window, and
/// a hard cap on the ring as a whole.
#[derive(Default)]
struct Idempotency {
    /// The answers, by `"{path}\n{key}"`.
    answers: std::collections::HashMap<String, AppliedKey>,
    /// The same keys in record order, oldest first: the eviction order.
    order: std::collections::VecDeque<String>,
}

impl Idempotency {
    /// The answer a key already has, if any.
    fn answer(&self, id: &str) -> Option<&AppliedKey> {
        self.answers.get(id)
    }

    /// Records a key's answer: the hash always, the view while the ring
    /// still keeps it.
    fn record(&mut self, id: String, hash: u64, view: EditorView) {
        if self.answers.remove(&id).is_some() {
            self.order.retain(|seen| seen != &id);
        }
        self.order.push_back(id.clone());
        self.answers.insert(
            id,
            AppliedKey {
                hash,
                view: Some(view),
            },
        );
        self.trim();
    }

    /// Adopts a key the durable ledger carried (F-06): the hash, and no
    /// view - a restarted API answers from the project's current state,
    /// which already carries the applied edit.
    fn adopt(&mut self, id: String, hash: u64) {
        if self.answers.contains_key(&id) {
            return;
        }
        self.order.push_back(id.clone());
        self.answers.insert(id, AppliedKey { hash, view: None });
        self.trim();
    }

    /// Keeps both floors: the ring no longer than [`LEDGER_KEYS_KEPT`],
    /// and a view only on the most recent [`KEYED_VIEWS_KEPT`] of them.
    fn trim(&mut self) {
        while self.order.len() > LEDGER_KEYS_KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.answers.remove(&oldest);
            }
        }
        let without_view = self.order.len().saturating_sub(KEYED_VIEWS_KEPT);
        for id in self.order.iter().take(without_view) {
            if let Some(answer) = self.answers.get_mut(id) {
                answer.view = None;
            }
        }
    }

    /// One project's rows for its ledger file, most recent first: the
    /// caller's own key and the hash of the view its edit produced.
    fn rows_for(&self, path: &str) -> Vec<(String, u64)> {
        let prefix = format!("{path}\n");
        self.order
            .iter()
            .rev()
            .filter_map(|id| {
                let key = id.strip_prefix(&prefix)?;
                let hash = self.answers.get(id)?.hash;
                Some((key.to_owned(), hash))
            })
            .take(LEDGER_KEYS_KEPT)
            .collect()
    }
}

impl Stopper {
    /// Asks every job this API started to stop at its next frame - its
    /// own and no other, even where the export slot is shared with a
    /// window. Each then ends with [`ErrorCode::Cancelled`]. How many
    /// were running.
    pub fn cancel_all(&self) -> usize {
        let running = self.book.cancel_all();
        if self.book.exports_running() {
            // A job still finding masks is in the analysis, not the
            // render; the analysis is this API's own - and only an
            // export of this API's is running, so the pool is this
            // API's to cancel.
            self.cutouts.cancel();
        }
        running
    }
}

impl Api {
    /// An API over this machine's app directories, reporting its jobs
    /// through `events`.
    pub fn new(events: EventSink) -> Result<Api, String> {
        Ok(Api::with_dirs(AppDirs::locate()?, events))
    }

    /// An API over the given directories: a test's scratch, or an embedder
    /// with a home of its own.
    pub fn with_dirs(dirs: AppDirs, events: EventSink) -> Api {
        let book = Arc::new(job::Book::default());
        let events: EventSink = {
            let book = Arc::clone(&book);
            Arc::new(move |event: Event| {
                book.record(&event);
                events(event);
            })
        };
        Api {
            book,
            applied: Idempotency::default(),
            titles: Titles::new(&dirs),
            cutouts: Arc::new(Cutouts::new(&dirs.data)),
            stabilizers: Arc::new(Stabilizers::new()),
            monitor: Monitor::new(),
            exporter: Exporter::new(),
            sessions: BTreeMap::new(),
            roots: Vec::new(),
            open: OpenProjects::default(),
            playhead: None,
            events,
            jobs: Jobs::default(),
            dirs,
            capabilities: CAPABILITIES.iter().map(|name| (*name).to_owned()).collect(),
            #[cfg(feature = "speech")]
            transcriber: Arc::new(concat_speech::Transcriber::new()),
            #[cfg(feature = "speech")]
            speech: Arc::new(concat_speech::Speech::new()),
        }
    }

    /// The directories this API works under.
    pub fn dirs(&self) -> &AppDirs {
        &self.dirs
    }

    /// Adds a name to [`VersionInfo::capabilities`]: what the transport
    /// or embedder around this API serves that the API cannot know of
    /// itself, such as the sockets it is listening on. A name already
    /// there is not repeated.
    pub fn add_capability(&mut self, name: &str) {
        if !self.capabilities.iter().any(|known| known == name) {
            self.capabilities.push(name.to_owned());
        }
    }

    /// What stops this API's jobs from another thread; see [`Stopper`].
    pub fn stopper(&self) -> Stopper {
        Stopper {
            book: Arc::clone(&self.book),
            cutouts: Arc::clone(&self.cutouts),
        }
    }

    /// [`Request::ExportStatus`].
    pub fn job_status(&mut self, job: &str) -> Result<JobStatus, ApiError> {
        self.jobs.reap(&self.book);
        self.book.status(job).ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("no job {job}: it never ran, or ended long ago"),
            )
        })
    }

    /// [`Request::ExportStatus`] by what the job was started on: the
    /// project folder for an export, the file for an analysis. The newest
    /// job for `path` answers - a running one first, else the last one
    /// that finished. For the caller that never held a name: an
    /// unattended run whose program was written before the job was
    /// minted.
    pub fn job_status_for_path(&mut self, path: &str) -> Result<JobStatus, ApiError> {
        self.jobs.reap(&self.book);
        self.book.status_for_path(path).ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("no job was started on {path}: it never ran, or ended long ago"),
            )
        })
    }

    /// The export slot, for an embedder that shares it with a window.
    pub fn exporter(&self) -> Exporter {
        self.exporter.clone()
    }

    /// Takes an embedder's export slot in place of its own, so "one export
    /// at a time" holds across a window and this API together.
    pub fn share_exporter(&mut self, exporter: Exporter) {
        self.exporter = exporter;
    }

    /// Takes an embedder's playhead in place of nothing: with it,
    /// `window.seek` moves the playhead of the timeline the window is
    /// showing, which is how an agent scrubs while it edits.
    pub fn share_playhead(&mut self, playhead: PlayheadControl) {
        self.playhead = Some(playhead);
        self.add_capability("window.seek");
    }

    /// Takes an embedder's register of open projects in place of its own,
    /// so a folder the window has open is refused here, and the other way
    /// round.
    pub fn share_open_projects(&mut self, open: OpenProjects) {
        self.open = open;
    }

    /// Confines every write to `roots`: from here on a created project, an
    /// instantiated template, an export and a preview file must land under
    /// one of them, and any other path is `refused`. Reads - a probe, a
    /// project opened by path - are not confined. An empty list confines
    /// nothing, which is right for the process's own owner at a terminal
    /// and wrong for a socket.
    pub fn restrict_writes_to(&mut self, roots: Vec<PathBuf>) {
        self.roots = roots
            .into_iter()
            .map(|root| root.canonicalize().unwrap_or(root))
            .collect();
    }

    /// The roots writes are confined to; empty when they are not.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// `path` as somewhere this API may write, or why not.
    fn writable(&self, path: &str) -> Result<(), ApiError> {
        if self.roots.is_empty() {
            return Ok(());
        }
        let outside = || {
            let roots = self
                .roots
                .iter()
                .map(|root| root.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            ApiError::new(
                ErrorCode::Refused,
                format!("{path} is outside where this API writes ({roots})"),
            )
        };
        let given = Path::new(path);
        // A `..` past the part of the path that exists cannot be resolved
        // and would walk out of a root on paper while staying under it in
        // this check; there is no reason a caller needs one.
        if given
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(outside());
        }
        let target = resolved(given);
        if self.roots.iter().any(|root| target.starts_with(root)) {
            Ok(())
        } else {
            Err(outside())
        }
    }

    /// How many jobs are still running.
    pub fn running(&mut self) -> usize {
        self.jobs.reap(&self.book);
        self.jobs.threads.len()
    }

    /// Waits for every running job to end. A transport calls this before
    /// it exits, so an export begun on its last line still finishes.
    pub fn finish(&mut self) {
        self.jobs.finish();
    }

    /// Runs one request. The response is what the request is worth; a job
    /// it begins reports through the sink from then on.
    pub fn dispatch(&mut self, request: Request) -> Response {
        self.jobs.reap(&self.book);
        self.run(request).into()
    }

    fn run(&mut self, request: Request) -> Result<Reply, ApiError> {
        let view = |view: EditorView| Ok(Reply::View(Box::new(view)));
        match request {
            Request::Version => Ok(Reply::Version(self.version())),
            Request::ProjectCreate {
                location,
                name,
                video,
            } => view(self.create(&location, &name, video.unwrap_or_default())?),
            Request::ProjectOpen { path } => view(self.open(&path)?),
            Request::ProjectClose { path, save } => {
                self.close(&path, save)?;
                Ok(Reply::Done(Done {}))
            }
            Request::ProjectList => Ok(Reply::Projects(self.recents())),
            Request::ProjectGet { path } => view(self.session(&path)?.view()),
            Request::ProjectDocument { path } => Ok(Reply::Document(
                self.session(&path)?.document().map_err(ApiError::failed)?,
            )),
            Request::ProjectSave { path, name } => {
                self.save(&path, name.as_deref())?;
                Ok(Reply::Done(Done {}))
            }
            Request::ProjectSetVideo { path, video } => {
                let set = self.session_mut(&path)?.set_video(video).map_err(refused)?;
                view(self.told_unsaved(&path, set))
            }
            Request::EditApply {
                path,
                command,
                idempotency_key,
            } => view(self.apply_keyed(&path, *command, idempotency_key)?),
            Request::EditUndo { path } => {
                let undone = self.session_mut(&path)?.undo();
                view(self.told_unsaved(&path, undone))
            }
            Request::EditRedo { path } => {
                let redone = self.session_mut(&path)?.redo();
                view(self.told_unsaved(&path, redone))
            }
            Request::MediaProbe { path } => {
                Ok(Reply::Media(media::probe(&path).map_err(ApiError::failed)?))
            }
            Request::OpenvidStatus => Ok(Reply::Openvid(self.openvid_state())),
            Request::OpenvidPrepare => {
                self.openvid_prepare();
                Ok(Reply::Openvid(self.openvid_state()))
            }
            Request::OpenvidSearch {
                query,
                min_aesthetic,
                min_motion,
                max_motion,
                min_temporal,
                min_seconds,
                max_seconds,
                min_fps,
                camera,
                source,
                orientation,
                sort,
                limit,
                offset,
                exclude_ids,
            } => Ok(Reply::OpenvidFound(self.openvid_search(
                query,
                min_aesthetic,
                min_motion,
                max_motion,
                min_temporal,
                min_seconds,
                max_seconds,
                min_fps,
                camera,
                source,
                orientation,
                sort,
                limit,
                offset,
                exclude_ids,
            )?)),
            Request::OpenvidGet { ids } => Ok(Reply::OpenvidClips(self.openvid_get(&ids)?)),
            Request::OpenvidFetch { id } => Ok(Reply::OpenvidClips(self.openvid_fetch(id)?)),
            Request::OpenvidImport {
                path,
                ids,
                start,
                track,
                monetized,
            } => Ok(Reply::OpenvidImported(
                self.openvid_import(&path, &ids, start, track, monetized)?,
            )),
            Request::StockSearch {
                provider,
                query,
                orientation,
                limit,
                page,
                api_key,
            } => Ok(Reply::StockFound(self.stock_search(
                &provider,
                query.as_deref(),
                orientation.as_deref(),
                limit.unwrap_or(20),
                page.unwrap_or(1),
                api_key.as_deref(),
            )?)),
            Request::StockFetch { provider, id, url } => {
                Ok(Reply::StockFetched(self.stock_fetch(&provider, &id, &url)?))
            }
            Request::StockImport {
                path,
                provider,
                id,
                url,
                author,
                start,
                track,
            } => Ok(Reply::StockImported(self.stock_import(
                &path,
                &provider,
                &id,
                &url,
                author.as_deref(),
                start,
                track,
            )?)),
            Request::SoundsSearch {
                query,
                category,
                limit,
                offset,
            } => Ok(Reply::SoundsFound(self.sounds_search(
                query.as_deref(),
                category.as_deref(),
                limit,
                offset,
            )?)),
            Request::SoundsAdd {
                path,
                id,
                start,
                track,
                file,
            } => Ok(Reply::SoundsAdded(self.sounds_add(
                &path,
                id.as_deref(),
                start,
                track,
                file.as_deref(),
            )?)),
            Request::BgmStatus => Ok(Reply::Bgm(self.bgm_state())),
            Request::BgmPrepare { source } => {
                self.bgm_prepare(source.as_deref())?;
                Ok(Reply::Bgm(self.bgm_state()))
            }
            Request::BgmSearch {
                query,
                source,
                category,
                min_seconds,
                max_seconds,
                sort,
                limit,
                offset,
            } => Ok(Reply::BgmFound(self.bgm_search(
                query,
                source,
                category,
                min_seconds,
                max_seconds,
                sort,
                limit,
                offset,
            )?)),
            Request::BgmFetch { id } => Ok(Reply::BgmTrack(self.bgm_fetch(&id)?)),
            Request::BgmImport {
                path,
                ids,
                start,
                track,
                monetized,
                file,
            } => Ok(Reply::BgmImported(self.bgm_import(
                &path,
                &ids,
                start,
                track,
                monetized,
                file.as_deref(),
            )?)),
            Request::MagickStatus => Ok(Reply::Magick(self.magick_state())),
            Request::MagickPrepare => {
                self.magick_prepare();
                Ok(Reply::Magick(self.magick_state()))
            }
            Request::MagickSearch {
                query,
                source,
                favourites,
                limit,
                offset,
            } => Ok(Reply::MagickFound(self.magick_search(
                query.as_deref(),
                source.as_deref(),
                favourites.unwrap_or(false),
                limit,
                offset,
            )?)),
            Request::MagickGet { ids } => Ok(Reply::MagickStickers(self.magick_get(&ids)?)),
            Request::MagickFetch { id } => Ok(Reply::MagickStickers(self.magick_fetch(&id)?)),
            Request::MagickAdd {
                path,
                id,
                start,
                duration,
                track,
                scale,
                offset_x,
                offset_y,
                rotation,
            } => Ok(Reply::MagickAdded(self.magick_add(
                &path,
                &id,
                Placement {
                    start,
                    duration,
                    track,
                    scale,
                    offset_x,
                    offset_y,
                    rotation,
                },
            )?)),
            Request::MediaImport { path, file } => view(self.import(&path, &file)?),
            Request::CatalogueList { kind } => Ok(Reply::Packages(catalogue(kind.as_deref())?)),
            Request::TemplateList => Ok(Reply::Templates(templates::list(&self.dirs.config))),
            Request::TemplateInstantiate {
                template,
                location,
                name,
                fills,
            } => view(self.instantiate(&template, &location, &name, fills)?),
            Request::TemplateSave { path, name } => {
                Ok(Reply::Template(self.save_template(&path, &name)?))
            }
            Request::ExportRun { path, spec } => Ok(Reply::Started(self.export(&path, &spec)?)),
            Request::ExportCancel { job } => {
                self.cancel(&job)?;
                Ok(Reply::Done(Done {}))
            }
            Request::ExportStatus { job, path } => {
                let status = match (&job, &path) {
                    (Some(job), _) => self.job_status(job)?,
                    (None, Some(path)) => self.job_status_for_path(path)?,
                    (None, None) => {
                        return Err(ApiError::invalid(
                            "export.status names a job or a path: one of the two",
                        ));
                    }
                };
                Ok(Reply::Job(status))
            }
            Request::PreviewFrame {
                path,
                time,
                output,
                width,
                height,
            } => {
                let size = width.zip(height);
                match output {
                    Some(output) => Ok(Reply::Written(self.frame(&path, time, &output, size)?)),
                    None => Ok(Reply::Picture(self.picture(&path, time, size)?)),
                }
            }
            Request::AnalysisScenes {
                path,
                threshold,
                detector,
                min_scene_len,
            } => Ok(Reply::Started(self.analysis_scenes(
                &path,
                threshold,
                detector,
                min_scene_len,
            )?)),
            Request::AnalysisBeats { path } => Ok(Reply::Started(self.analysis_beats(&path)?)),
            Request::AnalysisSilence {
                path,
                threshold_db,
                min_silence_ms,
            } => Ok(Reply::Started(self.analysis_silence(
                &path,
                threshold_db,
                min_silence_ms,
            )?)),
            #[cfg(feature = "speech")]
            Request::SpeechTranscribe {
                path,
                model,
                language,
                initial_prompt,
            } => Ok(Reply::Started(self.speech_transcribe(
                &path,
                model,
                language,
                initial_prompt,
            )?)),
            #[cfg(not(feature = "speech"))]
            Request::SpeechTranscribe { .. } => Err(ApiError::new(
                ErrorCode::Refused,
                "this build does not speak: it was built without the speech feature",
            )),
            #[cfg(feature = "speech")]
            Request::SpeechGenerate {
                project,
                text,
                voice,
                speed,
                model,
                delivery,
                pauses,
                steps,
            } => Ok(Reply::Started(self.speech_generate(
                &project,
                &text,
                voice,
                speed,
                model,
                delivery.as_deref(),
                pauses,
                steps,
            )?)),
            #[cfg(not(feature = "speech"))]
            Request::SpeechGenerate { .. } => Err(ApiError::new(
                ErrorCode::Refused,
                "this build does not speak: it was built without the speech feature",
            )),
            #[cfg(feature = "speech")]
            Request::ModelsList => Ok(Reply::Models(self.models_list()?)),
            #[cfg(not(feature = "speech"))]
            Request::ModelsList => Err(ApiError::new(
                ErrorCode::Refused,
                "this build does not speak: it was built without the speech feature",
            )),
            #[cfg(feature = "speech")]
            Request::ModelsDownload { kind, id } => {
                Ok(Reply::Started(self.models_download(&kind, &id)?))
            }
            #[cfg(not(feature = "speech"))]
            Request::ModelsDownload { .. } => Err(ApiError::new(
                ErrorCode::Refused,
                "this build does not speak: it was built without the speech feature",
            )),
            Request::WindowSeek { time, path } => {
                let control = self.playhead.as_ref().ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::Failed,
                        "no window is serving this API, so there is no playhead to move \
                         - start the editor, turn Settings > Automation on, and connect \
                         with --mcp window"
                            .to_owned(),
                    )
                })?;
                if !time.is_finite() {
                    return Err(ApiError::new(
                        ErrorCode::Failed,
                        "time must be a finite number of seconds".to_owned(),
                    ));
                }
                let sought = control
                    .seek(path.as_deref(), time.max(0.0))
                    .map_err(ApiError::failed)?;
                Ok(Reply::WindowSeek(sought))
            }
        }
    }

    /// [`Request::Version`].
    pub fn version(&self) -> VersionInfo {
        VersionInfo {
            api_version: API_VERSION.to_owned(),
            // The running build's own number, not the workspace's bare
            // one: a release built from one of the fork's tags -
            // v0.2.8-funcut.12 - carries its iteration through
            // `FUNCUT_ITERATION`, and a `concat_version` that answered
            // `0.2.8` would name upstream's release, not this fork's -
            // an agent could not tell the two installs apart.
            concat: concat_host::updates::Version::current().to_string(),
            dirs: Dirs::from(&self.dirs),
            capabilities: {
                let mut capabilities = self.capabilities.clone();
                if self.monitor.has_gpu() {
                    capabilities.push("gpu".to_owned());
                }
                capabilities
            },
        }
    }

    /// [`Request::ProjectCreate`]: the folder, its manifest, and a session
    /// on it, remembered in recents like a project the window made.
    pub fn create(
        &mut self,
        location: &str,
        name: &str,
        video: VideoSettings,
    ) -> Result<EditorView, ApiError> {
        self.writable(location)?;
        let info = projects::create(
            location,
            name,
            video.width,
            video.height,
            video.rate_num,
            video.rate_den,
        )
        .map_err(ApiError::failed)?;
        self.adopt(info)
    }

    /// [`Request::ProjectOpen`].
    pub fn open(&mut self, path: &str) -> Result<EditorView, ApiError> {
        let key = key_of(path);
        if let Some(session) = self.sessions.get(&key) {
            return Ok(session.view());
        }
        let info = projects::open(path).map_err(ApiError::failed)?;
        self.adopt(info)
    }

    /// Opens a session on a project the host just described and puts it at
    /// the front of the recents list.
    fn adopt(&mut self, info: ProjectInfo) -> Result<EditorView, ApiError> {
        if let Err(refusal) = self.open.claim(&info.path, Holder::Api) {
            // A window holding it is the case an agent meets: say how to
            // go through that window instead of around it.
            let how = match &refusal {
                Refusal::Elsewhere(owner) if owner.role == "window" => {
                    ". To work on it from an agent, turn on Settings > Automation in that window and run the agent with `concat --mcp window`; the project on screen stays the window's until live mode lands"
                }
                Refusal::Elsewhere(_) => ". Close it there first",
                Refusal::Here(_) => "",
            };
            return Err(ApiError::new(
                ErrorCode::Refused,
                format!("{} is {refusal}{how}", info.path),
            ));
        }
        let session = match Session::open_info(&info) {
            Ok(session) => session,
            Err(error) => {
                self.open.release(&info.path);
                return Err(ApiError::failed(error));
            }
        };
        // Recents are a convenience for the launch screen; a machine whose
        // config folder cannot be written still edits.
        let _ = projects::remember(&self.dirs.config, &info);
        let view = session.view();
        self.sessions.insert(key_of(&info.path), session);
        // The project's durable idempotency answers, adopted on open
        // (F-06): a key a prior API answered, re-sent after the restart,
        // lands once here too.
        self.load_idempotency_ledger(&info.path);
        // The project's media is seen to at once, the way a window's
        // open sees to it: the copies already written are adopted into
        // the pool, the videos as large as HD without one get theirs
        // queued, and the copies of media the project no longer holds
        // go.
        self.ensure_proxies(&info.path);
        self.sweep_stale_proxies(&info.path);
        Ok(view)
    }

    /// [`Request::ProjectClose`].
    pub fn close(&mut self, path: &str, save: bool) -> Result<(), ApiError> {
        if save {
            self.save(path, None)?;
        }
        let closed = self
            .sessions
            .remove(&key_of(path))
            .map(drop)
            .ok_or_else(|| not_open(path));
        if closed.is_ok() {
            self.open.release(path);
        }
        closed
    }

    /// [`Request::ProjectList`].
    pub fn recents(&self) -> Vec<ProjectInfo> {
        projects::list(&self.dirs.config)
    }

    /// [`Request::ProjectSave`].
    pub fn save(&mut self, path: &str, name: Option<&str>) -> Result<(), ApiError> {
        self.session_mut(path)?.save(name).map_err(ApiError::failed)
    }

    /// [`Request::EditApply`].
    pub fn apply(&mut self, path: &str, command: Command) -> Result<EditorView, ApiError> {
        let applied = self.session_mut(path)?.apply(command).map_err(refused)?;
        // F-07: the reply never says a success the disk did not see. The
        // autosave still cannot un-apply an edit that already happened,
        // so the call stays Ok - but its failure rides the view, and a
        // caller that ignores it is ignoring it on the record, not
        // discovering it three edits later as data loss.
        let view = self.told_unsaved(path, applied);
        // The edit may have brought media in - an import, an overlay, a
        // template's fills - and media is the one thing whose working
        // copy is worth having before anything reads it.
        self.ensure_proxies(path);
        Ok(view)
    }

    /// The view with the autosave's own failure marked on it, when there
    /// was one (F-07): the dispatch paths that save as a side effect of
    /// another call - `set_video`, `undo`, `redo`, `apply` - all answer
    /// the same way, never reporting a success the disk did not see.
    fn told_unsaved(&mut self, path: &str, mut view: EditorView) -> EditorView {
        if let Err(error) = self.autosave(path) {
            log::warn!("could not autosave {path}: {error}");
            view.unsaved = Some(error);
        }
        view
    }

    /// [`Request::EditApply`] with the caller's idempotency key (IDEM):
    /// a key this project already answered is answered again without a
    /// second edit - the same batch, asked twice, lands once. A key
    /// never seen (or absent) applies and is remembered under the key,
    /// so the reply a caller re-asks for is the reply it got. The
    /// caller's own key names the batch; two different batches under one
    /// key is a caller bug the API answers by design - the first batch's
    /// landing, not a merge. The answers are bounded (F-05) and durable
    /// (F-06): the most recent keys answer with their recorded view,
    /// older ones - a restart's, a long run's - with the project as it
    /// stands, which already carries the applied edit.
    pub fn apply_keyed(
        &mut self,
        path: &str,
        command: Command,
        key: Option<String>,
    ) -> Result<EditorView, ApiError> {
        let Some(key) = key else {
            return self.apply(path, command);
        };
        if key.is_empty() {
            return self.apply(path, command);
        }
        let id = format!("{path}\n{key}");
        if let Some(answered) = self.applied.answer(&id) {
            if let Some(view) = &answered.view {
                return Ok(view.clone());
            }
            // The ring no longer holds this key's view - a restart's
            // ledger key, or one pushed past the recent window (F-05 /
            // F-06): the edit stands, it applied once, and the honest
            // answer is the project as it is now - never a second
            // application of the batch.
            return Ok(self.session_mut(path)?.view());
        }
        let view = self.apply(path, command)?;
        let hash = view_hash(&view);
        self.applied.record(id, hash, view.clone());
        self.write_idempotency_ledger(path);
        Ok(view)
    }

    /// The window autosaves; a caller through the API had to remember
    /// `project.save`, and a crash - or an agent that never called it -
    /// orphaned every edit and imported file since the last one: the
    /// media sat in the folder with no document naming it. A save after
    /// every applied batch is cheap (the writer is ticketed and skips
    /// superseded snapshots) and is the honest state for a reopen.
    /// The edit already happened, so a save that cannot write cannot
    /// fail the call - but it is no longer silent either (F-07): the
    /// failure travels back to `apply`, which puts it on the view the
    /// caller holds.
    fn autosave(&mut self, path: &str) -> Result<(), String> {
        let session = self.session_mut(path).map_err(|error| error.to_string())?;
        session.save(None)
    }

    /// Writes the project's durable idempotency ledger (F-06):
    /// `<project>/.idempotency-ledger.json`, this project's keys with
    /// the hashes of the views their edits produced, the most recent
    /// first and pruned to the ring's own cap. Best-effort: a ledger
    /// that cannot be written costs the restart its re-send answer, not
    /// the edit its landing.
    fn write_idempotency_ledger(&self, path: &str) {
        let rows = self.applied.rows_for(path);
        let ledger = serde_json::json!({
            "schema": "funcut.idempotency-ledger/1",
            "schemaVersion": 1,
            "entries": rows.iter().map(|(key, hash)| serde_json::json!({
                "key": key, "hash": hash,
            })).collect::<Vec<_>>(),
        });
        let file = std::path::Path::new(path).join(IDEMPOTENCY_LEDGER);
        let staged = file.with_extension("json.saving");
        let write = || -> Result<(), String> {
            let bytes = serde_json::to_vec_pretty(&ledger).map_err(|error| error.to_string())?;
            std::fs::write(&staged, bytes).map_err(|error| error.to_string())?;
            std::fs::rename(&staged, &file).map_err(|error| error.to_string())
        };
        if let Err(error) = write() {
            log::warn!("could not write {}: {error}", file.display());
            let _ = std::fs::remove_file(&staged);
        }
    }

    /// Reads the project's durable idempotency ledger (F-06), adopting
    /// its keys: a restart's re-sent keys land once. A ledger that
    /// cannot be read or parsed is a fresh start, never a refusal to
    /// open the project.
    fn load_idempotency_ledger(&mut self, path: &str) {
        let file = std::path::Path::new(path).join(IDEMPOTENCY_LEDGER);
        let Ok(text) = std::fs::read_to_string(&file) else {
            return;
        };
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);
        let entries = match parsed {
            Ok(ledger) => ledger
                .get("entries")
                .and_then(|entries| entries.as_array())
                .cloned(),
            Err(error) => {
                log::warn!("could not read {}: {error}", file.display());
                None
            }
        };
        let Some(entries) = entries else {
            return;
        };
        // Adopted oldest first, the ring's own order: `rows_for` writes
        // the ledger newest first, so adopting the file's own order
        // would fill the ring backwards - and the first eviction after a
        // restart would drop the newest keys, exactly the ones a resumed
        // run re-sends, while the oldest ones stayed answered.
        for entry in entries.iter().rev() {
            let Some(key) = entry.get("key").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let hash = entry
                .get("hash")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            self.applied.adopt(format!("{path}\n{key}"), hash);
        }
    }

    /// [`Request::MediaImport`]: the probe and the add, as one.
    pub fn import(&mut self, path: &str, file: &str) -> Result<EditorView, ApiError> {
        let item = media::probe(file).map_err(ApiError::failed)?.to_new_media();
        self.apply(path, Command::AddMedia { item })
    }

    /// Sees to it that every video in an open project as large as HD
    /// has a proxy, and that the ones already written are what the
    /// reader pool reads: the same `concat_host::proxy::ensure` the
    /// window's art pass asks, for the editor without a window. An
    /// agent's import gets its working copy the moment it lands -
    /// written once, on the scheduler's proxy lane, while the edit goes
    /// on - and the frames small enough for a copy to serve whole come
    /// back from it; see [`Api::pixels`]. The codec is the default,
    /// MJPEG - every frame a keyframe, the scrubbiest there is (F-14:
    /// this comment said H.264, the default before the codec was a
    /// choice): the choice in Settings is the window's, and a caller
    /// here has none. Asking is idempotent - a file that has its copy,
    /// or is writing one, is a stat away - so `apply` may ask after
    /// every command, the way the window's art pass asks after every
    /// message.
    ///
    /// RES-03, the startup ordering: while the OpenVid index warms -
    /// this server's own start warmed it on its own thread - the host's
    /// startup I/O gate parks these asks instead of starting them, so
    /// an agent's first `project.open` never competes with the warm's
    /// 435 MB for the disk; the parked copies are submitted the moment
    /// the warm finishes, in arrival order, and a proxy already encoding
    /// when a warm begins is never interrupted. Nothing here asks,
    /// waits or knows: `proxy::ensure` answers the gate on its own.
    fn ensure_proxies(&self, path: &str) {
        let Ok(session) = self.session(path) else {
            return;
        };
        let project = Path::new(session.path());
        for item in session.project().media.iter() {
            if item.kind != MediaKind::Video || item.placeholder {
                continue;
            }
            let (Some(width), Some(height)) = (item.width, item.height) else {
                continue;
            };
            concat_host::proxy::ensure(
                project,
                &item.path,
                width,
                height,
                item.color_range.map(engine_range),
                item.color_space.is_hdr(),
                concat_host::proxy::ProxyCodec::default(),
            );
        }
    }

    /// Removes the proxies under an open project of media it no longer
    /// holds, or holds read as a range it no longer is: the same sweep a
    /// window makes when a project opens, for the editor without one -
    /// nothing else sweeps this folder. A copy being written is left for
    /// its writer.
    fn sweep_stale_proxies(&self, path: &str) {
        let Ok(session) = self.session(path) else {
            return;
        };
        let project = Path::new(session.path());
        let keep: HashSet<PathBuf> = session
            .project()
            .media
            .iter()
            .filter_map(|item| {
                concat_host::proxy::path_for(
                    project,
                    &item.path,
                    item.color_range.map(engine_range),
                    item.color_space.is_hdr(),
                    concat_host::proxy::ProxyCodec::default(),
                )
            })
            .collect();
        concat_host::proxy::sweep(project, &keep);
    }

    // ── the OpenVid stock library ─────────────────────────────────────────

    /// The library this API reads, from the app's own directories. One
    /// per process, so the window's pane and this server share a download
    /// and an index connection rather than racing each other.
    fn library(&self) -> &'static openvid::Library {
        openvid::shared(&self.dirs)
    }
    /// [`Request::OpenvidStatus`].
    pub fn openvid_state(&self) -> OpenvidState {
        let library = self.library();
        let (state, percent, error) = match library.status() {
            openvid::Status::Missing => ("missing", 0, None),
            openvid::Status::Downloading(percent) => ("downloading", percent, None),
            openvid::Status::Ready => ("ready", 100, None),
            openvid::Status::Failed(error) => ("failed", 0, Some(error)),
        };
        OpenvidState {
            state: state.to_owned(),
            percent,
            rows: library.rows(),
            measured: library.measured(),
            rerank: rerank_state(&self.dirs),
            error,
        }
    }

    /// [`Request::OpenvidPrepare`]: the one-time index download, started
    /// on its own thread because it is 435 MB and a caller should not
    /// wait for it. Its progress is what `openvid.status` reports; a
    /// second call while one runs is a no-op, and the window's own page
    /// starting one is the same download.
    pub fn openvid_prepare(&self) {
        let library = self.library();
        if matches!(
            library.status(),
            openvid::Status::Ready | openvid::Status::Downloading(_)
        ) {
            return;
        }
        std::thread::spawn(move || {
            // This flag is never set: the download runs to its end. A
            // cancel is the process going away, which takes the thread
            // with it; the partial file is resumed by the next start.
            let cancel = std::sync::atomic::AtomicBool::new(false);
            // The outcome lands in the status the next caller reads:
            // `Failed`, with these words, rather than a log line this
            // crate has no facade for.
            let _ = library.prepare(&cancel);
        });
    }

    /// The cold-load's own cure, on a background thread: open the OpenVid
    /// index now, so the first search does not pay for it. A
    /// session-serving caller - the headless MCP server, at its start -
    /// asks for this once; the open is a 435 MB SQLite connection, its
    /// pragmas and its caption dictionary, and paying it inside an
    /// agent's first `openvid.search` is seconds that read as a hang.
    /// Every outcome is discarded, on purpose: a missing index stays
    /// missing until `openvid.prepare` downloads it (this warms, it never
    /// downloads), a broken one stays broken for `openvid.status` to
    /// say, and a race with the first real asker is settled by the
    /// library's own `OnceLock` - one connection wins, the loser is
    /// dropped - so the thread is best-effort the whole way down.
    ///
    /// RES-03, the startup ordering: this warm and the proxy generation
    /// an import starts are serialized by the host's startup I/O gate -
    /// the warm holds it for its whole duration, and `proxy::ensure`
    /// parks behind it - so the two never compete for the disk. The
    /// window's OpenVid page and its download hold the same gate, so the
    /// ordering holds wherever the warm starts from.
    pub fn warm_openvid(&self) {
        let library = self.library();
        std::thread::spawn(move || {
            let _ = library.status();
        });
    }

    /// [`Request::OpenvidSearch`]: the filters as the request carries
    /// them, parsed once, with the words a caller chose kept as words.
    #[allow(clippy::too_many_arguments)]
    pub fn openvid_search(
        &self,
        query: Option<String>,
        min_aesthetic: Option<f64>,
        min_motion: Option<f64>,
        max_motion: Option<f64>,
        min_temporal: Option<f64>,
        min_seconds: Option<f64>,
        max_seconds: Option<f64>,
        min_fps: Option<f64>,
        camera: Option<String>,
        source: Option<String>,
        orientation: Option<String>,
        sort: Option<String>,
        limit: Option<u32>,
        offset: Option<u32>,
        exclude_ids: Vec<i64>,
    ) -> Result<OpenvidFound, ApiError> {
        self.openvid_ready()?;
        let source = source
            .map(|name| match name.to_ascii_lowercase().as_str() {
                "pixabay" => Ok(Source::Pixabay),
                "pexels" => Ok(Source::Pexels),
                "celebv" => Ok(Source::CelebV),
                "other" => Ok(Source::Other),
                other => Err(ApiError::failed(format!(
                    "the source must be pixabay, pexels, celebv or other, not {other:?}"
                ))),
            })
            .transpose()?;
        let orientation = orientation
            .map(|name| match name.to_ascii_lowercase().as_str() {
                "vertical" => Ok(Orientation::Vertical),
                "horizontal" => Ok(Orientation::Horizontal),
                "square" => Ok(Orientation::Square),
                other => Err(ApiError::failed(format!(
                    "the orientation must be vertical, horizontal or square, not {other:?}"
                ))),
            })
            .transpose()?;
        let sort = match sort.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("relevance") => Sort::Relevance,
            Some("aesthetic") => Sort::Aesthetic,
            Some("motion") => Sort::Motion,
            Some("temporal") => Sort::Temporal,
            Some("shortest") => Sort::Shortest,
            Some("longest") => Sort::Longest,
            Some("surprise") => Sort::Surprise,
            Some(other) => {
                return Err(ApiError::failed(format!(
                    "the sort must be relevance, aesthetic, motion, temporal, shortest, \
                     longest or surprise, not {other:?}"
                )));
            }
        };
        let search = Query {
            text: query,
            min_aesthetic,
            min_motion,
            max_motion,
            min_temporal,
            min_seconds,
            max_seconds,
            min_fps,
            camera,
            source,
            orientation,
            sort,
            limit: limit.unwrap_or(20).clamp(1, 100),
            offset: offset.unwrap_or(0),
            exclude_ids,
        };
        let found = self.library().search(&search).map_err(ApiError::failed)?;
        Ok(OpenvidFound {
            clips: found
                .clips
                .iter()
                .map(|clip| OpenvidClip::of(clip, clip.dims, None))
                .collect(),
            total: found.total,
        })
    }

    /// [`Request::OpenvidGet`].
    pub fn openvid_get(&self, ids: &[i64]) -> Result<Vec<OpenvidClip>, ApiError> {
        if ids.len() > 20 {
            return Err(ApiError::failed(
                "at most 20 clips a call; page through them".to_owned(),
            ));
        }
        self.openvid_ready()?;
        let library = self.library();
        let mut clips = Vec::new();
        for id in ids {
            let Some(clip) = library.clip(*id).map_err(ApiError::failed)? else {
                return Err(ApiError::failed(format!("there is no clip {id}")));
            };
            // The frame size is measured here, on first ask: one small
            // network read a clip, remembered after.
            let dims = library.dims(*id).map_err(ApiError::failed)?;
            clips.push(OpenvidClip::of(&clip, dims, None));
        }
        Ok(clips)
    }

    /// Whether a fetched clip's head reads like an MP4 (DEG-08): the
    /// download refuses a short body already, but a file whose head is not
    /// a box structure - a proxy's error page padded to size, a cache
    /// entry a crash truncated at the front - is not a clip however many
    /// bytes long it is. The check is the container's own signature, read
    /// in one small syscall: the dataset's clips are faststart MP4s and
    /// carry their `ftyp` in the first bytes. A digest would be stronger,
    /// but the index rows carry no per-clip hash to check against - name,
    /// offset and size are all they say - so the honest validation is
    /// size (already refused short) plus the container's shape, here.
    fn mp4_head_ok(path: &Path) -> bool {
        use std::io::Read;
        let Ok(mut file) = std::fs::File::open(path) else {
            return false;
        };
        let mut head = [0_u8; 512];
        let Ok(read) = file.read(&mut head) else {
            return false;
        };
        head[..read].windows(4).any(|window| window == b"ftyp")
    }

    /// A fetched clip, proven before use (DEG-08): fetch, then the head
    /// check, and a file that fails it leaves the cache so the next ask
    /// re-downloads instead of serving the corruption forever.
    fn fetched_clip_checked(library: &openvid::Library, id: i64) -> Result<PathBuf, ApiError> {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let path = library.fetch(id, &cancel).map_err(ApiError::failed)?;
        if !Self::mp4_head_ok(&path) {
            let _ = std::fs::remove_file(&path);
            return Err(ApiError::failed(format!(
                "clip {id} arrived but does not read as MP4 media; the bad copy was dropped \
                 from the cache - ask again to re-download it"
            )));
        }
        Ok(path)
    }

    /// [`Request::OpenvidFetch`].
    pub fn openvid_fetch(&self, id: i64) -> Result<Vec<OpenvidClip>, ApiError> {
        self.openvid_ready()?;
        let library = self.library();
        let path = Self::fetched_clip_checked(library, id)?;
        let Some(clip) = library.clip(id).map_err(ApiError::failed)? else {
            return Err(ApiError::failed(format!("there is no clip {id}")));
        };
        Ok(vec![OpenvidClip::of(&clip, clip.dims, Some(&path))])
    }

    /// [`Request::OpenvidImport`]: fetch, copy into the project's own
    /// folder, and add to the bin - one undoable step, or two when the
    /// clips are also laid on a lane.
    ///
    /// RISK-01: a monetised edit cannot stand on OpenVid stock. The
    /// index stores no licence field, no author and no release
    /// information (docs/STOCK_LICENSING.md) - no clip has a verified
    /// licence record to import against - so the `monetized` path
    /// refuses outright, before any fetch. The day the index grows a
    /// licence column, this is the gate that consults it.
    pub fn openvid_import(
        &mut self,
        path: &str,
        ids: &[i64],
        start: Option<f64>,
        track: Option<String>,
        monetized: bool,
    ) -> Result<OpenvidImport, ApiError> {
        if ids.is_empty() {
            return Err(ApiError::failed("no clip ids given".to_owned()));
        }
        if ids.len() > 20 {
            return Err(ApiError::failed(
                "at most 20 clips a call; page through them".to_owned(),
            ));
        }
        let effective_monetized = monetized || self.project_is_monetized(path)?;
        if effective_monetized {
            return Err(ApiError::failed(
                "the edit is monetized and OpenVid clips carry no verified licence \
                 record: the index holds no licence column, no author and no release \
                 information (docs/STOCK_LICENSING.md). Clear the clip yourself and \
                 import it with media.import, or plan the edit non-monetized"
                    .to_owned(),
            ));
        }
        self.openvid_ready()?;
        self.writable(path)?;
        let folder = {
            let session = self.session(path)?;
            let folder = std::path::PathBuf::from(session.path()).join("openvid");
            std::fs::create_dir_all(&folder).map_err(|error| {
                ApiError::failed(format!("could not create {}: {error}", folder.display()))
            })?;
            folder
        };
        let library = self.library();
        // Everything is fetched and checked before anything is added, so a
        // clip that will not arrive - or arrives unreadable - refuses the
        // whole request rather than leaving half a set behind.
        let mut copies = Vec::new();
        for id in ids {
            let cached = Self::fetched_clip_checked(library, *id)?;
            let Some(clip) = library.clip(*id).map_err(ApiError::failed)? else {
                return Err(ApiError::failed(format!("there is no clip {id}")));
            };
            // The dataset's own name plus the id: unique for ever, and
            // the same clip imported twice is the same file, copied over
            // itself.
            let name = format!("{}-{id}.mp4", clip.name);
            let into = folder.join(&name);
            std::fs::copy(&cached, &into).map_err(|error| {
                ApiError::failed(format!("could not copy into the project: {error}"))
            })?;
            copies.push(into);
        }
        let mut commands = Vec::new();
        for file in &copies {
            let item = media::probe(&file.to_string_lossy())
                .map_err(ApiError::failed)?
                .to_new_media();
            commands.push(Command::AddMedia { item });
        }
        let mut view = self.apply(path, Command::Batch { commands })?;
        // The ids the copies were given, by the paths they landed at: a
        // batch reports only its last minted id, and the caller needs
        // every one.
        let media_ids: Vec<String> = copies
            .iter()
            .filter_map(|file| {
                view.project
                    .media
                    .iter()
                    .find(|item| item.path == file.to_string_lossy())
                    .map(|item| item.id.clone())
            })
            .collect();
        if media_ids.len() != copies.len() {
            return Err(ApiError::failed(
                "the clips were imported but not all of them were found again".to_owned(),
            ));
        }
        if let Some(start) = start {
            view = self.apply(
                path,
                Command::AddClipsInSequence {
                    media_ids: media_ids.clone(),
                    track_id: track,
                    start,
                    ripple: false,
                },
            )?;
        }
        Ok(OpenvidImport {
            media_ids,
            view: Box::new(view),
        })
    }

    // ── the live stock APIs ────────────────────────────────────────────────

    /// The live shelves' own library, the one [`stock::shared`] holds for
    /// this process.
    fn live_library(&self) -> &'static live_stock::Library {
        live_stock::Library::shared(&self.dirs)
    }

    /// The live provider a caller named, or the refusal that names both.
    fn live_provider(name: &str) -> Result<Provider, ApiError> {
        live_stock::Provider::of(name).ok_or_else(|| {
            ApiError::failed(format!(
                "the provider must be pexels or pixabay, not {:?}",
                name.trim()
            ))
        })
    }

    /// [`Request::StockSearch`]: one live shelf, searched with this
    /// machine's own key (or the one the caller handed), the hits
    /// normalized out of the provider's dialect.
    pub fn stock_search(
        &self,
        provider: &str,
        query: Option<&str>,
        orientation: Option<&str>,
        limit: u32,
        page: u32,
        api_key: Option<&str>,
    ) -> Result<StockFound, ApiError> {
        let provider = Self::live_provider(provider)?;
        if let Some(orientation) = orientation.filter(|orientation| {
            !matches!(
                orientation.trim().to_ascii_lowercase().as_str(),
                "vertical" | "horizontal" | "square"
            )
        }) {
            return Err(ApiError::failed(format!(
                "the orientation must be vertical, horizontal or square, not {orientation:?}"
            )));
        }
        let found = self
            .live_library()
            .search(
                provider,
                query.unwrap_or_default().trim(),
                limit,
                page.max(1),
                orientation.map(str::trim),
                api_key,
            )
            .map_err(ApiError::failed)?;
        let library = self.live_library();
        Ok(StockFound {
            hits: found
                .hits
                .iter()
                .map(|hit| StockHit {
                    id: hit.id.clone(),
                    provider: provider.name().to_owned(),
                    caption: hit.caption.clone(),
                    name: hit.name.clone(),
                    seconds: hit.seconds,
                    width: hit.width,
                    height: hit.height,
                    fps: hit.fps,
                    url: hit.url.clone(),
                    preview: hit.preview.clone(),
                    author: hit.author.clone(),
                    license: hit.license.to_owned(),
                    cached: library.fetched(provider, &hit.id).is_some(),
                    local_path: library
                        .fetched(provider, &hit.id)
                        .map(|path| path.to_string_lossy().into_owned()),
                })
                .collect(),
            total: found.total,
            provider: provider.name().to_owned(),
            license: live_stock::license_of(provider).to_owned(),
        })
    }

    /// [`Request::StockFetch`]: the clip's bytes into the cache, checked
    /// the way OpenVid's own fetch checks them - an unreadable copy is
    /// dropped from the cache, so a second ask re-downloads it.
    pub fn stock_fetch(
        &self,
        provider: &str,
        id: &str,
        url: &str,
    ) -> Result<StockFetched, ApiError> {
        let provider = Self::live_provider(provider)?;
        let path = self.stock_fetch_checked(provider, id, url)?;
        let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        Ok(StockFetched {
            id: id.trim().to_owned(),
            provider: provider.name().to_owned(),
            path: path.to_string_lossy().into_owned(),
            bytes,
        })
    }

    /// The hosts each live shelf actually serves its clips from. A fetch
    /// that takes any caller-named URL would download from anywhere and
    /// stamp the bytes with the provider's licence - licence laundering
    /// on exactly the provenance chain the receipt stands on, and an
    /// internal-network fetch primitive beside it. The host must be one
    /// of the shelf's own, over https.
    fn stock_host_allowed(provider: Provider, url: &str) -> Result<(), ApiError> {
        provider
            .validate_download_url(url)
            .map(|_| ())
            .map_err(ApiError::failed)
    }

    /// The fetched-and-checked path of one live clip: cached as it lies,
    /// fetched when it is not, and refused - with the bad copy dropped -
    /// when the bytes that arrive do not read as MP4 media.
    fn stock_fetch_checked(
        &self,
        provider: Provider,
        id: &str,
        url: &str,
    ) -> Result<PathBuf, ApiError> {
        Self::stock_host_allowed(provider, url)?;
        let library = self.live_library();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let path = library
            .fetch_url(provider, id.trim(), url, &cancel)
            .map_err(ApiError::failed)?;
        if !Self::mp4_head_ok(&path) {
            let _ = std::fs::remove_file(&path);
            return Err(ApiError::failed(format!(
                "the {} clip {id} arrived but does not read as MP4 media; the bad copy was \
                 dropped from the cache - ask again to re-download it",
                provider.name()
            )));
        }
        Ok(path)
    }

    /// [`Request::StockImport`]: fetch if need be, copy into the project's
    /// own `stock` folder, and add to the bin - one undoable step, or two
    /// when the clip is also laid on a lane. The live shelves' licences
    /// allow commercial use, so the monetized edit is served here, not
    /// refused: the licence travels in the reply, the words the import
    /// stands on.
    pub fn stock_import(
        &mut self,
        path: &str,
        provider: &str,
        id: &str,
        url: &str,
        author: Option<&str>,
        start: Option<f64>,
        track: Option<String>,
    ) -> Result<StockImported, ApiError> {
        let provider = Self::live_provider(provider)?;
        if id.trim().is_empty() || url.trim().is_empty() {
            return Err(ApiError::failed(
                "the live import needs the hit's id and the url the search page gave".to_owned(),
            ));
        }
        self.writable(path)?;
        // Everything is fetched and checked before anything is added, so
        // a clip that will not arrive refuses the whole request rather
        // than leaving half an import behind.
        let cached = self.stock_fetch_checked(provider, id, url)?;
        let folder = {
            let session = self.session(path)?;
            let folder = std::path::PathBuf::from(session.path()).join("stock");
            std::fs::create_dir_all(&folder).map_err(|error| {
                ApiError::failed(format!("could not create {}: {error}", folder.display()))
            })?;
            folder
        };
        // The provider's own name plus the id: unique for ever, and the
        // same clip imported twice is the same file, copied over itself.
        let name = format!("{}-{}.mp4", provider.name(), live_stock::safe_stem(id));
        let into = folder.join(&name);
        std::fs::copy(&cached, &into).map_err(|error| {
            ApiError::failed(format!("could not copy into the project: {error}"))
        })?;
        let mut item = media::probe(&into.to_string_lossy())
            .map_err(ApiError::failed)?
            .to_new_media();
        // Where the clip came from, kept beside the file it landed as:
        // once this reply is gone only the document can answer for the
        // clip - the licence it stands on, the creator it credits.
        let licence = live_stock::license_of(provider).to_owned();
        item.stock = Some(concat_project::StockSource {
            provider: provider.name().to_owned(),
            id: id.to_owned(),
            url: url.to_owned(),
            licence: licence.clone(),
            author: author.unwrap_or_default().trim().to_owned(),
        });
        let author = item.stock.as_ref().map_or_default(|stock| stock.author.clone());
        let mut view = self.apply(path, Command::AddMedia { item: item.clone() })?;
        let media_id = view
            .project
            .media
            .iter()
            .find(|item| item.path == into.to_string_lossy())
            .map(|item| item.id.clone())
            .ok_or_else(|| {
                ApiError::failed("the clip was imported but could not be found again".to_owned())
            })?;
        if let Some(start) = start {
            view = self.apply(
                path,
                Command::AddClipsInSequence {
                    media_ids: vec![media_id.clone()],
                    track_id: track,
                    start,
                    ripple: false,
                },
            )?;
        }
        Ok(StockImported {
            media_ids: vec![media_id],
            license: live_stock::license_of(provider).to_owned(),
            author,
            view: Box::new(view),
        })
    }

    // ── the built-in sound library ────────────────────────────────────────

    /// The catalogue every `sounds` method reads: the one this machine
    /// last fetched, or one brought down now when there is none yet - the
    /// same list the window's Audio page loads on demand. `Err` when the
    /// manifest cannot be reached and nothing is cached.
    fn sounds_catalogue(&self) -> Result<sounds::Catalogue, ApiError> {
        if let Some(catalogue) = sounds::cached_catalogue(&self.dirs) {
            return Ok(catalogue);
        }
        sounds::fetch_catalogue(&self.dirs)
            .map_err(ApiError::failed)?
            .ok_or_else(|| ApiError::failed("the sound list is not published yet".to_owned()))
    }

    /// [`Request::SoundsSearch`]: the catalogue narrowed by the filters,
    /// best first. The words of `query` are ANDed and prefix-matched
    /// against each sound's name, id, category, tags, use-for lines and
    /// description - the one search the window's shelf asks too, in
    /// `concat_host::sounds::Catalogue::search` - and where a word
    /// matched says how high its sound stands: the name counts most,
    /// then the tags, then the description, so "fast whoosh" puts a
    /// *Whoosh, fast* ahead of a whoosh merely tagged fast.
    pub fn sounds_search(
        &self,
        query: Option<&str>,
        category: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<SoundsFound, ApiError> {
        let catalogue = self.sounds_catalogue()?;
        let matched = catalogue.search(query.unwrap_or_default(), category);
        let total = matched.len() as u64;
        let limit = limit.unwrap_or(20).clamp(1, 100) as usize;
        let offset = offset.unwrap_or(0) as usize;
        let rows = matched
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|sound| self.sound_entry(sound))
            .collect();
        Ok(SoundsFound {
            sounds: rows,
            total,
        })
    }

    /// A library sound as the API hands it back: the manifest's row, and
    /// where the file is when it is already on this machine.
    fn sound_entry(&self, sound: &sounds::Listed) -> SoundEntry {
        let cached = sounds::cached_file(&self.dirs, sound);
        SoundEntry {
            id: sound.id.clone(),
            name: sound.name.clone(),
            category: sound.category.clone(),
            tags: sound.tags.clone(),
            duration: sound.duration,
            bytes: sound.bytes,
            license: sound.license.clone(),
            author: sound.author.clone(),
            description: sound.description.clone(),
            use_for: sound.use_for.clone(),
            sync_at: sound.sync_at,
            local_path: cached
                .is_file()
                .then(|| cached.to_string_lossy().into_owned()),
        }
    }

    /// Copy a local audio file into one of a project's own folders.
    /// The source keeps its name, so importing the same file twice is the
    /// same file copied over itself rather than a second copy.
    fn copy_local(file: &str, folder: &std::path::Path) -> Result<PathBuf, ApiError> {
        let source = std::path::Path::new(file);
        if !source.is_file() {
            return Err(ApiError::failed(format!("there is no file {file}")));
        }
        let name = source
            .file_name()
            .ok_or_else(|| ApiError::failed(format!("{file} does not name a file")))?;
        let into = folder.join(name);
        std::fs::copy(source, &into).map_err(|error| {
            ApiError::failed(format!("could not copy into the project: {error}"))
        })?;
        Ok(into)
    }

    /// [`Request::SoundsAdd`]: download into the cache when it is not
    /// there yet, copy into the project's own `audio` folder, and add to
    /// the bin - laying it on a lane too when `start` is given, as one
    /// undoable step.
    pub fn sounds_add(
        &mut self,
        path: &str,
        id: Option<&str>,
        start: Option<f64>,
        track: Option<String>,
        file: Option<&str>,
    ) -> Result<SoundsAdded, ApiError> {
        self.writable(path)?;
        // A generated accent carries its licence beside it; a monetized
        // edit is not handed a non-commercial one, whatever the tool that
        // asked for it believed.
        if let Some(file) = file
            && let Some(refusal) = self.generated_licence_refusal(path, file)?
        {
            return Err(ApiError::failed(refusal));
        }
        let folder = {
            let session = self.session(path)?;
            let folder = PathBuf::from(session.path()).join("audio");
            std::fs::create_dir_all(&folder).map_err(|error| {
                ApiError::failed(format!("could not create {}: {error}", folder.display()))
            })?;
            folder
        };
        // A shelf sound arrives from the cache under the library's own
        // file name; a local file (a generated accent, a take) arrives
        // under its own. Either way the project owns the copy.
        let into = if let Some(file) = file {
            Self::copy_local(file, &folder)?
        } else {
            let id = id.ok_or_else(|| {
                ApiError::failed("name `id` (a shelf sound) or `file` (a local one)".to_owned())
            })?;
            let sound = self
                .sounds_catalogue()?
                .get(id)
                .cloned()
                .ok_or_else(|| ApiError::failed(format!("there is no sound {id}")))?;
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let cached = sounds::fetch_sound(&self.dirs, &sound, &cancel, &mut |_, _| {})
                .map_err(ApiError::failed)?;
            let into = folder.join(&sound.file);
            std::fs::copy(&cached, &into).map_err(|error| {
                ApiError::failed(format!("could not copy into the project: {error}"))
            })?;
            into
        };
        let item = media::probe(&into.to_string_lossy())
            .map_err(ApiError::failed)?
            .to_new_media();
        let mut view = self.apply(path, Command::AddMedia { item })?;
        let Some(media_id) = view
            .project
            .media
            .iter()
            .find(|item| item.path == into.to_string_lossy())
            .map(|item| item.id.clone())
        else {
            return Err(ApiError::failed(
                "the sound was imported but could not be found again".to_owned(),
            ));
        };
        let mut clip_id = None;
        if let Some(start) = start {
            view = self.apply(
                path,
                match track {
                    Some(track_id) => Command::AddClip {
                        media_id: media_id.clone(),
                        track_id,
                        start,
                        ripple: false,
                    },
                    None => Command::AddClipAtFirstFree {
                        media_id: media_id.clone(),
                        start,
                        above: false,
                    },
                },
            )?;
            clip_id = view.created_id.clone();
        }
        Ok(SoundsAdded {
            media_id,
            clip_id,
            file: into.to_string_lossy().into_owned(),
            view: Box::new(view),
        })
    }

    /// The one refusal every OpenVid method that needs the index makes:
    /// not on this machine yet, with the words that say what to do about
    /// it.
    fn openvid_ready(&self) -> Result<(), ApiError> {
        match self.library().status() {
            openvid::Status::Ready => Ok(()),
            openvid::Status::Downloading(percent) => Err(ApiError::failed(format!(
                "the OpenVid index is still downloading ({percent}%); poll openvid_status \
                 (openvid.status) until it is ready"
            ))),
            openvid::Status::Missing => Err(ApiError::failed(
                "the OpenVid index is not on this machine yet; call openvid_prepare once, \
                 then poll openvid_status until it is ready"
                    .to_owned(),
            )),
            openvid::Status::Failed(error) => Err(ApiError::failed(format!(
                "the OpenVid index is unusable: {error}"
            ))),
        }
    }

    // ── the background-music library ─────────────────────────────────────

    /// The music library this API reads, from the app's own directories.
    /// One per process, so the window's page and this server share a
    /// download and a cache rather than racing each other.
    fn music(&self) -> &'static bgm_library::Library {
        bgm_library::shared(&self.dirs)
    }

    /// [`Request::BgmStatus`]: both shelves, each in its own word.
    pub fn bgm_state(&self) -> BgmState {
        let library = self.music();
        let word = |source: bgm_library::Source| match library.status(source) {
            bgm_library::Status::Missing => ("missing".to_owned(), None),
            bgm_library::Status::Loading(percent) => {
                (format!("loading ({percent}%)"), Some(percent))
            }
            bgm_library::Status::Ready => ("ready".to_owned(), None),
            bgm_library::Status::Failed(error) => (format!("failed: {error}"), None),
        };
        let (freepd, _) = word(bgm_library::Source::FreePd);
        let (lofi, lofi_percent) = word(bgm_library::Source::Lofi);
        // The per-cent that names the whole state is the slower shelf's:
        // the FreePD listing is one file that arrives in a moment, and the
        // Lo-Fi census is the one a caller can watch.
        let percent = match library.status(bgm_library::Source::Lofi) {
            bgm_library::Status::Loading(percent) => percent,
            _ => match library.status(bgm_library::Source::FreePd) {
                bgm_library::Status::Loading(percent) => percent,
                _ => lofi_percent.unwrap_or(0),
            },
        };
        let error = [
            (bgm_library::Source::FreePd, &freepd),
            (bgm_library::Source::Lofi, &lofi),
        ]
        .into_iter()
        .filter(|(_, word)| word.starts_with("failed"))
        .map(|(source, word)| format!("{}: {}", source.name(), word.trim_start_matches("failed: ")))
        .reduce(|both, one| format!("{both}; {one}"));
        BgmState {
            freepd,
            lofi,
            percent,
            freepd_tracks: library.rows(bgm_library::Source::FreePd),
            lofi_tracks: library.rows(bgm_library::Source::Lofi),
            error,
        }
    }

    /// [`Request::BgmPrepare`]: both shelves' one-time catalog fetches,
    /// started on their own threads because they are a few MB and the
    /// Lo-Fi census after them takes seconds; a caller should not wait
    /// for them. Their progress is what `bgm.status` reports; a second
    /// call while they run is a no-op, and the window's own page starting
    /// one is the same download. A `source` narrows the ask to that one
    /// shelf, so a caller that only wants Lo-Fi never pays for FreePD's
    /// Archive round trip.
    pub fn bgm_prepare(&self, source: Option<&str>) -> Result<(), ApiError> {
        let sources = match source.map(str::to_ascii_lowercase).as_deref() {
            None => vec![bgm_library::Source::FreePd, bgm_library::Source::Lofi],
            Some("freepd") => vec![bgm_library::Source::FreePd],
            Some("lofi") => vec![bgm_library::Source::Lofi],
            Some(other) => {
                return Err(ApiError::new(
                    ErrorCode::Invalid,
                    format!("unknown source {other:?} - freepd or lofi"),
                ));
            }
        };
        let library = self.music();
        for source in sources {
            if matches!(
                library.status(source),
                bgm_library::Status::Ready | bgm_library::Status::Loading(_)
            ) {
                continue;
            }
            std::thread::spawn(move || {
                // This flag is never set: the fetch runs to its end. A
                // cancel is the process going away, which takes the thread
                // with it; the catalogs are re-read from the cache either
                // way.
                let cancel = std::sync::atomic::AtomicBool::new(false);
                // The outcome lands in the status the next caller reads:
                // `Failed`, with these words, rather than a log line this
                // crate has no facade for.
                let _ = library.prepare(source, &cancel);
            });
        }
        Ok(())
    }

    /// [`Request::BgmSearch`]: the filters as the request carries them,
    /// parsed once, with the words a caller chose kept as words.
    #[allow(clippy::too_many_arguments)]
    pub fn bgm_search(
        &self,
        query: Option<String>,
        source: Option<String>,
        category: Option<String>,
        min_seconds: Option<f64>,
        max_seconds: Option<f64>,
        sort: Option<String>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<BgmFound, ApiError> {
        let source = source
            .map(|name| match name.to_ascii_lowercase().as_str() {
                "freepd" => Ok(bgm_library::Source::FreePd),
                "lofi" | "lo-fi" | "openlofi" => Ok(bgm_library::Source::Lofi),
                other => Err(ApiError::failed(format!(
                    "the source must be freepd or lofi, not {other:?}"
                ))),
            })
            .transpose()?;
        let sort = match sort.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("relevance") => BgmSort::Relevance,
            Some("shortest") => BgmSort::Shortest,
            Some("longest") => BgmSort::Longest,
            Some("surprise") => BgmSort::Surprise,
            Some(other) => {
                return Err(ApiError::failed(format!(
                    "the sort must be relevance, shortest, longest or surprise, not {other:?}"
                )));
            }
        };
        let found = self
            .music()
            .search(&BgmQuery {
                text: query,
                source,
                category,
                min_seconds,
                max_seconds,
                sort,
                limit: limit.unwrap_or(20),
                offset: offset.unwrap_or(0),
            })
            .map_err(ApiError::failed)?;
        Ok(BgmFound {
            tracks: found
                .tracks
                .iter()
                .map(|track| BgmTrack::of(track, self.music().fetched(&track.id).as_deref()))
                .collect(),
            total: found.total,
        })
    }

    /// [`Request::BgmFetch`]: the track's own bytes, from whichever shelf
    /// holds it.
    pub fn bgm_fetch(&self, id: &str) -> Result<BgmTrack, ApiError> {
        let library = self.music();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut progress = |_, _| {};
        let path = library
            .fetch(id, &cancel, &mut progress)
            .map_err(ApiError::failed)?;
        let Some(track) = library.track(id).map_err(ApiError::failed)? else {
            return Err(ApiError::failed(format!("there is no track {id}")));
        };
        Ok(BgmTrack::of(&track, Some(&path)))
    }

    /// [`Request::BgmImport`]: fetch, copy into the project's own
    /// `bgm` folder, and add to the bin - one undoable step, or two when
    /// the tracks are also laid on a lane.
    ///
    /// RISK-02: the Lo-Fi shelf is Moby's MobyGratis - free for
    /// non-commercial use only (docs/STOCK_LICENSING.md) - so a
    /// monetised import refuses its tracks before any byte is fetched.
    /// FreePD's CC0 passes.
    pub fn bgm_import(
        &mut self,
        path: &str,
        ids: &[String],
        start: Option<f64>,
        track: Option<String>,
        monetized: bool,
        file: Option<&str>,
    ) -> Result<BgmImport, ApiError> {
        if file.is_none() && ids.is_empty() {
            return Err(ApiError::failed("no track ids given".to_owned()));
        }
        if ids.len() > 20 {
            return Err(ApiError::failed(
                "at most 20 tracks a call; page through them".to_owned(),
            ));
        }
        // A local file is the caller's own - a generated bed, a licensed
        // track they fetched themselves - so the shelf's catalogue has
        // nothing to say about it. What a generated file does say is
        // written beside it: the generator leaves `<file>.licence.json`,
        // and a bed whose record forbids commercial use is refused here
        // in a monetized edit, the same line MobyGratis is held to.
        if let Some(file) = file
            && let Some(refusal) = self.generated_licence_refusal(path, file)?
        {
            return Err(ApiError::failed(refusal));
        }
        let effective_monetized =
            file.is_none() && (monetized || self.project_is_monetized(path)?);
        if effective_monetized {
            // The catalogue's own word when it is loaded; the id's shelf
            // prefix - the source's prefix and twelve hex, a contract
            // that never changed (concat-host/src/bgm.rs) - when it is
            // not. The fetch below remains the existence check either
            // way.
            let library = self.music();
            for id in ids {
                let license = match library.track(id) {
                    Ok(Some(track)) => BgmLicense::of(track.source),
                    _ if id.starts_with("lofi-") => BgmLicense::MobyGratisNonCommercial,
                    _ => BgmLicense::Cc0,
                };
                if !license.permits_commercial_use() {
                    return Err(ApiError::failed(format!(
                        "the edit is monetized and track {id} is MobyGratis \
                         (MobyGratis-NonCommercial): the shelf is free for non-commercial \
                         use only. Shop the FreePD shelf instead - bgm.search with source \
                         freepd - or import the cleared file with media.import"
                    )));
                }
            }
        }
        self.writable(path)?;
        let folder = {
            let session = self.session(path)?;
            let folder = std::path::PathBuf::from(session.path()).join("bgm");
            std::fs::create_dir_all(&folder).map_err(|error| {
                ApiError::failed(format!("could not create {}: {error}", folder.display()))
            })?;
            folder
        };
        let library = self.music();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        // Everything is fetched and copied before anything is added, so a
        // track that will not arrive refuses the whole request rather
        // than leaving half a set behind.
        let mut copies = Vec::new();
        if let Some(file) = file {
            copies.push(Self::copy_local(file, &folder)?);
        }
        for id in ids.iter().filter(|_| file.is_none()) {
            let mut progress = |_, _| {};
            let cached = library
                .fetch(id, &cancel, &mut progress)
                .map_err(ApiError::failed)?;
            let Some(found) = library.track(id).map_err(ApiError::failed)? else {
                return Err(ApiError::failed(format!("there is no track {id}")));
            };
            // The title plus the id: unique for as long as the track is in
            // the catalog, and the same track imported twice is the same
            // file, copied over itself. The tail is read through `get`, so
            // a shorter or multi-byte id can never panic the slice.
            let tail = id.get(id.len().saturating_sub(6)..).unwrap_or(id.as_str());
            let name = format!("{}-{tail}.mp3", clean_name(&found.title));
            let into = folder.join(&name);
            std::fs::copy(&cached, &into).map_err(|error| {
                ApiError::failed(format!("could not copy into the project: {error}"))
            })?;
            copies.push(into);
        }
        let mut commands = Vec::new();
        for file in &copies {
            let item = media::probe(&file.to_string_lossy())
                .map_err(ApiError::failed)?
                .to_new_media();
            commands.push(Command::AddMedia { item });
        }
        let mut view = self.apply(path, Command::Batch { commands })?;
        // The ids the copies were given, by the paths they landed at: a
        // batch reports only its last minted id, and the caller needs
        // every one.
        let media_ids: Vec<String> = copies
            .iter()
            .filter_map(|file| {
                view.project
                    .media
                    .iter()
                    .find(|item| item.path == file.to_string_lossy())
                    .map(|item| item.id.clone())
            })
            .collect();
        if media_ids.len() != copies.len() {
            return Err(ApiError::failed(
                "the tracks were imported but not all of them were found again".to_owned(),
            ));
        }
        if let Some(start) = start {
            view = self.apply(
                path,
                Command::AddClipsInSequence {
                    media_ids: media_ids.clone(),
                    track_id: track,
                    start,
                    ripple: false,
                },
            )?;
        }
        Ok(BgmImport {
            media_ids,
            view: Box::new(view),
        })
    }

    // ── the sticker library ─────────────────────────────────────────────

    /// The sticker library this API reads, from the app's own
    /// directories. One per process, so the window's page and this
    /// server share a download and a cache rather than racing each
    /// other.
    fn stickers(&self) -> &'static sticker_library::Library {
        sticker_library::shared(&self.dirs)
    }

    /// [`Request::MagickStatus`]: the shelf, in its own word.
    pub fn magick_state(&self) -> MagickState {
        let library = self.stickers();
        let word = |source: sticker_library::Source| match library.status(source) {
            sticker_library::Status::Missing => "missing".to_owned(),
            sticker_library::Status::Downloading(percent) => format!("downloading ({percent}%)"),
            sticker_library::Status::Ready => "ready".to_owned(),
            sticker_library::Status::Failed(error) => format!("failed: {error}"),
        };
        let magick = word(sticker_library::Source::Fluent);
        let percent = sticker_library::Source::ALL
            .into_iter()
            .filter_map(|source| match library.status(source) {
                sticker_library::Status::Downloading(percent) => Some(percent),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let error = magick
            .strip_prefix("failed: ")
            .map(|words| format!("{}: {words}", sticker_library::Source::Fluent.name()));
        MagickState {
            magick,
            percent,
            magick_rows: library.rows(sticker_library::Source::Fluent),
            error,
        }
    }

    /// [`Request::MagickPrepare`]: the shelf's one-time index download,
    /// started on its own thread because it is a few hundred KB and a
    /// parse follows it; a caller should not wait for it. Its progress
    /// is what `magick.status` reports; a second call while it runs is a
    /// no-op, and the window's own page starting one is the same
    /// download.
    pub fn magick_prepare(&self) {
        let library = self.stickers();
        for source in sticker_library::Source::ALL {
            if matches!(
                library.status(source),
                sticker_library::Status::Ready | sticker_library::Status::Downloading(_)
            ) {
                continue;
            }
            std::thread::spawn(move || {
                // This flag is never set: the fetch runs to its end. A
                // cancel is the process going away, which takes the
                // thread with it; the indexes are re-read from the cache
                // either way. The outcome lands in the status the next
                // caller reads: `Failed`, with these words, rather than
                // a log line this crate has no facade for.
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let _ = library.prepare(source, &cancel);
            });
        }
    }

    /// [`Request::MagickSearch`]: the filters as the request carries
    /// them, parsed once, with the words a caller chose kept as words.
    pub fn magick_search(
        &self,
        query: Option<&str>,
        source: Option<&str>,
        favourites: bool,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<MagickFound, ApiError> {
        // A shelf that is no shelf's is a refusal in words rather than
        // an empty page, so a caller knows the ask was wrong and not
        // that nothing matched.
        let source = match source.map(sticker_library::Source::parse) {
            Some(Some(source)) => Some(source),
            Some(None) => {
                return Err(ApiError::invalid(
                    "the source must be fluent (or one of the old names, magick or openmoji), \
                     not whatever was given",
                ));
            }
            None => None,
        };
        let found = self
            .stickers()
            .search(&sticker_library::Query {
                text: query.map(str::to_owned),
                source,
                favourites,
                limit: limit.unwrap_or(20),
                offset: offset.unwrap_or(0),
            })
            .map_err(ApiError::failed)?;
        Ok(MagickFound {
            stickers: found
                .stickers
                .iter()
                .map(|sticker| {
                    MagickEntry::of(sticker, self.stickers().fetched(&sticker.id).as_deref())
                })
                .collect(),
            total: found.total,
        })
    }

    /// [`Request::MagickGet`]: everything about up to 20 stickers by id,
    /// from whichever shelf holds each.
    pub fn magick_get(&self, ids: &[String]) -> Result<Vec<MagickEntry>, ApiError> {
        if ids.is_empty() {
            return Err(ApiError::failed("no sticker ids given".to_owned()));
        }
        if ids.len() > 20 {
            return Err(ApiError::failed(
                "at most 20 stickers a call; page through them".to_owned(),
            ));
        }
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.stickers().find(id).map(|sticker| {
                    MagickEntry::of(&sticker, self.stickers().fetched(id).as_deref())
                })
            })
            .collect())
    }

    /// [`Request::MagickFetch`]: the picture's own bytes, one plain GET,
    /// checked to be a PNG that can hold transparency before it is kept.
    pub fn magick_fetch(&self, id: &str) -> Result<Vec<MagickEntry>, ApiError> {
        let sticker = self
            .stickers()
            .find(id)
            .ok_or_else(|| ApiError::failed(format!("there is no sticker {id}")))?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut progress = |_, _| {};
        let path = self
            .stickers()
            .fetch(sticker.source, id, &cancel, &mut progress)
            .map_err(ApiError::failed)?;
        Ok(vec![MagickEntry::of(&sticker, Some(&path))])
    }

    /// [`Request::MagickAdd`]: fetch the picture when it is not on this
    /// machine yet, copy it into the project's own `magick` folder, and
    /// add it to the bin - laying it *over* the picture when `start` is
    /// given, with the placement asked for as one undoable step beside
    /// the clip's own.
    pub fn magick_add(
        &mut self,
        path: &str,
        id: &str,
        placement: Placement,
    ) -> Result<MagickAdd, ApiError> {
        let sticker = self
            .stickers()
            .find(id)
            .ok_or_else(|| ApiError::failed(format!("there is no sticker {id}")))?;
        self.writable(path)?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut progress = |_, _| {};
        let cached = self
            .stickers()
            .fetch(sticker.source, id, &cancel, &mut progress)
            .map_err(ApiError::failed)?;
        let folder = {
            let session = self.session(path)?;
            let folder = PathBuf::from(session.path()).join("magick");
            std::fs::create_dir_all(&folder).map_err(|error| {
                ApiError::failed(format!("could not create {}: {error}", folder.display()))
            })?;
            folder
        };
        // The caption plus the id: unique for as long as the picture is
        // in the library, and the same picture imported twice is the
        // same file, copied over itself.
        let name = format!("{}-{id}.png", clean_name(&sticker.subject));
        let into = folder.join(name);
        std::fs::copy(&cached, &into).map_err(|error| {
            ApiError::failed(format!("could not copy into the project: {error}"))
        })?;
        let item = media::probe(&into.to_string_lossy())
            .map_err(ApiError::failed)?
            .to_new_media();
        let mut view = self.apply(path, Command::AddMedia { item })?;
        let Some(media_id) = view
            .project
            .media
            .iter()
            .find(|item| item.path == into.to_string_lossy())
            .map(|item| item.id.clone())
        else {
            return Err(ApiError::failed(
                "the picture was imported but could not be found again".to_owned(),
            ));
        };
        let mut clip_id = None;
        if let Some(start) = placement.start {
            view = self.apply(
                path,
                match placement.track.clone() {
                    Some(track_id) => Command::AddClip {
                        media_id: media_id.clone(),
                        track_id,
                        start,
                        ripple: false,
                    },
                    // The overlay walk: over the picture, never beneath
                    // it, the way a title lands.
                    None => Command::AddClipAtFirstFree {
                        media_id: media_id.clone(),
                        start,
                        above: true,
                    },
                },
            )?;
            clip_id = view.created_id.clone();
            // The placement, when any was asked for: the clip's own
            // shape, size and angle as one undoable step beside the
            // clip's own.
            let mut commands = Vec::new();
            if let (Some(duration), Some(clip)) = (placement.duration, clip_id.as_deref()) {
                let now = view
                    .project
                    .active()
                    .clips
                    .iter()
                    .find(|held| held.id == clip)
                    .map(|held| held.duration)
                    .unwrap_or_default();
                commands.push(Command::TrimClip {
                    clip_id: clip.to_owned(),
                    edge: TrimEdge::End,
                    delta: duration - now,
                    ripple: false,
                });
            }
            if let Some(clip) = clip_id.as_deref()
                && (placement.scale.is_some()
                    || placement.offset_x.is_some()
                    || placement.offset_y.is_some()
                    || placement.rotation.is_some())
            {
                commands.push(Command::SetClipTransform {
                    clip_id: clip.to_owned(),
                    scale: placement.scale,
                    offset_x: placement.offset_x,
                    offset_y: placement.offset_y,
                    rotation: placement.rotation,
                    stretch_x: None,
                    stretch_y: None,
                });
            }
            if !commands.is_empty() {
                view = self.apply(path, Command::Batch { commands })?;
            }
        }
        Ok(MagickAdd {
            media_id,
            clip_id,
            file: into.to_string_lossy().into_owned(),
            view: Box::new(view),
        })
    }

    /// [`Request::TemplateInstantiate`].
    pub fn instantiate(
        &mut self,
        template: &str,
        location: &str,
        name: &str,
        fills: Vec<Fill>,
    ) -> Result<EditorView, ApiError> {
        self.writable(location)?;
        // Every file is probed before anything is made, so a bad path
        // refuses the whole request rather than leaving a folder behind.
        let fills = fills
            .into_iter()
            .map(|fill| {
                Ok(SlotFill {
                    media_id: fill.media_id,
                    item: media::probe(&fill.file)
                        .map_err(ApiError::failed)?
                        .to_new_media(),
                })
            })
            .collect::<Result<Vec<SlotFill>, ApiError>>()?;
        let info =
            templates::instantiate(template, location, name, fills).map_err(ApiError::failed)?;
        self.adopt(info)
    }

    /// [`Request::TemplateSave`].
    pub fn save_template(
        &mut self,
        path: &str,
        name: &str,
    ) -> Result<concat_host::templates::TemplateInfo, ApiError> {
        let session = self.session(path)?;
        let document = session.document().map_err(ApiError::failed)?;
        templates::save(
            &self.dirs.config,
            &document,
            &session.settings(),
            session.path(),
            name,
        )
        .map_err(ApiError::failed)
    }

    /// [`Request::ExportRun`]: what the window's Export sheet does, in its
    /// order, on a thread of its own. Masks first, because the frame loop
    /// reads whatever is in the project's cache and draws the picture as
    /// shot where there is none; then the render, with the titles already
    /// painted here and rejoining the clip list as stills.
    pub fn export(&mut self, path: &str, spec: &ExportSpec) -> Result<Started, ApiError> {
        self.writable(&spec.output)?;
        let session = self.session(path)?;
        if session.project().active().clips.is_empty() {
            return Err(ApiError::new(
                ErrorCode::Refused,
                "There is nothing on the timeline to export",
            ));
        }
        let settings = session.settings();
        let (width, height) = checked_size(
            spec.width.unwrap_or(settings.width),
            spec.height.unwrap_or(settings.height),
        )?;
        if let Some(crf) = spec.crf
            && crf > MAX_CRF
        {
            return Err(ApiError::invalid(format!("crf {crf} is over {MAX_CRF}")));
        }
        let rate_num = spec.rate_num.unwrap_or(settings.rate_num);
        let rate_den = spec.rate_den.unwrap_or(settings.rate_den);
        checked_rate(rate_num, rate_den)?;
        let hdr = session.video().color_space.is_hdr() && spec.hdr.unwrap_or(true);
        let codec = match spec.codec.as_deref() {
            None if hdr => export::VideoCodec::Hevc,
            None => export::VideoCodec::H264,
            Some(name) => export::VideoCodec::parse(name).ok_or_else(|| {
                ApiError::invalid(format!("unknown codec {name:?}: h264, hevc or av1"))
            })?,
        };
        if hdr && codec == export::VideoCodec::H264 {
            return Err(ApiError::invalid(
                "H.264 does not carry HDR: hevc or av1, or hdr false for SDR".to_owned(),
            ));
        }
        let color_range = match spec.color_range.as_deref() {
            None => export::ColorRange::Limited,
            Some(name) => export::ColorRange::parse(name).ok_or_else(|| {
                ApiError::invalid(format!("unknown colour range {name:?}: limited or full"))
            })?,
        };
        let host_spec = export::ExportSpec {
            output: spec.output.clone(),
            crf: spec.crf.unwrap_or(DEFAULT_CRF),
            preset: spec
                .preset
                .clone()
                .unwrap_or_else(|| DEFAULT_PRESET.to_owned()),
            codec,
            ten_bit: spec.ten_bit.unwrap_or(false),
            rate_mode: export::RateMode::Vbr,
            bitrate_kbps: 0,
            color_range,
            hdr,
            // The hardware encoders lead, as every export always has; the
            // sheet's GPU-by-name choice is the window's own, and an API
            // export has not said it.
            hardware: true,
            vaapi: false,
        };

        let project_path = session.path().to_owned();
        let project_dir = PathBuf::from(&project_path);
        let masks: Vec<(String, AnalyseRequest)> =
            Cutouts::requests(session.project(), &project_dir)
                .into_iter()
                .filter(|(_, request)| Cutouts::outstanding(request) != 0)
                .collect();
        let steadies: Vec<(String, StabilizeRequest)> =
            Stabilizers::requests(session.project(), &project_dir)
                .into_iter()
                .filter(|(_, request)| !Stabilizers::outstanding(request).is_empty())
                .collect();
        let titles = self.title_clips(session, width, height);
        let mut request = export::request(session, &host_spec, titles);
        request.width = width;
        request.height = height;
        request.rate_num = rate_num;
        request.rate_den = rate_den;

        let slot = self
            .exporter
            .begin()
            .map_err(|message| ApiError::new(ErrorCode::Busy, message))?;
        let started = Started {
            job: self.jobs.mint(),
            path: project_path,
            output: spec.output.clone(),
        };
        self.book.start(
            JobStatus::running(&started.job, &started.path, &started.output),
            slot.cancel_handle(),
            job::Side::Export,
        );
        let job = ExportJob {
            name: started.job.clone(),
            path: started.path.clone(),
            request,
            masks,
            cutouts: Arc::clone(&self.cutouts),
            steadies,
            stabilizers: Arc::clone(&self.stabilizers),
            events: Arc::clone(&self.events),
        };
        let book = Arc::clone(&self.book);
        let name = started.job.clone();
        let thread = std::thread::Builder::new()
            .name(format!("export {}", started.job))
            .spawn(move || job.run(slot))
            .map_err(|error| {
                book.abandon(&name);
                ApiError::failed(format!("could not start the export: {error}"))
            })?;
        self.jobs.threads.push((started.job.clone(), thread));
        Ok(started)
    }

    /// [`Request::ExportCancel`].
    pub fn cancel(&mut self, job: &str) -> Result<(), ApiError> {
        self.jobs.reap(&self.book);
        if !self.jobs.threads.iter().any(|(name, _)| name == job) {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no job {job} is running"),
            ));
        }
        // The shared machinery is reached only when the named job is
        // the export that holds it: the exporter slot is shared with
        // the window, and an export the window started - or a cutout it
        // is finding, or a render it is waiting on - has no name here to
        // be cancelled by. An analysis or a voice is stopped by its own
        // flag and nothing else's.
        if self.book.is_export(job) {
            // Whichever phase it is in: the analysis ahead of the
            // render, or the render itself.
            self.cutouts.cancel();
            self.stabilizers.cancel();
            self.exporter.cancel();
        }
        // And the jobs that are only this API's: an analysis or a voice.
        self.book.cancel(job);
        Ok(())
    }

    /// The timeline's titles painted for a `width` × `height` frame, as
    /// the stills that stand in for them.
    fn title_clips(&self, session: &Session, width: u32, height: u32) -> Vec<ExportClip> {
        self.titles
            .clips(session.project(), width, height)
            .into_iter()
            .map(|title| title.clip)
            .collect()
    }

    /// [`Request::PreviewFrame`] with an output: the paused monitor's true
    /// frame, to a file. A frame asked for small - a thumbnail, a page
    /// of an edit - is read through the proxies where the media has
    /// them, whole, the same picture from the smaller file; a frame at
    /// the timeline's own size stays on the originals.
    pub fn frame(
        &mut self,
        path: &str,
        time: f64,
        output: &str,
        size: Option<(u32, u32)>,
    ) -> Result<Written, ApiError> {
        self.writable(output)?;
        let (width, height, pixels) = self.pixels(path, time, size)?;
        write_png(Path::new(output), width, height, &pixels)?;
        Ok(Written {
            path: output.to_owned(),
            width,
            height,
        })
    }

    /// [`Request::PreviewFrame`] without one: the same frame, inline,
    /// small frames included in the proxies' service.
    pub fn picture(
        &mut self,
        path: &str,
        time: f64,
        size: Option<(u32, u32)>,
    ) -> Result<Picture, ApiError> {
        let (width, height, pixels) = self.pixels(path, time, size)?;
        let mut png = Vec::new();
        encode_png(&mut png, width, height, &pixels)
            .map_err(|error| ApiError::failed(format!("could not encode the frame: {error}")))?;
        Ok(Picture {
            width,
            height,
            png: base64::engine::general_purpose::STANDARD.encode(png),
        })
    }

    /// The composited RGBA frame at `time`.
    fn pixels(
        &mut self,
        path: &str,
        time: f64,
        size: Option<(u32, u32)>,
    ) -> Result<(u32, u32, Vec<u8>), ApiError> {
        let session = self.session(path)?;
        let settings = session.settings();
        let (width, height) = size.unwrap_or((settings.width, settings.height));
        let (width, height) = checked_size(width, height)?;
        let mut clips = session.flattened_clips();
        clips.extend(self.title_clips(session, width, height));
        // A frame asked for small is read through the proxies where the
        // media has them: every copy is at least 960 along its long
        // side, so a frame no longer than that is served whole - the
        // same picture, from the smaller file, which is what a caller
        // paging through an edit wants. A frame at the timeline's own
        // size stays on the originals: the paused monitor's frame is
        // the true one, and a proxy drawn at full size is a blur.
        let proxy =
            size.is_some_and(|(width, height)| concat_host::proxy::serves_frame(width, height));
        let pixels = self
            .monitor
            .frame(
                Arc::new(clips),
                &settings,
                FrameSpec {
                    time,
                    width,
                    height,
                    moving: false,
                    proxy,
                    color_space: session.video().color_space,
                },
            )
            .map_err(ApiError::failed)?;
        Ok((width, height, pixels))
    }

    /// Read the monetized flag from the opened project's persisted creative
    /// brief. Import request flags are additional caller assertions, not
    /// authority: a direct MCP/API caller must not bypass a monetized
    /// project's licence gate by omitting or setting monetized to false.
    /// A legacy project without a brief remains non-monetized; a malformed
    /// brief is refused rather than treated as permission.
    fn project_is_monetized(&self, path: &str) -> Result<bool, ApiError> {
        let session = self.session(path)?;
        let brief_path = std::path::PathBuf::from(session.path()).join("creative-brief.json");
        let contents = match std::fs::read_to_string(&brief_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(ApiError::failed(format!(
                    "could not read the project's creative brief: {error}"
                )));
            }
        };
        let brief: serde_json::Value = serde_json::from_str(&contents).map_err(|error| {
            ApiError::failed(format!(
                "the project's creative brief is invalid JSON: {error}"
            ))
        })?;
        match brief.get("monetized") {
            None => Ok(false),
            Some(serde_json::Value::Bool(value)) => Ok(*value),
            Some(_) => Err(ApiError::failed(
                "the project's creative brief monetized field must be a boolean".to_owned(),
            )),
        }
    }

    /// Why a generated file may not enter this edit, when it may not: the
    /// licence record `sound_model.py generate` leaves beside its own
    /// output, read against the project's monetized flag. A file with no
    /// record is the caller's own and answers to nothing here.
    fn generated_licence_refusal(
        &self,
        path: &str,
        file: &str,
    ) -> Result<Option<String>, ApiError> {
        let record = std::path::PathBuf::from(format!("{file}.licence.json"));
        let contents = match std::fs::read_to_string(&record) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Ok(None),
        };
        let record: serde_json::Value = match serde_json::from_str(&contents) {
            Ok(record) => record,
            Err(_) => return Ok(None),
        };
        if record
            .get("commercial")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        {
            return Ok(None);
        }
        if !self.project_is_monetized(path)? {
            return Ok(None);
        }
        let model = record
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("a generated sound");
        let licence = record
            .get("licence")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("a non-commercial licence");
        Ok(Some(format!(
            "the edit is monetized and {model} writes under {licence}, which does not \
             allow commercial use. Generate the bed with `sfx-large` (Stability AI \
             Community Licence), shop the CC0 shelf, or mark the project non-monetized"
        )))
    }

    fn session(&self, path: &str) -> Result<&Session, ApiError> {
        self.sessions
            .get(&key_of(path))
            .ok_or_else(|| not_open(path))
    }

    fn session_mut(&mut self, path: &str) -> Result<&mut Session, ApiError> {
        self.sessions
            .get_mut(&key_of(path))
            .ok_or_else(|| not_open(path))
    }
}

/// The jobs the API has begun, by name, with the thread each runs on.
#[derive(Default)]
struct Jobs {
    minted: u64,
    threads: Vec<(String, JoinHandle<()>)>,
}

impl Jobs {
    /// The next name.
    fn mint(&mut self) -> String {
        self.minted += 1;
        format!("j{}", self.minted)
    }

    /// Forgets the threads that have ended. One that ended without its
    /// last event - a panic in the render - is booked as failed.
    fn reap(&mut self, book: &job::Book) {
        self.threads.retain(|(name, thread)| {
            let ended = thread.is_finished();
            if ended {
                book.abandon(name);
            }
            !ended
        });
    }

    /// Waits for the rest.
    fn finish(&mut self) {
        for (_, thread) in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// An export in flight: everything its thread needs, owned.
struct ExportJob {
    name: String,
    path: String,
    request: ExportRequest,
    masks: Vec<(String, AnalyseRequest)>,
    cutouts: Arc<Cutouts>,
    steadies: Vec<(String, StabilizeRequest)>,
    stabilizers: Arc<Stabilizers>,
    events: EventSink,
}

impl ExportJob {
    /// The whole job, start to end, on the thread it is called from. The
    /// slot is held until this returns, so the next export waits its turn.
    fn run(self, slot: concat_host::jobs::Job) {
        let outcome = self
            .masks(&slot)
            .and_then(|()| self.steadies(&slot))
            .and_then(|()| {
                export::run(&self.request, slot.cancel_flag(), |progress| {
                    (self.events)(Event::progress(&self.name, &self.path, progress));
                })
            });
        let event = match outcome {
            Ok(output) => Event::ExportDone {
                job: self.name.clone(),
                path: self.path.clone(),
                output,
                width: self.request.width,
                height: self.request.height,
            },
            Err(message) => Event::ExportFailed {
                job: self.name.clone(),
                path: self.path.clone(),
                error: if slot.cancelled() {
                    ApiError::new(ErrorCode::Cancelled, "The export was cancelled")
                } else {
                    ApiError::failed(message)
                },
            },
        };
        (self.events)(event);
    }

    /// Runs every cutout analysis the timeline still needs, one after the
    /// other, so the render that follows cuts every clip it should.
    fn masks(&self, slot: &concat_host::jobs::Job) -> Result<(), String> {
        for (media_id, request) in &self.masks {
            if slot.cancelled() {
                return Err("cancelled".to_owned());
            }
            self.cutouts.analyse(request, &mut |progress| {
                let (fetching, fraction) = match progress {
                    cutout::Progress::Fetching { received, total } => {
                        (true, received as f32 / total.max(1) as f32)
                    }
                    cutout::Progress::Analysing(fraction) => (false, fraction),
                };
                (self.events)(Event::CutoutProgress {
                    job: self.name.clone(),
                    path: self.path.clone(),
                    media_id: media_id.clone(),
                    fetching,
                    fraction,
                });
            })?;
        }
        Ok(())
    }
}

impl ExportJob {
    /// Measures every stabilized clip's media the timeline still needs,
    /// one after the other, so the render that follows steadies every
    /// clip it should.
    fn steadies(&self, slot: &concat_host::jobs::Job) -> Result<(), String> {
        for (media_id, request) in &self.steadies {
            if slot.cancelled() {
                return Err("cancelled".to_owned());
            }
            self.stabilizers.analyse(request, &mut |fraction| {
                (self.events)(Event::StabilizeProgress {
                    job: self.name.clone(),
                    path: self.path.clone(),
                    media_id: media_id.clone(),
                    fraction,
                });
            })?;
        }
        Ok(())
    }
}

/// [`Request::CatalogueList`]: the built-in packages plus whatever
/// [`Catalogue::install`] added, in id order.
fn catalogue(kind: Option<&str>) -> Result<Vec<PackageInfo>, ApiError> {
    let kind = match kind {
        None => None,
        Some("effect") => Some(Kind::Effect),
        Some("filter") => Some(Kind::Filter),
        Some("audio") => Some(Kind::Audio),
        Some("transition") => Some(Kind::Transition),
        Some("generator") => Some(Kind::Generator),
        Some(other) => {
            return Err(ApiError::invalid(format!(
                "{other:?} is not a kind: effect, filter, audio, transition or generator"
            )));
        }
    };
    let mut packages: Vec<PackageInfo> = Catalogue::builtin()
        .packages()
        .filter(|package| kind.is_none_or(|kind| package.kind() == kind))
        .map(|package| {
            let meta = &package.manifest.effect;
            PackageInfo {
                id: meta.id.clone(),
                name: meta.name.clone(),
                kind: format!("{:?}", meta.kind).to_ascii_lowercase(),
                category: meta.category.clone(),
                description: meta.description.clone(),
                intensity: meta.intensity.clone(),
                params: package
                    .manifest
                    .params
                    .iter()
                    .map(|param| ParamInfo {
                        key: param.key.clone(),
                        label: param.label.clone(),
                        kind: format!("{:?}", param.kind).to_ascii_lowercase(),
                        min: param.min,
                        max: param.max,
                        default: param.default,
                        step: param.step,
                        unit: param.unit.clone(),
                        animate: param.animate,
                        values: param.values.clone(),
                        labels: param.labels.clone(),
                    })
                    .collect(),
            }
        })
        .collect();
    packages.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(packages)
}

/// The key a project folder is held under: its canonical path where the
/// folder exists, so `.` and an absolute spelling of it are one session,
/// and the path as given where it does not yet.
impl Drop for Api {
    /// The register is shared with an embedder that outlives this API:
    /// what it held open is given back.
    fn drop(&mut self) {
        for key in self.sessions.keys() {
            self.open.release(key);
        }
    }
}

/// A frame size a caller may ask for, or why not.
fn checked_size(width: u32, height: u32) -> Result<(u32, u32), ApiError> {
    if width == 0 || height == 0 {
        return Err(ApiError::invalid("A frame needs a width and a height"));
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(ApiError::invalid(format!(
            "{width}×{height} is over {MAX_SIDE} a side"
        )));
    }
    Ok((width, height))
}

/// A frame rate a caller may ask for, or why not.
fn checked_rate(num: i64, den: i64) -> Result<(), ApiError> {
    if num <= 0 || den <= 0 {
        return Err(ApiError::invalid(
            "A frame rate needs a positive numerator and denominator",
        ));
    }
    if num as f64 / den as f64 > MAX_RATE {
        return Err(ApiError::invalid(format!(
            "{num}/{den} is over {MAX_RATE} frames a second"
        )));
    }
    Ok(())
}

/// `path` with its symlinks resolved as far as it exists: the deepest
/// ancestor that is there, canonicalised, with the rest appended. A file
/// not written yet still says where it would land.
fn resolved(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            let mut out = real;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The semantic rerank's state: whether the two MobileCLIP2-S0 ONNX
/// files `analysis/rerank.py` looks for are on this machine. The same
/// three places, in the same order, so what this reports is what the
/// search will do - and the reply a director reads names the folder to
/// put the files in rather than leaving the half silent.
fn rerank_state(dirs: &concat_host::dirs::AppDirs) -> OpenvidRerank {
    /// The two files, by the names the rerank looks for.
    const FILES: [&str; 2] = ["mobileclip2_s0_vision.onnx", "mobileclip2_s0_text.onnx"];
    /// Whether both files sit in `dir`.
    fn holds_both(dir: &Path) -> bool {
        FILES.iter().all(|file| dir.join(file).is_file())
    }
    let candidates: [(PathBuf, &str); 3] = [
        (
            std::env::var_os("FUNCUT_MOBILECLIP")
                .map(PathBuf::from)
                .unwrap_or_default(),
            "$FUNCUT_MOBILECLIP",
        ),
        (
            dirs.data.join("models").join("mobileclip2-s0"),
            "the app data dir's models/mobileclip2-s0",
        ),
        (
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".cache").join("concat").join("mobileclip2-s0"))
                .unwrap_or_default(),
            "~/.cache/concat/mobileclip2-s0",
        ),
    ];
    for (dir, _) in &candidates {
        if holds_both(dir) {
            return OpenvidRerank {
                available: true,
                dir: dir.to_string_lossy().into_owned(),
                files: FILES,
            };
        }
    }
    // Not there: name the place to put them, the first real one.
    let (dir, place) = candidates
        .iter()
        .find(|(dir, _)| !dir.as_os_str().is_empty())
        .unwrap_or(&candidates[1]);
    OpenvidRerank {
        available: false,
        dir: if dir.as_os_str().is_empty() {
            (*place).to_owned()
        } else {
            dir.to_string_lossy().into_owned()
        },
        files: FILES,
    }
}

fn key_of(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|canonical| canonical.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_owned())
}

/// The hash of an applied view's serialized whole (F-05): what the
/// idempotency ledger keeps per key, small enough that remembering
/// thousands of them costs nothing against the full `EditorView` each
/// one used to hold.
fn view_hash(view: &EditorView) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Ok(whole) = serde_json::to_string(view) {
        hasher.write(whole.as_bytes());
    }
    hasher.finish()
}

/// A sticker's placement on the timeline, as `magick.add` was asked for
/// it: every field optional, `start` being the one that puts the picture
/// on a lane at all.
pub struct Placement {
    /// Lay it on the timeline from this moment, in seconds.
    start: Option<f64>,
    /// How long it stays; the still's default hold when absent.
    duration: Option<f64>,
    /// The lane by name; the overlay walk when absent.
    track: Option<String>,
    /// Scale, 1 filling the frame.
    scale: Option<f64>,
    /// Horizontal placement, frame widths from the centre, positive
    /// right.
    offset_x: Option<f64>,
    /// Vertical placement, frame heights from the centre, positive down.
    offset_y: Option<f64>,
    /// Rotation in degrees.
    rotation: Option<f64>,
}

/// A track's title as a file name a project can hold: the characters a
/// plain name cannot carry taken out, the whole thing cut to a length a
/// name was meant to be. The id beside it is what makes the name unique;
/// this only makes it polite.
fn clean_name(title: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.');
    let cut: String = clean.chars().take(60).collect();
    if cut.is_empty() {
        "Track".to_owned()
    } else {
        cut
    }
}

fn not_open(path: &str) -> ApiError {
    ApiError::new(
        ErrorCode::NotOpen,
        format!("{path} is not open - open it first"),
    )
}

/// The edit layer's no, as the code that says so.
fn refused(message: String) -> ApiError {
    ApiError::new(ErrorCode::Refused, message)
}

/// Writes RGBA pixels as a PNG, creating the folder above the file.
fn write_png(output: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<(), ApiError> {
    let could_not = |error: &dyn std::fmt::Display| {
        ApiError::failed(format!("could not write {}: {error}", output.display()))
    };
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            ApiError::failed(format!("could not create {}: {error}", parent.display()))
        })?;
    }
    let file = std::fs::File::create(output).map_err(|error| could_not(&error))?;
    encode_png(std::io::BufWriter::new(file), width, height, pixels)
        .map_err(|error| could_not(&error))
}

/// Encodes RGBA pixels as a PNG into `sink`.
fn encode_png(
    sink: impl std::io::Write,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(sink, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::commands::NewMedia;
    use concat_project::model::MediaKind;
    use serde_json::json;
    use std::sync::Mutex;

    /// An API whose config and data live in a scratch folder that goes
    /// away with the test, and whose events pile up where a test can read
    /// them.
    fn api() -> (Api, tempfile::TempDir, Arc<Mutex<Vec<Event>>>) {
        let scratch = tempfile::tempdir().expect("scratch");
        let dirs = AppDirs {
            config: scratch.path().join("config"),
            data: scratch.path().join("data"),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let api = Api::with_dirs(
            dirs,
            Arc::new(move |event| sink.lock().expect("events").push(event)),
        );
        (api, scratch, events)
    }

    #[test]
    fn a_corrupt_cache_entry_is_dropped_not_served() {
        // The head check: a real faststart MP4 head passes, a proxy error
        // page with the size padded right does not (DEG-08).
        let scratch = tempfile::tempdir().expect("scratch");
        let good = scratch.path().join("good.mp4");
        let mut mp4 = b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom".to_vec();
        mp4.extend_from_slice(&[0_u8; 64]);
        std::fs::write(&good, mp4).expect("writes");
        assert!(
            Api::mp4_head_ok(&good),
            "an ftyp box at the front is an MP4"
        );
        let bad = scratch.path().join("bad.mp4");
        std::fs::write(&bad, b"<html><body>502 Bad Gateway</body></html>").expect("writes");
        assert!(
            !Api::mp4_head_ok(&bad),
            "an error page is not a clip however many bytes it claims"
        );
        let empty = scratch.path().join("empty.mp4");
        std::fs::write(&empty, b"").expect("writes");
        assert!(!Api::mp4_head_ok(&empty), "nothing is not a clip");
    }

    fn ok(response: Response) -> Reply {
        match response {
            Response::Result(reply) => reply,
            Response::Error(error) => panic!("refused: {error}"),
        }
    }

    fn err(response: Response) -> ApiError {
        match response {
            Response::Result(_) => panic!("worked, unexpectedly"),
            Response::Error(error) => error,
        }
    }

    /// The footage filters accept the snake_case spellings the footage
    /// brief uses, landing on exactly the fields their camelCase names
    /// carry - a caller following the brief and a caller following the
    /// schema send the same filter.
    #[test]
    fn the_footage_filters_accept_their_snake_case_aliases() {
        let request: Request = serde_json::from_value(json!({
            "method": "openvid.search",
            "query": "ocean sunset",
            "motion_min": 1.5,
            "exclude_ids": [820_338_756_299i64, 42],
        }))
        .expect("the aliases deserialize");
        let Request::OpenvidSearch {
            min_motion,
            exclude_ids,
            ..
        } = request
        else {
            panic!("the search request");
        };
        assert_eq!(min_motion, Some(1.5));
        assert_eq!(exclude_ids, vec![820_338_756_299, 42]);

        // And the camelCase names, unchanged.
        let request: Request = serde_json::from_value(json!({
            "method": "openvid.search",
            "minMotion": 2.0,
            "excludeIds": [7],
        }))
        .expect("the camelCase names deserialize");
        let Request::OpenvidSearch {
            min_motion,
            exclude_ids,
            ..
        } = request
        else {
            panic!("the search request");
        };
        assert_eq!(min_motion, Some(2.0));
        assert_eq!(exclude_ids, vec![7]);

        // A search that names neither filter filters nothing: the field
        // defaults are empty and absent, the library as it comes.
        let request: Request = serde_json::from_value(json!({
            "method": "openvid.search",
            "query": "ocean",
        }))
        .expect("no filters at all");
        let Request::OpenvidSearch {
            min_motion,
            exclude_ids,
            ..
        } = request
        else {
            panic!("the search request");
        };
        assert_eq!(min_motion, None);
        assert!(exclude_ids.is_empty());
    }

    fn view(reply: Reply) -> EditorView {
        match reply {
            Reply::View(view) => *view,
            _ => panic!("not a view"),
        }
    }

    fn still(path: &str) -> NewMedia {
        NewMedia {
            path: path.to_owned(),
            name: "still.png".to_owned(),
            duration: None,
            kind: MediaKind::Image,
            width: Some(640),
            height: Some(360),
            frame_rate: None,
            frame_rate_fraction: None,
            video_codec: None,
            audio_codec: None,
            has_audio: false,
            audio_tracks: Vec::new(),
            origin: None,
            color_space: Default::default(),
        }
    }

    /// A project named `name` under the scratch folder, open.
    fn project(api: &mut Api, scratch: &tempfile::TempDir, name: &str) -> String {
        let location = scratch.path().to_string_lossy().into_owned();
        ok(api.dispatch(Request::ProjectCreate {
            location: location.clone(),
            name: name.to_owned(),
            video: None,
        }));
        scratch.path().join(name).to_string_lossy().into_owned()
    }

    /// An edit applied through the API survives a close that never saved:
    /// the autosave after every applied batch is what stands between an
    /// agent's edit and a crash, and what keeps an imported file from
    /// sitting in the folder with no document naming it.
    #[test]
    fn an_applied_edit_survives_a_close_that_never_saved() {
        let (mut api, scratch, _events) = api();
        let path = project(&mut api, &scratch, "Autosaved");
        let added = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        let media_id = added.created_id.expect("minted");
        ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id,
                start: 2.0,
                above: false,
            }),
            idempotency_key: None,
        }));
        ok(api.dispatch(Request::ProjectClose {
            path: path.clone(),
            save: false,
        }));
        let reopened = view(ok(api.dispatch(Request::ProjectOpen { path })));
        assert_eq!(
            reopened.project.active().clips.len(),
            1,
            "the placed clip is on disk, not only in the session that was dropped"
        );
        assert_eq!(reopened.project.active().clips[0].start, 2.0);
    }

    /// A `width` by `height` video of plain frames, where a test needs a
    /// file the probe reads back with the size it was written at.
    fn video_at(scratch: &tempfile::TempDir, name: &str, width: u32, height: u32) -> String {
        use concat_media::{EncodeOptions, Encoder, FrameSink, VideoCodec};
        let path = scratch.path().join(name);
        let mut encoder = Encoder::create(
            &path,
            width,
            height,
            concat_core::time::FrameRate::THIRTY,
            &EncodeOptions {
                codec: VideoCodec::H264,
                preset: "ultrafast".to_owned(),
                ..EncodeOptions::default()
            },
        )
        .expect("encodes");
        let frame = concat_core::frame::Frame::from_rgba(
            width,
            height,
            vec![64u8; (width as usize) * (height as usize) * 4],
        )
        .expect("a frame");
        for _ in 0..6 {
            encoder.write_frame(&frame).expect("writes");
        }
        encoder.finish().expect("finishes");
        path.to_string_lossy().into_owned()
    }

    /// An agent's import gets its proxy without a window to ask for one:
    /// the copy is written on the proxy lane and adopted into the pool
    /// the frames are read through, while a video below HD - under the
    /// size a proxy starts at - never gets one. A project reopened
    /// finds the copy again and sweeps nothing it holds.
    #[test]
    fn an_agents_import_gets_its_proxy() {
        let (mut api, scratch, _events) = api();
        let path = project(&mut api, &scratch, "Proxied");
        let big = video_at(&scratch, "big.mp4", 2560, 1440);
        let small = video_at(&scratch, "small.mp4", 1280, 720);
        ok(api.dispatch(Request::MediaImport {
            path: path.clone(),
            file: big.clone(),
        }));
        ok(api.dispatch(Request::MediaImport {
            path: path.clone(),
            file: small.clone(),
        }));
        // The write is a job nobody waits on; the lanes drain before the
        // copy is there to be found.
        concat_host::scheduler().drain();
        let project_dir = Path::new(&path);
        assert!(
            concat_host::proxy::existing(project_dir, &big, None).is_some(),
            "the larger file's working copy was written"
        );
        assert!(
            concat_host::proxy::existing(project_dir, &small, None).is_none(),
            "a file below HD never gets one"
        );
        assert!(
            concat_host::scheduler()
                .pool()
                .proxy_of(Path::new(&big))
                .is_some(),
            "the copy is what the pool reads for the file"
        );
        ok(api.dispatch(Request::ProjectClose {
            path: path.clone(),
            save: false,
        }));
        ok(api.dispatch(Request::ProjectOpen { path: path.clone() }));
        assert!(
            concat_host::proxy::existing(Path::new(&path), &big, None).is_some(),
            "the copy survived the reopen, and was not swept as stale"
        );
    }

    #[test]
    fn version_names_what_the_build_serves() {
        let (mut api, _scratch, _events) = api();
        let base = api.version();
        assert_eq!(base.api_version, API_VERSION);
        // The build's own number: bare outside a release tag, the tag's
        // own iteration inside one - never upstream's bare number alone,
        // which no agent could tell from an upstream install.
        let expected = match option_env!("FUNCUT_ITERATION") {
            Some(iteration) => format!("{}-funcut.{iteration}", env!("CARGO_PKG_VERSION")),
            None => env!("CARGO_PKG_VERSION").to_owned(),
        };
        assert_eq!(base.concat, expected);
        assert_eq!(base.capabilities, vec!["events".to_owned()]);
        assert!(!base.capabilities.iter().any(|name| name == "gpu"));

        api.add_capability("json-rpc");
        api.add_capability("json-rpc");
        api.add_capability("grpc");
        let served = serde_json::to_value(ok(api.dispatch(Request::Version))).expect("JSON");
        assert_eq!(
            served["capabilities"],
            json!(["events", "json-rpc", "grpc"]),
            "each name once, in the order added"
        );
    }

    #[test]
    fn a_request_parses_by_method_and_carries_a_command_verbatim() {
        let request: Request = serde_json::from_value(json!({
            "method": "edit.apply",
            "path": "/p",
            "command": { "op": "addTextClip", "start": 1.5 }
        }))
        .expect("parses");
        assert_eq!(
            request,
            Request::EditApply {
                path: "/p".to_owned(),
                command: Box::new(Command::AddTextClip {
                    above: false,
                    track_id: None,
                    start: 1.5,
                    style: None,
                    duration: None,
                    offset_y: None,
                }),
                idempotency_key: None,
            }
        );
    }

    #[test]
    fn a_response_is_a_result_or_an_error_object() {
        let worked = serde_json::to_value(Response::Result(Reply::Done(Done {}))).expect("json");
        assert_eq!(worked, json!({ "result": {} }));
        let refused =
            serde_json::to_value(Response::Error(ApiError::new(ErrorCode::Refused, "no")))
                .expect("json");
        assert_eq!(
            refused,
            json!({ "error": { "code": "refused", "message": "no" } })
        );
    }

    #[test]
    fn create_edit_save_and_reopen_round_trip() {
        let (mut api, scratch, _) = api();
        let location = scratch.path().join("projects");
        std::fs::create_dir_all(&location).expect("location");
        let location = location.to_string_lossy().into_owned();

        let created = view(ok(api.dispatch(Request::ProjectCreate {
            location: location.clone(),
            name: "Round trip".to_owned(),
            video: Some(VideoSettings {
                width: 1080,
                height: 1920,
                rate_num: 60,
                rate_den: 1,
                color_space: Default::default(),
            }),
        })));
        assert_eq!(created.settings.width, 1080);
        assert_eq!(created.settings.height, 1920);
        assert_eq!(created.settings.rate_num, 60);
        let path = format!("{location}/Round trip");
        assert!(projects::is_project(Path::new(&path)));

        let added = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        let media_id = added.created_id.expect("minted");
        let placed = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id,
                start: 2.0,
                above: false,
            }),
            idempotency_key: None,
        })));
        assert!(placed.can_undo);
        assert_eq!(placed.project.active().clips.len(), 1);

        ok(api.dispatch(Request::ProjectClose {
            path: path.clone(),
            save: true,
        }));
        assert_eq!(
            err(api.dispatch(Request::ProjectGet { path: path.clone() })),
            not_open(&path)
        );

        let reopened = view(ok(api.dispatch(Request::ProjectOpen { path: path.clone() })));
        assert_eq!(reopened.project.active().clips.len(), 1);
        assert_eq!(reopened.project.active().clips[0].start, 2.0);
        assert!(!reopened.can_undo, "history does not survive a save");

        let recents = match ok(api.dispatch(Request::ProjectList)) {
            Reply::Projects(list) => list,
            _ => panic!("not a list"),
        };
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].name, "Round trip");
    }

    /// A clip's mask over the wire, as a caller writes it: put on through
    /// `updateClip`'s patch, its numbers keyed through `setClipKey`, and
    /// read back in the project the reply carries (#239).
    #[test]
    fn a_mask_is_set_and_keyed_through_the_api() {
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Masked");
        let added = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        let placed = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id: added.created_id.expect("minted"),
                start: 0.0,
                above: false,
            }),
            idempotency_key: None,
        })));
        let clip_id = placed.project.active().clips[0].id.clone();
        for command in [
            json!({ "op": "updateClip", "clipId": clip_id,
                    "patch": { "mask": { "shape": "star", "feather": 0.3, "invert": true } } }),
            json!({ "op": "setClipKey", "clipId": clip_id, "property": "maskWidth",
                    "at": 0.0, "value": 0.2 }),
            json!({ "op": "setClipKey", "clipId": clip_id, "property": "maskWidth",
                    "at": 1.0, "value": 1.2 }),
        ] {
            let request: Request = serde_json::from_value(json!({
                "method": "edit.apply", "path": path, "command": command
            }))
            .expect("parses");
            ok(api.dispatch(request));
        }
        let got = view(ok(api.dispatch(Request::ProjectGet { path })));
        let clip = &got.project.active().clips[0];
        let mask = clip.mask.expect("a mask");
        assert_eq!(mask.shape, concat_project::model::MaskShape::Star);
        assert!(mask.invert && (mask.feather - 0.3).abs() < 1e-12);
        let width = clip.value_at(concat_project::model::KeyProperty::MaskWidth, 0.5);
        assert!((width - 0.7).abs() < 1e-12, "the key rides: {width}");
    }

    #[test]
    fn a_refusal_is_the_command_layers_sentence_with_its_code() {
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Refused");
        let created = view(ok(api.dispatch(Request::ProjectGet { path: path.clone() })));
        let track_id = created.project.active().tracks[0].id.clone();
        let refused = err(api.dispatch(Request::EditApply {
            path,
            command: Box::new(Command::AddClip {
                media_id: "m999".to_owned(),
                track_id,
                start: 0.0,
                ripple: false,
            }),
            idempotency_key: None,
        }));
        assert_eq!(refused.code, ErrorCode::Refused);
        assert_eq!(refused.message, "That media is no longer in the bin.");
    }

    #[test]
    fn a_keyed_apply_lands_once_no_matter_how_often_it_is_re_sent() {
        // IDEM: the same idempotency key, re-sent after the reply was
        // lost or the run restarted, returns the recorded view and adds
        // no second edit - the history holds one step, the undo steps
        // back over one, and a second key (or none) edits as usual.
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Keyed");
        let created = view(ok(api.dispatch(Request::ProjectGet { path: path.clone() })));
        let track_id = created.project.active().tracks[0].id.clone();
        let media = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        let media_id = media.created_id.expect("minted");
        let place = |key: Option<&str>| Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClip {
                media_id: media_id.clone(),
                track_id: track_id.clone(),
                start: 0.0,
                ripple: false,
            }),
            idempotency_key: key.map(str::to_owned),
        };
        let first = view(ok(api.dispatch(place(Some("run-1:timeline")))));
        let before = first.project.active().clips.len();
        let again = view(ok(api.dispatch(place(Some("run-1:timeline")))));
        assert_eq!(
            again.project.active().clips.len(),
            before,
            "a re-sent key adds no second clip"
        );
        assert_eq!(
            again.project.active().clips[0].id,
            first.project.active().clips[0].id,
            "a re-sent key answers with the recorded view"
        );
        let other = view(ok(api.dispatch(place(Some("run-2:timeline")))));
        assert_eq!(
            other.project.active().clips.len(),
            before + 1,
            "a different key is a different edit"
        );
        let plain = view(ok(api.dispatch(place(None))));
        assert_eq!(
            plain.project.active().clips.len(),
            before + 2,
            "an unkeyed apply edits as it always did"
        );
    }

    #[test]
    fn a_keyed_apply_survives_a_restart_of_the_api() {
        // F-06: the keys land in <project>/.idempotency-ledger.json, are
        // read when the project opens again, and a re-sent key after the
        // restart adds no second edit - the ledger is the durable half of
        // idempotency, not just the memory's.
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Ledgered");
        let created = view(ok(api.dispatch(Request::ProjectGet { path: path.clone() })));
        let track_id = created.project.active().tracks[0].id.clone();
        let media = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        let media_id = media.created_id.expect("minted");
        let place = || Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClip {
                media_id: media_id.clone(),
                track_id: track_id.clone(),
                start: 0.0,
                ripple: false,
            }),
            idempotency_key: Some("run-1:timeline".to_owned()),
        };
        let first = view(ok(api.dispatch(place())));
        let once = first.project.active().clips.len();
        drop(api); // the restart: a whole new API over the same dirs.

        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let mut api = Api::with_dirs(
            AppDirs {
                config: scratch.path().join("config"),
                data: scratch.path().join("data"),
            },
            Arc::new(move |event| sink.lock().expect("events").push(event)),
        );
        ok(api.dispatch(Request::ProjectOpen { path: path.clone() }));
        let again = view(ok(api.dispatch(place())));
        assert_eq!(
            again.project.active().clips.len(),
            once,
            "a re-sent key after the restart adds no second clip"
        );
        let other = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClip {
                media_id: media_id.clone(),
                track_id: track_id.clone(),
                start: 9.0,
                ripple: false,
            }),
            idempotency_key: Some("run-2:timeline".to_owned()),
        })));
        assert_eq!(
            other.project.active().clips.len(),
            once + 1,
            "a different key is still a different edit after the restart"
        );
        // The ledger itself, on the disk it promised: this project's key
        // with a hash beside it.
        let ledger =
            std::fs::read_to_string(std::path::Path::new(&path).join(".idempotency-ledger.json"))
                .expect("the ledger was written");
        let ledger: serde_json::Value = serde_json::from_str(&ledger).unwrap();
        assert_eq!(ledger["schema"], "funcut.idempotency-ledger/1");
        let entries = ledger["entries"].as_array().unwrap();
        assert!(
            entries.iter().any(
                |entry| entry["key"] == "run-1:timeline" && entry["hash"].as_u64().unwrap() > 0
            ),
            "the ledger carries the key with its hash: {entries:?}"
        );
    }

    #[test]
    fn the_durable_ledger_adopts_its_rows_oldest_first() {
        // F-06: `rows_for` writes the ledger newest first, so the restart
        // adopts it oldest first - the ring keeps its record order, and
        // the first eviction past the cap drops the oldest keys, never
        // the newest ones a resumed run is about to re-send. Adopting
        // the file's own order filled the ring backwards and evicted
        // exactly the keys the resume still owes.
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Adopted");
        let created = view(ok(api.dispatch(Request::ProjectGet { path: path.clone() })));
        let track_id = created.project.active().tracks[0].id.clone();
        let media = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        let media_id = media.created_id.expect("minted");
        for number in 0..4 {
            ok(api.dispatch(Request::EditApply {
                path: path.clone(),
                command: Box::new(Command::AddClip {
                    media_id: media_id.clone(),
                    track_id: track_id.clone(),
                    start: number as f64 * 10.0,
                    ripple: false,
                }),
                idempotency_key: Some(format!("run-1:cut-{number}")),
            }));
        }
        drop(api); // the restart: a whole new API over the same dirs.
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let mut api = Api::with_dirs(
            AppDirs {
                config: scratch.path().join("config"),
                data: scratch.path().join("data"),
            },
            Arc::new(move |event| sink.lock().expect("events").push(event)),
        );
        ok(api.dispatch(Request::ProjectOpen { path: path.clone() }));
        let keys: Vec<&str> = api
            .applied
            .order
            .iter()
            .map(|id| id.rsplit('\n').next().unwrap())
            .collect();
        assert_eq!(
            keys,
            vec!["run-1:cut-0", "run-1:cut-1", "run-1:cut-2", "run-1:cut-3"],
            "the restarted ring is oldest first, ready to evict the oldest"
        );
    }

    #[test]
    fn the_recorded_views_are_bounded_not_the_keys() {
        // F-05: the ring keeps a view only for the most recent keys; an
        // older key still answers - from the project as it stands, never
        // a second edit - and its hash stays in the ledger. A long
        // running API's memory no longer grows with every keyed edit.
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Bounded");
        let created = view(ok(api.dispatch(Request::ProjectGet { path: path.clone() })));
        let track_id = created.project.active().tracks[0].id.clone();
        let media = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        let media_id = media.created_id.expect("minted");
        for number in 0..(super::KEYED_VIEWS_KEPT + 4) {
            ok(api.dispatch(Request::EditApply {
                path: path.clone(),
                command: Box::new(Command::AddClip {
                    media_id: media_id.clone(),
                    track_id: track_id.clone(),
                    start: number as f64 * 10.0,
                    ripple: false,
                }),
                idempotency_key: Some(format!("run-1:cut-{number}")),
            }));
        }
        let total = view(ok(api.dispatch(Request::ProjectGet { path: path.clone() })));
        let total = total.project.active().clips.len();
        // The oldest key is past the view window: its re-send is answered
        // from the project as it stands - the edit lands once, and no
        // view is held for it any more.
        let replay = ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClip {
                media_id: media_id.clone(),
                track_id: track_id.clone(),
                start: 0.0,
                ripple: false,
            }),
            idempotency_key: Some("run-1:cut-0".to_owned()),
        }));
        let replay = view(replay);
        assert_eq!(
            replay.project.active().clips.len(),
            total,
            "a key past the view window still lands once"
        );
        assert!(
            api.applied
                .answer(&format!("{path}\nrun-1:cut-0"))
                .is_some_and(|answer| answer.view.is_none()),
            "the oldest key's view is dropped, its hash kept"
        );
    }

    #[test]
    fn an_edit_whose_autosave_failed_says_so_in_the_reply() {
        // F-07: the call still lands - the edit already happened, and
        // refusing it would only invite a retry that doubles it - but the
        // reply carries the save's own failure on `unsaved`, so no caller
        // believes a success the disk never saw.
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Unsaved");
        // Block the manifest the save renames onto, the way the close
        // test does: a directory where the file must land.
        let manifest = concat_host::projects::manifest_path(std::path::Path::new(&path));
        let _ = std::fs::remove_file(&manifest);
        std::fs::create_dir_all(&manifest).expect("a blocking directory");
        let applied = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        assert!(applied.can_undo, "the edit itself landed");
        let unsaved = applied.unsaved.expect("the reply says the save failed");
        assert!(
            unsaved.contains("could not")
                || unsaved.contains(&manifest.to_string_lossy().to_string()),
            "the unsaved note names the failure: {unsaved}"
        );
        let _ = std::fs::remove_dir_all(&manifest);
    }

    #[test]
    fn creating_over_a_project_is_refused() {
        let (mut api, scratch, _) = api();
        let location = scratch.path().to_string_lossy().into_owned();
        let create = || Request::ProjectCreate {
            location: location.clone(),
            name: "Twice".to_owned(),
            video: None,
        };
        ok(api.dispatch(create()));
        let again = err(api.dispatch(create()));
        assert_eq!(again.code, ErrorCode::Failed);
        assert!(again.message.contains("already exists"));
    }

    #[test]
    fn the_catalogue_lists_packages_with_their_knobs() {
        let packages = catalogue(Some("filter")).expect("kind");
        assert!(!packages.is_empty());
        assert!(packages.iter().all(|package| package.kind == "filter"));
        assert!(packages.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert_eq!(
            catalogue(Some("look")).expect_err("not a kind").code,
            ErrorCode::Invalid
        );
        let all = catalogue(None).expect("all");
        assert!(all.len() > packages.len());
        assert!(all.iter().any(|package| !package.params.is_empty()));
    }

    #[test]
    fn a_method_on_a_closed_project_says_so() {
        let (mut api, _scratch, _) = api();
        let refused = err(api.dispatch(Request::EditUndo {
            path: "/never".to_owned(),
        }));
        assert_eq!(refused, not_open("/never"));
        assert_eq!(refused.code, ErrorCode::NotOpen);
    }

    /// The sticker library refuses until its indexes are on this machine,
    /// with the words that say what to do about it; a shelf that is no
    /// shelf's never reaches the library at all; and an id no shelf holds
    /// is a refusal of its own - the ask was wrong, not the library
    /// empty. Nothing here touches the network.
    #[test]
    fn the_sticker_library_refuses_until_prepared() {
        let (mut api, _scratch, _) = api();
        let state = match ok(api.dispatch(Request::MagickStatus)) {
            Reply::Magick(state) => state,
            _ => panic!("the status is the status"),
        };
        assert_eq!(state.magick, "missing");
        assert_eq!(state.percent, 0);
        assert!(state.error.is_none());
        let refused = err(api.dispatch(Request::MagickSearch {
            query: Some("waving".to_owned()),
            source: None,
            favourites: None,
            limit: None,
            offset: None,
        }));
        assert!(
            refused.message.contains("magick.prepare"),
            "{}",
            refused.message
        );
        // A shelf that is no shelf's is refused before the library is
        // asked.
        let wrong = err(api.dispatch(Request::MagickSearch {
            query: None,
            source: Some("nonsense".to_owned()),
            favourites: None,
            limit: None,
            offset: None,
        }));
        assert_eq!(wrong.code, ErrorCode::Invalid);
        // And an id no shelf holds is a refusal of its own.
        let unknown = err(api.dispatch(Request::MagickFetch {
            id: "noSuchSticker".to_owned(),
        }));
        assert!(
            unknown.message.contains("no sticker noSuchSticker"),
            "{}",
            unknown.message
        );
    }

    #[test]
    fn undo_and_redo_step_the_history() {
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "History");
        ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddTextClip {
                above: false,
                track_id: None,
                start: 0.0,
                style: None,
                duration: Some(3.0),
                offset_y: None,
            }),
            idempotency_key: None,
        }));
        let undone = view(ok(api.dispatch(Request::EditUndo { path: path.clone() })));
        assert!(undone.project.active().clips.is_empty());
        assert!(undone.can_redo);
        let redone = view(ok(api.dispatch(Request::EditRedo { path })));
        assert_eq!(redone.project.active().clips.len(), 1);
    }

    #[test]
    fn the_document_is_what_a_save_writes() {
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Doc");
        let document = match ok(api.dispatch(Request::ProjectDocument { path: path.clone() })) {
            Reply::Document(document) => document,
            _ => panic!("not a document"),
        };
        ok(api.dispatch(Request::ProjectSave {
            path: path.clone(),
            name: None,
        }));
        assert_eq!(
            projects::read_document(&path)
                .expect("saved")
                .expect("a document"),
            document
        );
    }

    #[test]
    fn an_empty_timeline_has_nothing_to_export_and_no_job_to_cancel() {
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Empty");
        let refused = err(api.dispatch(Request::ExportRun {
            path,
            spec: ExportSpec {
                output: scratch
                    .path()
                    .join("out.mp4")
                    .to_string_lossy()
                    .into_owned(),
                ..ExportSpec::default()
            },
        }));
        assert_eq!(refused.code, ErrorCode::Refused);
        let missing = err(api.dispatch(Request::ExportCancel {
            job: "j1".to_owned(),
        }));
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(api.running(), 0);
    }

    #[test]
    fn an_export_is_a_job_that_reports_how_it_ended() {
        let (mut api, scratch, events) = api();
        let path = project(&mut api, &scratch, "Job");
        // A still that does not exist: the job starts, since the timeline
        // has a clip, and fails in the render, which is an event.
        let added = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddMedia {
                item: still("/nowhere/still.png"),
            }),
            idempotency_key: None,
        })));
        ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id: added.created_id.expect("minted"),
                start: 0.0,
                above: false,
            }),
            idempotency_key: None,
        }));
        let started = match ok(api.dispatch(Request::ExportRun {
            path: path.clone(),
            spec: ExportSpec {
                output: scratch
                    .path()
                    .join("out.mp4")
                    .to_string_lossy()
                    .into_owned(),
                ..ExportSpec::default()
            },
        })) {
            Reply::Started(started) => started,
            _ => panic!("not a job"),
        };
        assert_eq!(started.job, "j1");
        api.finish();
        let events = events.lock().expect("events");
        let last = events.last().expect("the job said how it ended");
        assert_eq!(last.job(), "j1");
        match last {
            Event::ExportFailed {
                path: at, error, ..
            } => {
                assert_eq!(at, &path);
                assert_eq!(error.code, ErrorCode::Failed);
            }
            other => panic!("ended with {other:?}"),
        }
        drop(events);
        assert_eq!(api.running(), 0);
        // Asked afterwards, by a caller who was not listening, the job
        // says the same.
        let status = match ok(api.dispatch(Request::ExportStatus {
            job: Some("j1".to_owned()),
            path: None,
        })) {
            Reply::Job(status) => status,
            _ => panic!("not a job's status"),
        };
        assert_eq!(status.state, JobState::Failed);
        assert_eq!(status.path, path);
        assert_eq!(
            status.error.map(|error| error.code),
            Some(ErrorCode::Failed)
        );
        let unknown = err(api.dispatch(Request::ExportStatus {
            job: Some("j9".to_owned()),
            path: None,
        }));
        assert_eq!(unknown.code, ErrorCode::NotFound);
        // Asked by what it was started on - the project folder - the same
        // job answers, for a caller that never held the name.
        let by_path = match ok(api.dispatch(Request::ExportStatus {
            job: None,
            path: Some(path.clone()),
        })) {
            Reply::Job(status) => status,
            _ => panic!("not a job's status"),
        };
        assert_eq!(by_path.job, status.job);
        assert_eq!(by_path.state, JobState::Failed);
        let no_path = err(api.dispatch(Request::ExportStatus {
            job: None,
            path: Some("/nowhere/at/all".to_owned()),
        }));
        assert_eq!(no_path.code, ErrorCode::NotFound);
        let nameless = err(api.dispatch(Request::ExportStatus {
            job: None,
            path: None,
        }));
        assert_eq!(nameless.code, ErrorCode::Invalid);
        assert_eq!(api.stopper().cancel_all(), 0, "nothing left to stop");
    }

    #[test]
    fn a_frame_comes_back_inline_when_no_output_is_named() {
        let (mut api, scratch, _) = api();
        let path = project(&mut api, &scratch, "Frame");
        let picture = match ok(api.dispatch(Request::PreviewFrame {
            path: path.clone(),
            time: 0.0,
            output: None,
            width: Some(16),
            height: Some(9),
        })) {
            Reply::Picture(picture) => picture,
            _ => panic!("not a picture"),
        };
        assert_eq!((picture.width, picture.height), (16, 9));
        let png = base64::engine::general_purpose::STANDARD
            .decode(picture.png)
            .expect("base64");
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let reader = decoder.read_info().expect("a PNG");
        assert_eq!((reader.info().width, reader.info().height), (16, 9));

        let refused = err(api.dispatch(Request::PreviewFrame {
            path,
            time: 0.0,
            output: None,
            width: Some(0),
            height: Some(9),
        }));
        assert_eq!(refused.code, ErrorCode::Invalid);
    }

    #[test]
    fn writes_stay_under_the_roots() {
        let (mut api, scratch, _events) = api();
        let allowed = scratch.path().join("allowed");
        std::fs::create_dir_all(&allowed).expect("allowed");
        api.restrict_writes_to(vec![allowed.clone()]);
        let outside = scratch
            .path()
            .join("elsewhere")
            .to_string_lossy()
            .into_owned();
        let refused = err(api.dispatch(Request::ProjectCreate {
            location: outside.clone(),
            name: "Out".to_owned(),
            video: None,
        }));
        assert_eq!(refused.code, ErrorCode::Refused);
        assert!(!scratch.path().join("elsewhere").exists(), "nothing made");
        let inside = allowed.to_string_lossy().into_owned();
        ok(api.dispatch(Request::ProjectCreate {
            location: inside.clone(),
            name: "In".to_owned(),
            video: None,
        }));
        let path = format!("{inside}/In");
        for output in [
            format!("{inside}/../elsewhere/frame.png"),
            format!("{outside}/frame.png"),
        ] {
            let refused = err(api.dispatch(Request::PreviewFrame {
                path: path.clone(),
                time: 0.0,
                output: Some(output.clone()),
                width: None,
                height: None,
            }));
            assert_eq!(refused.code, ErrorCode::Refused, "{output}");
        }
        // Under the root, into a folder not there yet, is fine.
        let written = serde_json::to_value(ok(api.dispatch(Request::PreviewFrame {
            path,
            time: 0.0,
            output: Some(format!("{inside}/frames/first.png")),
            width: Some(16),
            height: Some(9),
        })))
        .expect("JSON");
        assert!(
            written["path"]
                .as_str()
                .is_some_and(|written| written.ends_with("first.png"))
        );
    }

    #[test]
    fn a_frame_over_8k_a_side_is_invalid() {
        let (mut api, scratch, _events) = api();
        let path = project(&mut api, &scratch, "Big");
        let refused = err(api.dispatch(Request::PreviewFrame {
            path,
            time: 0.0,
            output: None,
            width: Some(65_535),
            height: Some(65_535),
        }));
        assert_eq!(refused.code, ErrorCode::Invalid);
    }

    #[test]
    fn a_project_the_window_holds_is_refused_and_the_other_way_round() {
        let (mut api, scratch, _events) = api();
        let path = project(&mut api, &scratch, "Mine");
        ok(api.dispatch(Request::ProjectClose {
            path: path.clone(),
            save: false,
        }));
        let open = OpenProjects::default();
        api.share_open_projects(open.clone());
        open.claim(&path, Holder::Window)
            .expect("the window has it");
        let refused = err(api.dispatch(Request::ProjectOpen { path: path.clone() }));
        assert_eq!(refused.code, ErrorCode::Refused);
        open.release(&path);
        ok(api.dispatch(Request::ProjectOpen { path: path.clone() }));
        assert_eq!(open.holder(&path), Some(Holder::Api));
        assert_eq!(
            open.claim(&path, Holder::Window),
            Err(Refusal::Here(Holder::Api)),
            "and the window is refused in turn"
        );
        assert!(
            Path::new(&path).join(lock::FILE).exists(),
            "and every other process by the lock"
        );
        ok(api.dispatch(Request::ProjectClose {
            path: path.clone(),
            save: false,
        }));
        assert_eq!(open.holder(&path), None, "closing gives it back");
        assert!(!Path::new(&path).join(lock::FILE).exists(), "lock and all");
    }

    /// Two registers are two processes as far as the lock goes: an API
    /// that does not share the window's register - `concat --mcp` on its
    /// own - is kept off a folder the window has, and told the way in.
    #[test]
    fn a_project_another_process_holds_is_refused_with_the_way_in() {
        let (mut other, _other_scratch, _other_events) = api();
        let (mut api, scratch, _events) = api();
        let path = project(&mut api, &scratch, "Held");
        ok(api.dispatch(Request::ProjectClose {
            path: path.clone(),
            save: false,
        }));
        let window = lock::ProjectLock::acquire(Path::new(&path), "window").expect("the window");
        let refused = err(api.dispatch(Request::ProjectOpen { path: path.clone() }));
        assert_eq!(refused.code, ErrorCode::Refused);
        assert!(
            refused.message.contains("a Concat window (process"),
            "{}",
            refused.message
        );
        assert!(
            refused.message.contains("concat --mcp window"),
            "{}",
            refused.message
        );
        drop(window);
        ok(api.dispatch(Request::ProjectOpen { path: path.clone() }));

        let refused = err(other.dispatch(Request::ProjectOpen { path: path.clone() }));
        assert!(
            refused.message.contains("another Concat process"),
            "{}",
            refused.message
        );
    }

    /// RISK-02: every track the `bgm` methods hand back carries its
    /// shelf's licence - FreePD as CC0-1.0, MobyGratis as
    /// non-commercial-only - so a caller can decline a track before it
    /// is ever fetched.
    #[test]
    fn bgm_tracks_carry_their_shelfs_licence() {
        let shelf = |source: concat_host::bgm::Source| concat_host::bgm::Track {
            id: format!(
                "{}-0123456789ab",
                match source {
                    concat_host::bgm::Source::FreePd => "freepd",
                    concat_host::bgm::Source::Lofi => "lofi",
                }
            ),
            source,
            title: "Night Pulse".to_owned(),
            artist: "the artist".to_owned(),
            category: "late-night".to_owned(),
            duration: 180.0,
            bytes: 4_000_000,
            file: "120 Monster.mp3".to_owned(),
        };
        let freepd = BgmTrack::of(&shelf(concat_host::bgm::Source::FreePd), None);
        assert_eq!(freepd.license, BgmLicense::Cc0);
        assert!(freepd.license.permits_commercial_use());
        let lofi = BgmTrack::of(&shelf(concat_host::bgm::Source::Lofi), None);
        assert_eq!(lofi.license, BgmLicense::MobyGratisNonCommercial);
        assert!(!lofi.license.permits_commercial_use());
        let served = serde_json::to_value(&lofi).expect("JSON");
        assert_eq!(served["license"], json!("MobyGratis-NonCommercial"));
        let served = serde_json::to_value(&freepd).expect("JSON");
        assert_eq!(served["license"], json!("CC0-1.0"));
    }

    /// RISK-01: a monetized import of OpenVid stock is refused before
    /// anything is fetched - the index carries no per-clip licence
    /// record - while the same call un-monetized fails for the index's
    /// own reason instead, proving the gate is the flag's.
    #[test]
    fn a_monetized_openvid_import_is_refused() {
        let (mut api, _scratch, _events) = api();
        let refused = err(api.dispatch(Request::OpenvidImport {
            path: "/nowhere".to_owned(),
            ids: vec![7],
            start: None,
            track: None,
            monetized: true,
        }));
        assert!(
            refused.message.contains("no verified licence"),
            "the refusal names the missing licence record: {refused}"
        );
        assert!(
            refused.message.contains("monetized"),
            "the refusal names the monetized flag: {refused}"
        );
        let other = err(api.dispatch(Request::OpenvidImport {
            path: "/nowhere".to_owned(),
            ids: vec![7],
            start: None,
            track: None,
            monetized: false,
        }));
        assert!(
            !other.message.contains("no verified licence"),
            "without the flag the refusal is the index's own: {other}"
        );
    }

    /// The persisted project brief is authoritative: a direct import
    /// cannot turn a monetized project into a non-monetized one by sending
    /// monetized=false or omitting the request field.
    #[test]
    fn persisted_monetized_brief_cannot_be_overridden_by_import_flags() {
        let (mut api, scratch, _events) = api();
        let location = scratch.path().join("projects");
        std::fs::create_dir_all(&location).expect("projects folder");
        let name = "monetized-edit";
        let path = location.join(name);
        let path_text = path.to_string_lossy().into_owned();
        ok(api.dispatch(Request::ProjectCreate {
            location: location.to_string_lossy().into_owned(),
            name: name.to_owned(),
            video: None,
        }));
        std::fs::write(path.join("creative-brief.json"), r#"{"monetized":true}"#)
            .expect("persisted brief");

        let openvid = err(api.dispatch(Request::OpenvidImport {
            path: path_text.clone(),
            ids: vec![7],
            start: None,
            track: None,
            monetized: false,
        }));
        assert!(
            openvid.message.contains("monetized") && openvid.message.contains("verified licence"),
            "the persisted brief must arm the OpenVid gate: {openvid}"
        );

        let bgm = err(api.dispatch(Request::BgmImport {
            path: path_text,
            ids: vec!["lofi-0123456789ab".to_owned()],
            start: None,
            track: None,
            file: None,
            monetized: false,
        }));
        assert!(
            bgm.message.contains("MobyGratis"),
            "the persisted brief must arm the MobyGratis gate: {bgm}"
        );
    }

    /// RISK-02: a monetized import refuses MobyGratis (lofi) tracks
    /// before any byte is fetched; FreePD passes the licence gate and
    /// fails for the project's own reason instead.
    #[test]
    fn a_monetized_bgm_import_refuses_mobygratis() {
        let (mut api, _scratch, _events) = api();
        let refused = err(api.dispatch(Request::BgmImport {
            path: "/nowhere".to_owned(),
            ids: vec!["lofi-0123456789ab".to_owned()],
            start: None,
            track: None,
            file: None,
            monetized: true,
        }));
        assert!(
            refused.message.contains("MobyGratis"),
            "the refusal names the shelf's terms: {refused}"
        );
        assert!(
            refused.message.contains("non-commercial"),
            "the refusal says what the licence forbids: {refused}"
        );
        // The same lofi track, un-monetized, is not stopped by the
        // licence gate: it proceeds to fail for the project's own
        // reason, proving the gate is the flag's.
        let unmonetized = err(api.dispatch(Request::BgmImport {
            path: "/nowhere".to_owned(),
            ids: vec!["lofi-0123456789ab".to_owned()],
            start: None,
            track: None,
            file: None,
            monetized: false,
        }));
        assert!(
            !unmonetized.message.contains("MobyGratis"),
            "without the flag the licence gate stays out of the way: {unmonetized}"
        );
        // FreePD is CC0: the monetized import passes the licence gate
        // and proceeds towards the project, whatever it says about it.
        let freepd = err(api.dispatch(Request::BgmImport {
            path: "/nowhere".to_owned(),
            ids: vec!["freepd-0123456789ab".to_owned()],
            start: None,
            track: None,
            file: None,
            monetized: true,
        }));
        assert!(
            !freepd.message.contains("MobyGratis"),
            "the CC0 shelf is not refused by the monetized gate: {freepd}"
        );
    }

    /// A generated bed carries the licence it was written under beside it
    /// (`sound_model.py generate` writes `<file>.licence.json`). A
    /// monetized edit refuses a non-commercial one and takes a commercial
    /// one; a file with no record is the caller's own and is not asked.
    #[test]
    fn a_generated_bed_is_held_to_the_licence_written_beside_it() {
        let (mut api, scratch, _events) = api();
        let location = scratch.path().join("projects");
        std::fs::create_dir_all(&location).expect("projects folder");
        let name = "generated-bed";
        let path = location.join(name);
        let path_text = path.to_string_lossy().into_owned();
        ok(api.dispatch(Request::ProjectCreate {
            location: location.to_string_lossy().into_owned(),
            name: name.to_owned(),
            video: None,
        }));
        std::fs::write(path.join("creative-brief.json"), r#"{"monetized":true}"#)
            .expect("persisted brief");

        let bed = scratch.path().join("bed.wav");
        std::fs::write(&bed, b"RIFF").expect("a file to point at");
        let bed_text = bed.to_string_lossy().into_owned();
        std::fs::write(
            format!("{bed_text}.licence.json"),
            r#"{"model":"tinymusician","licence":"CC-BY-NC-4.0 (MusicGen-small weights)","commercial":false}"#,
        )
        .expect("the generator's record");

        let refused = err(api.dispatch(Request::BgmImport {
            path: path_text.clone(),
            ids: Vec::new(),
            start: None,
            track: None,
            file: Some(bed_text.clone()),
            monetized: false,
        }));
        assert!(
            refused.message.contains("CC-BY-NC") && refused.message.contains("monetized"),
            "the record's licence arms the gate whatever the caller says: {refused}"
        );

        std::fs::write(
            format!("{bed_text}.licence.json"),
            r#"{"model":"stable-audio-open-small","licence":"Stability AI Community License","commercial":true}"#,
        )
        .expect("a commercial record");
        let allowed = err(api.dispatch(Request::BgmImport {
            path: path_text,
            ids: Vec::new(),
            start: None,
            track: None,
            file: Some(bed_text),
            monetized: false,
        }));
        assert!(
            !allowed.message.contains("CC-BY-NC") && !allowed.message.contains("Stability"),
            "a commercial bed passes the licence gate: {allowed}"
        );
    }
}

#[cfg(test)]
mod seek_tests {
    use super::*;
    use crate::message::WindowSought;

    fn api() -> (Api, tempfile::TempDir, Arc<Mutex<Vec<Event>>>) {
        let scratch = tempfile::tempdir().expect("scratch");
        let dirs = AppDirs {
            config: scratch.path().join("config"),
            data: scratch.path().join("data"),
        };
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let api = Api::with_dirs(
            dirs,
            Arc::new(move |event| sink.lock().expect("events").push(event)),
        );
        (api, scratch, events)
    }

    /// A headless API - one with no window behind it - refuses the seek
    /// with the reason, and never pretends a scrub happened.
    #[test]
    fn headless_refuses_the_seek() {
        let (mut api, _scratch, _events) = api();
        let reply = api.dispatch(Request::WindowSeek {
            time: 12.5,
            path: None,
        });
        let Response::Error(error) = reply else {
            panic!("a headless seek must refuse");
        };
        assert!(
            error.message.contains("no window"),
            "the refusal says why: {}",
            error.message
        );
    }

    /// A windowed API seeks through the shared control: the control is
    /// called with the clamped time, and the caller's `path` is handed
    /// through so the window can refuse a project it does not hold. The
    /// capability is named from the moment the control is shared.
    #[test]
    fn a_shared_playhead_seeks() {
        let (mut api, _scratch, _events) = api();
        let sought = std::sync::Arc::new(Mutex::new(Vec::<(Option<String>, f64)>::new()));
        let seen = std::sync::Arc::clone(&sought);
        api.share_playhead(PlayheadControl::new(move |path, time| {
            seen.lock()
                .expect("seen")
                .push((path.map(str::to_owned), time));
            Ok(WindowSought {
                path: "/tmp/Cut".to_owned(),
                time,
            })
        }));
        assert!(
            api.version()
                .capabilities
                .iter()
                .any(|name| name == "window.seek"),
            "sharing a playhead names the capability"
        );
        // A negative time is clamped to zero on the way in: a playhead
        // before the start of the timeline is not a thing to ask a window
        // to show.
        let reply = api.dispatch(Request::WindowSeek {
            time: -3.0,
            path: Some("/tmp/Cut".to_owned()),
        });
        let Response::Result(Reply::WindowSeek(answered)) = reply else {
            panic!("a windowed seek answers");
        };
        assert_eq!(answered.path, "/tmp/Cut");
        assert_eq!(answered.time, 0.0);
        assert_eq!(
            sought.lock().expect("seen").as_slice(),
            [(Some("/tmp/Cut".to_owned()), 0.0)],
            "the control saw the clamped time and the named path"
        );
        // A time that is not a number at all is refused before the window
        // is bothered.
        let reply = api.dispatch(Request::WindowSeek {
            time: f64::NAN,
            path: None,
        });
        assert!(
            matches!(reply, Response::Error(_)),
            "a NaN time is refused, not scrubbed"
        );
        // A window that cannot be reached says so in its own words.
        api.share_playhead(PlayheadControl::new(|_, _| {
            Err("the window closed".to_owned())
        }));
        let reply = api.dispatch(Request::WindowSeek {
            time: 1.0,
            path: None,
        });
        assert!(matches!(reply, Response::Error(_)));
    }
}
