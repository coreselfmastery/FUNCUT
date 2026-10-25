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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use base64::Engine as _;
use concat_effects::Catalogue;
use concat_effects::manifest::Kind;
use concat_export::{ExportClip, ExportRequest};
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
use concat_host::templates::{self, SlotFill};
use concat_host::{ProjectInfo, Session, Titles, lock, media, projects};
use concat_project::Command;
use concat_project::commands::TrimEdge;
use concat_project::model::VideoSettings;

pub use message::{
    API_VERSION, ApiError, BgmFound, BgmImport, BgmState, BgmTrack, Dirs, Done, ErrorCode, Event,
    ExportSpec, Fill, JobState, JobStatus, MagickAdd, MagickEntry, MagickFound, MagickState,
    OpenvidClip, OpenvidFound, OpenvidImport, OpenvidState, PackageInfo, ParamInfo, Picture, Reply,
    Request, Response, SoundEntry, SoundsAdded, SoundsFound, Started, VersionInfo, Written,
};

/// The export sheet's middle quality, and what an export gets when the
/// caller says nothing.
const DEFAULT_CRF: u8 = 20;
/// The x264 preset every export uses unless told otherwise.
const DEFAULT_PRESET: &str = "medium";

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
    /// Where this API may write: a created project, an instantiated
    /// template, an export or a preview file lands under one of these.
    /// Empty means anywhere, for the process's own owner at a terminal.
    roots: Vec<PathBuf>,
    /// Which project folders are open on this machine, across this API
    /// and a window it is embedded in.
    open: OpenProjects,
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

impl Stopper {
    /// Asks every job this API started to stop at its next frame - its
    /// own and no other, even where the export slot is shared with a
    /// window. Each then ends with [`ErrorCode::Cancelled`]. How many
    /// were running.
    pub fn cancel_all(&self) -> usize {
        let running = self.book.cancel_all();
        if running > 0 {
            // A job still finding masks is in the analysis, not the
            // render; the analysis is this API's own.
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
            titles: Titles::new(&dirs),
            cutouts: Arc::new(Cutouts::new(&dirs.data)),
            stabilizers: Arc::new(Stabilizers::new()),
            monitor: Monitor::new(),
            exporter: Exporter::new(),
            sessions: BTreeMap::new(),
            roots: Vec::new(),
            open: OpenProjects::default(),
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

    /// The export slot, for an embedder that shares it with a window.
    pub fn exporter(&self) -> Exporter {
        self.exporter.clone()
    }

    /// Takes an embedder's export slot in place of its own, so "one export
    /// at a time" holds across a window and this API together.
    pub fn share_exporter(&mut self, exporter: Exporter) {
        self.exporter = exporter;
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
            Request::ProjectDocument { path } => {
                Ok(Reply::Document(self.session(&path)?.document()))
            }
            Request::ProjectSave { path, name } => {
                self.save(&path, name.as_deref())?;
                Ok(Reply::Done(Done {}))
            }
            Request::ProjectSetVideo { path, video } => {
                view(self.session_mut(&path)?.set_video(video).map_err(refused)?)
            }
            Request::EditApply { path, command } => view(self.apply(&path, *command)?),
            Request::EditUndo { path } => view(self.session_mut(&path)?.undo()),
            Request::EditRedo { path } => view(self.session_mut(&path)?.redo()),
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
            )?)),
            Request::OpenvidGet { ids } => Ok(Reply::OpenvidClips(self.openvid_get(&ids)?)),
            Request::OpenvidFetch { id } => Ok(Reply::OpenvidClips(self.openvid_fetch(id)?)),
            Request::OpenvidImport {
                path,
                ids,
                start,
                track,
            } => Ok(Reply::OpenvidImported(
                self.openvid_import(&path, &ids, start, track)?,
            )),
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
            } => Ok(Reply::SoundsAdded(
                self.sounds_add(&path, &id, start, track)?,
            )),
            Request::BgmStatus => Ok(Reply::Bgm(self.bgm_state())),
            Request::BgmPrepare => {
                self.bgm_prepare();
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
            } => Ok(Reply::BgmImported(
                self.bgm_import(&path, &ids, start, track)?,
            )),
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
            Request::ExportStatus { job } => Ok(Reply::Job(self.job_status(&job)?)),
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
            } => Ok(Reply::Started(
                self.speech_transcribe(&path, model, language)?,
            )),
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
            } => Ok(Reply::Started(
                self.speech_generate(&project, &text, voice, speed, model)?,
            )),
            #[cfg(not(feature = "speech"))]
            Request::SpeechGenerate { .. } => Err(ApiError::new(
                ErrorCode::Refused,
                "this build does not speak: it was built without the speech feature",
            )),
        }
    }

    /// [`Request::Version`].
    pub fn version(&self) -> VersionInfo {
        VersionInfo {
            api_version: API_VERSION.to_owned(),
            concat: env!("CARGO_PKG_VERSION").to_owned(),
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
        self.session_mut(path)?.apply(command).map_err(refused)
    }

    /// [`Request::MediaImport`]: the probe and the add, as one.
    pub fn import(&mut self, path: &str, file: &str) -> Result<EditorView, ApiError> {
        let item = media::probe(file).map_err(ApiError::failed)?.to_new_media();
        self.apply(path, Command::AddMedia { item })
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

    /// [`Request::OpenvidFetch`].
    pub fn openvid_fetch(&self, id: i64) -> Result<Vec<OpenvidClip>, ApiError> {
        self.openvid_ready()?;
        let library = self.library();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let path = library.fetch(id, &cancel).map_err(ApiError::failed)?;
        let Some(clip) = library.clip(id).map_err(ApiError::failed)? else {
            return Err(ApiError::failed(format!("there is no clip {id}")));
        };
        Ok(vec![OpenvidClip::of(&clip, clip.dims, Some(&path))])
    }

    /// [`Request::OpenvidImport`]: fetch, copy into the project's own
    /// folder, and add to the bin - one undoable step, or two when the
    /// clips are also laid on a lane.
    pub fn openvid_import(
        &mut self,
        path: &str,
        ids: &[i64],
        start: Option<f64>,
        track: Option<String>,
    ) -> Result<OpenvidImport, ApiError> {
        if ids.is_empty() {
            return Err(ApiError::failed("no clip ids given".to_owned()));
        }
        if ids.len() > 20 {
            return Err(ApiError::failed(
                "at most 20 clips a call; page through them".to_owned(),
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
        let cancel = std::sync::atomic::AtomicBool::new(false);
        // Everything is fetched and copied before anything is added, so a
        // clip that will not arrive refuses the whole request rather than
        // leaving half a set behind.
        let mut copies = Vec::new();
        for id in ids {
            let cached = library.fetch(*id, &cancel).map_err(ApiError::failed)?;
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

    /// [`Request::SoundsAdd`]: download into the cache when it is not
    /// there yet, copy into the project's own `audio` folder, and add to
    /// the bin - laying it on a lane too when `start` is given, as one
    /// undoable step.
    pub fn sounds_add(
        &mut self,
        path: &str,
        id: &str,
        start: Option<f64>,
        track: Option<String>,
    ) -> Result<SoundsAdded, ApiError> {
        let sound = self
            .sounds_catalogue()?
            .get(id)
            .cloned()
            .ok_or_else(|| ApiError::failed(format!("there is no sound {id}")))?;
        self.writable(path)?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let cached = sounds::fetch_sound(&self.dirs, &sound, &cancel, &mut |_, _| {})
            .map_err(ApiError::failed)?;
        let folder = {
            let session = self.session(path)?;
            let folder = PathBuf::from(session.path()).join("audio");
            std::fs::create_dir_all(&folder).map_err(|error| {
                ApiError::failed(format!("could not create {}: {error}", folder.display()))
            })?;
            folder
        };
        // The library's own file name: unique for ever, and the same sound
        // imported twice is the same file, copied over itself.
        let into = folder.join(&sound.file);
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
                "the OpenVid index is still downloading ({percent}%); ask openvid.status \
                 until it is ready"
            ))),
            openvid::Status::Missing => Err(ApiError::failed(
                "the OpenVid index is not on this machine yet; call openvid.prepare once, \
                 then ask openvid.status until it is ready"
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
    /// one is the same download.
    pub fn bgm_prepare(&self) {
        let library = self.music();
        for source in [bgm_library::Source::FreePd, bgm_library::Source::Lofi] {
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
    pub fn bgm_import(
        &mut self,
        path: &str,
        ids: &[String],
        start: Option<f64>,
        track: Option<String>,
    ) -> Result<BgmImport, ApiError> {
        if ids.is_empty() {
            return Err(ApiError::failed("no track ids given".to_owned()));
        }
        if ids.len() > 20 {
            return Err(ApiError::failed(
                "at most 20 tracks a call; page through them".to_owned(),
            ));
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
        for id in ids {
            let mut progress = |_, _| {};
            let cached = library
                .fetch(id, &cancel, &mut progress)
                .map_err(ApiError::failed)?;
            let Some(found) = library.track(id).map_err(ApiError::failed)? else {
                return Err(ApiError::failed(format!("there is no track {id}")));
            };
            // The title plus the id: unique for as long as the track is in
            // the catalog, and the same track imported twice is the same
            // file, copied over itself.
            let name = format!("{}-{}.mp3", clean_name(&found.title), &id[id.len() - 6..]);
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

    /// [`Request::MagickStatus`]: both shelves, each in its own word.
    pub fn magick_state(&self) -> MagickState {
        let library = self.stickers();
        let word = |source: sticker_library::Source| match library.status(source) {
            sticker_library::Status::Missing => "missing".to_owned(),
            sticker_library::Status::Downloading(percent) => format!("downloading ({percent}%)"),
            sticker_library::Status::Ready => "ready".to_owned(),
            sticker_library::Status::Failed(error) => format!("failed: {error}"),
        };
        let magick = word(sticker_library::Source::Magick);
        let openmoji = word(sticker_library::Source::Openmoji);
        let percent = [
            sticker_library::Source::Magick,
            sticker_library::Source::Openmoji,
        ]
        .into_iter()
        .filter_map(|source| match library.status(source) {
            sticker_library::Status::Downloading(percent) => Some(percent),
            _ => None,
        })
        .max()
        .unwrap_or(0);
        let error = [
            (sticker_library::Source::Magick, &magick),
            (sticker_library::Source::Openmoji, &openmoji),
        ]
        .into_iter()
        .filter(|(_, word)| word.starts_with("failed"))
        .map(|(source, word)| format!("{}: {}", source.name(), word.trim_start_matches("failed: ")))
        .reduce(|both, one| format!("{both}; {one}"));
        MagickState {
            magick,
            openmoji,
            percent,
            magick_rows: library.rows(sticker_library::Source::Magick),
            openmoji_rows: library.rows(sticker_library::Source::Openmoji),
            error,
        }
    }

    /// [`Request::MagickPrepare`]: both shelves' one-time index
    /// downloads, started on their own threads because they are a few
    /// MB and a parse follows them; a caller should not wait for them.
    /// Their progress is what `magick.status` reports; a second call
    /// while they run is a no-op, and the window's own page starting one
    /// is the same download.
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
                    "the source must be magick or openmoji, not whatever was given",
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
        templates::save(
            &self.dirs.config,
            &session.document(),
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
        // Whichever phase it is in: the analysis ahead of the render, or
        // the render itself.
        self.cutouts.cancel();
        self.stabilizers.cancel();
        self.exporter.cancel();
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
    /// frame, to a file.
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

    /// [`Request::PreviewFrame`] without one: the same frame, inline.
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
                    proxy: false,
                    color_space: session.video().color_space,
                },
            )
            .map_err(ApiError::failed)?;
        Ok((width, height, pixels))
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

fn key_of(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|canonical| canonical.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_owned())
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

    #[test]
    fn version_names_what_the_build_serves() {
        let (mut api, _scratch, _events) = api();
        let base = api.version();
        assert_eq!(base.api_version, API_VERSION);
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
        })));
        let media_id = added.created_id.expect("minted");
        let placed = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id,
                start: 2.0,
                above: false,
            }),
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
        })));
        let placed = view(ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id: added.created_id.expect("minted"),
                start: 0.0,
                above: false,
            }),
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
        }));
        assert_eq!(refused.code, ErrorCode::Refused);
        assert_eq!(refused.message, "That media is no longer in the bin.");
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
        assert_eq!(state.openmoji, "missing");
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
        })));
        ok(api.dispatch(Request::EditApply {
            path: path.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id: added.created_id.expect("minted"),
                start: 0.0,
                above: false,
            }),
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
            job: "j1".to_owned(),
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
            job: "j9".to_owned(),
        }));
        assert_eq!(unknown.code, ErrorCode::NotFound);
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
}
