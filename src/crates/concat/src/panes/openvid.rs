// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The library's OpenVid page: 938,000 stock clips, searched locally,
//! fetched one clip at a time.
//!
//! The page owns what is the window's about the library and not the
//! library's own: the current results and where the next page starts, the
//! per-card fetches, and the one-time index download's progress. The
//! search itself, the index and the byte-range trick that makes a clip one
//! ranged read are `concat_host::openvid`'s; see there for how the whole
//! thing stays off the network until a clip is picked.
//!
//! A search runs on a worker and comes back as [`OpenvidMsg::Searched`],
//! as an import comes back as `MediaMsg::Imported`: the window's thread
//! never waits on a query, however quick the local index has made them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use concat_host::AppDirs;
use concat_host::openvid::{self as library, Found, Orientation, Query, Sort, Source};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::OpenvidCardData;

/// How many clips a page of results holds. Forty is two to three screens
/// of rows in a seat of the usual width, and a page is what a search
/// costs; the next page is one More away.
const PAGE: u32 = 40;

/// What to do with a clip once its bytes have arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Then {
    /// Play it through the source preview.
    Preview,
    /// Lay it at the playhead.
    Place,
}

/// Everything that can happen to the page.
#[derive(Clone, Debug)]
pub enum OpenvidMsg {
    /// A library pane turned to the page or away from it.
    Shown(bool),
    /// The search field's words changed, or a filter moved. Every filter
    /// arrives with it, because the page's filters live in the UI global
    /// and this is the one moment they are read.
    Search {
        query: String,
        source: i32,
        shape: i32,
        duration: i32,
        quality: i32,
        camera: i32,
        order: i32,
    },
    /// The search's worker is done. The second field says whether the
    /// page asked for the next page of an existing search or a fresh one.
    Searched(Result<Found, String>, bool),
    /// The next page of what the last search found.
    More,
    /// A card clicked: preview the clip, fetching it first if need be.
    Preview(String),
    /// A card's plus: lay the clip at the playhead, fetching it first.
    Add(String),
    /// "Download the index": the one-time fetch, begun.
    Download,
    /// The download's own tick: the pane asks the library where it stands
    /// and publishes, four times a second, until it is done.
    Tick,
    /// A fetch is done, and what its bytes were for.
    Fetched(String, Result<PathBuf, String>, Then),
}

/// The page's state.
pub struct OpenvidPane {
    /// A library pane is showing the page.
    pub shown: bool,
    /// The library as the pane last read it: what the status card shows.
    pub state: library::Status,
    /// The clips on the page, in the order the sort put them.
    pub results: Vec<library::Clip>,
    /// How many clips match in all, this page aside.
    pub total: u64,
    /// A search or its next page is running.
    pub searching: bool,
    /// The last search, as it was asked: what More asks more of.
    last: Option<Query>,
    /// Fetches running, by card id: what the bytes are for.
    fetching: HashMap<String, Then>,
    /// The camera motions the index lists, for the filter's dropdown.
    pub cameras: Vec<String>,
    /// Stops the index download when the window goes.
    cancel: Arc<AtomicBool>,
    /// The app's own folders, for the library's own.
    dirs: Option<AppDirs>,
    /// The download's progress poll, running only while one runs.
    timer: slint::Timer,
}

impl OpenvidPane {
    /// The pane, with the library's state as it stands on this machine.
    pub fn open(dirs: &AppDirs) -> OpenvidPane {
        let pane = OpenvidPane {
            shown: false,
            state: library::shared(dirs).status(),
            results: Vec::new(),
            total: 0,
            searching: false,
            last: None,
            fetching: HashMap::new(),
            cameras: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: Some(dirs.clone()),
            timer: slint::Timer::default(),
        };
        pane
    }

    /// The library this pane reads, when it has folders to read from.
    fn library(&self) -> Option<&'static library::Library> {
        self.dirs.as_ref().map(library::shared)
    }

    /// How many clips the library holds, formatted with its thousands
    /// separated for the page's footnote.
    pub fn library_rows(&self) -> Option<u64> {
        self.library().and_then(|library| library.rows())
    }

    /// How many clips' frame sizes have been measured so far.
    pub fn state_measured(&self) -> u64 {
        self.library().map_or(0, |library| library.measured())
    }

    /// Applies one message. The studio is the rest of the window; while
    /// this runs the studio's copy of the pane is a blank it must not read.
    pub fn update(&mut self, msg: OpenvidMsg, studio: &mut Studio) {
        match msg {
            OpenvidMsg::Shown(on) => {
                self.shown = on;
                if !on {
                    self.timer.stop();
                    return;
                }
                self.refresh();
            }
            OpenvidMsg::Search {
                query,
                source,
                shape,
                duration,
                quality,
                camera,
                order,
            } => {
                let search = Query {
                    text: (!query.trim().is_empty()).then(|| query.clone()),
                    source: match source {
                        1 => Some(Source::Pixabay),
                        2 => Some(Source::Pexels),
                        3 => Some(Source::CelebV),
                        _ => None,
                    },
                    orientation: match shape {
                        1 => Some(Orientation::Square),
                        2 => Some(Orientation::Vertical),
                        3 => Some(Orientation::Horizontal),
                        _ => None,
                    },
                    min_aesthetic: match quality {
                        1 => Some(4.0),
                        2 => Some(4.5),
                        3 => Some(5.0),
                        4 => Some(5.2),
                        5 => Some(5.4),
                        _ => None,
                    },
                    min_seconds: match duration {
                        2 => Some(5.0),
                        3 => Some(15.0),
                        4 => Some(30.0),
                        _ => None,
                    },
                    max_seconds: match duration {
                        1 => Some(5.0),
                        2 => Some(15.0),
                        3 => Some(30.0),
                        _ => None,
                    },
                    camera: camera
                        .checked_sub(1)
                        .and_then(|index| self.cameras.get(index as usize))
                        .cloned(),
                    sort: match order {
                        1 => Sort::Aesthetic,
                        2 => Sort::Motion,
                        3 => Sort::Shortest,
                        4 => Sort::Longest,
                        5 => Sort::Surprise,
                        _ => Sort::Relevance,
                    },
                    ..Query::default()
                };
                let mut search = search;
                search.limit = PAGE;
                self.last = Some(search.clone());
                self.search(search, false);
            }
            OpenvidMsg::More => {
                let Some(mut search) = self.last.clone() else {
                    return;
                };
                search.offset += search.limit;
                self.search(search, true);
            }
            OpenvidMsg::Searched(result, appended) => {
                self.searching = false;
                match result {
                    Ok(found) => {
                        self.total = found.total;
                        if appended {
                            self.results.extend(found.clips);
                        } else {
                            self.results = found.clips;
                        }
                    }
                    Err(error) => studio.notify(&error, true),
                }
            }
            OpenvidMsg::Preview(id) => self.take(id, Then::Preview, studio),
            OpenvidMsg::Add(id) => self.take(id, Then::Place, studio),
            OpenvidMsg::Download => {
                let Some(dirs) = self.dirs.clone() else {
                    return;
                };
                if matches!(self.state, library::Status::Downloading(_)) {
                    return;
                }
                self.state = library::Status::Downloading(0);
                // The poll: the download ticks the library's own counter,
                // and the page reads it four times a second until it ends.
                self.timer.start(
                    slint::TimerMode::Repeated,
                    std::time::Duration::from_millis(250),
                    || {
                        crate::host::Shell::with(|shell, _| {
                            let mut studio = shell.studio.borrow_mut();
                            studio.handle(Msg::Openvid(OpenvidMsg::Tick));
                        });
                    },
                );
                let cancel = Arc::clone(&self.cancel);
                crate::host::spawn(
                    move || library::shared(&dirs).prepare(&cancel),
                    |studio, _, _, result| {
                        // The verdict before the words: the words consume
                        // the result, and a half-moved value cannot be
                        // asked what it was.
                        let failed = result.is_err();
                        let words = match result {
                            Ok(()) => t("openvid.indexReady"),
                            Err(error) => format!("{}: {error}", t("openvid.downloadFailed")),
                        };
                        studio.notify(&words, failed);
                    },
                );
            }
            OpenvidMsg::Tick => {
                let Some(library) = self.library() else {
                    self.timer.stop();
                    return;
                };
                self.state = library.status();
                match self.state {
                    library::Status::Ready | library::Status::Failed(_) => {
                        self.timer.stop();
                        self.refresh();
                    }
                    _ => {}
                }
            }
            OpenvidMsg::Fetched(id, result, then) => {
                self.fetching.remove(&id);
                let Some(library) = self.library() else {
                    return;
                };
                let Ok(id_number) = id.parse::<i64>() else {
                    return;
                };
                // The frame size is free now that the bytes are local, and
                // the card that asked is redrawn with it.
                let _ = library.dims(id_number);
                let name = library
                    .clip(id_number)
                    .ok()
                    .flatten()
                    .map(|clip| clip.name)
                    .unwrap_or_default();
                match result {
                    Ok(path) => match then {
                        Then::Preview => studio.preview_file(&id, &name, &path),
                        Then::Place => studio.place_openvid(&name, &id, &path),
                    },
                    Err(error) if error == "cancelled" => {}
                    Err(error) => studio.notify(&tf("openvid.fetchFailed", &[&name, &error]), true),
                }
            }
        }
    }

    /// A fresh read of the library's state, and the first search when the
    /// page has nothing to show yet. Called when the page comes up and
    /// when its download ends; cheap, and off the window's thread by
    /// being only a status read.
    fn refresh(&mut self) {
        let Some(library) = self.library() else {
            return;
        };
        self.state = library.status();
        if let library::Status::Ready = self.state {
            if self.cameras.is_empty() {
                self.cameras = library.cameras();
            }
            if self.results.is_empty() && !self.searching && self.last.is_none() {
                self.search(
                    Query {
                        limit: PAGE,
                        ..Query::default()
                    },
                    false,
                );
            }
        }
    }

    /// Runs a search on a worker, with the page's own mark that one is
    /// running so the field can hold its breath.
    fn search(&mut self, query: Query, appended: bool) {
        let Some(library) = self.library() else {
            return;
        };
        if !matches!(self.state, library::Status::Ready) {
            return;
        }
        self.searching = true;
        crate::host::spawn(
            move || library.search(&query),
            move |studio, _, _, result| {
                studio.handle(Msg::Openvid(OpenvidMsg::Searched(result, appended)));
            },
        );
    }

    /// A card's verb: the clip's bytes if they are not on this machine
    /// yet, then [`OpenvidMsg::Fetched`] with what they were for. A clip
    /// already on disk goes straight to its verb.
    fn take(&mut self, id: String, then: Then, studio: &mut Studio) {
        let Some(library) = self.library() else {
            return;
        };
        let Ok(number) = id.parse::<i64>() else {
            return;
        };
        if let Some(path) = library.fetched(number) {
            let _ = library.dims(number);
            let name = library
                .clip(number)
                .ok()
                .flatten()
                .map(|clip| clip.name)
                .unwrap_or_default();
            match then {
                Then::Preview => studio.preview_file(&id, &name, &path),
                Then::Place => studio.place_openvid(&name, &id, &path),
            }
            return;
        }
        if self.fetching.contains_key(&id) {
            return;
        }
        self.fetching.insert(id.clone(), then);
        let cancel = AtomicBool::new(false);
        crate::host::spawn(
            move || library.fetch(number, &cancel),
            move |studio, _, _, result| {
                studio.handle(Msg::Openvid(OpenvidMsg::Fetched(id, result, then)));
            },
        );
    }

    /// The cards, as Slint shows them. Everything display-ready: the
    /// numbers arrive as words because a card is a picture of a clip.
    pub fn rows(&self) -> Vec<OpenvidCardData> {
        self.results
            .iter()
            .map(|clip| {
                let shape = clip
                    .dims
                    .map_or_else(|| "…".to_owned(), |(w, h)| format!("{w}×{h}"));
                OpenvidCardData {
                    id: clip.id.to_string().into(),
                    name: clip.name.as_str().into(),
                    caption: clip.caption.as_str().into(),
                    meta: format!(
                        "{:.1}s · {:.1} MB · {} · {}",
                        clip.seconds,
                        clip.bytes as f64 / 1e6,
                        shape,
                        clip.camera,
                    )
                    .into(),
                    quality: format!("{:.2}", clip.aesthetic).into(),
                    ready: self
                        .library()
                        .is_some_and(|library| library.fetched(clip.id).is_some()),
                    busy: self.fetching.contains_key(&clip.id.to_string()),
                }
            })
            .collect()
    }
}

impl Default for OpenvidPane {
    fn default() -> Self {
        OpenvidPane {
            shown: false,
            state: library::Status::Missing,
            results: Vec::new(),
            total: 0,
            searching: false,
            last: None,
            fetching: HashMap::new(),
            cameras: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: None,
            timer: slint::Timer::default(),
        }
    }
}

impl Studio {
    /// Lays an OpenVid clip on the timeline: copied into the project's
    /// own `openvid` folder and imported into the bin first - once; a
    /// second use finds the copy already there - then cut onto the first
    /// lane with room at the playhead. Selects what it made, as laying a
    /// sound does.
    pub fn place_openvid(&mut self, name: &str, id: &str, path: &PathBuf) {
        let media_id = match self.adopt_openvid(name, id, path) {
            Ok(id) => id,
            Err(error) => {
                self.notify(&error, true);
                return;
            }
        };
        self.close_source();
        let start = f64::from(self.playhead.max(0.0));
        let created = self.apply(concat_project::Command::AddClipAtFirstFree { media_id, start });
        if let Some(id) = created {
            self.selection = vec![id];
        }
    }

    /// The bin's id for an OpenVid clip in this project: its copy in the
    /// project's `openvid` folder, made and imported when it is not there
    /// yet. The dataset's id travels in the name - `{name}-{id}.mp4` - so
    /// the same clip is always the same file and two different clips
    /// never share one.
    fn adopt_openvid(&mut self, name: &str, id: &str, path: &PathBuf) -> Result<String, String> {
        let project = self
            .session
            .as_ref()
            .map(|session| PathBuf::from(session.path()))
            .ok_or_else(|| t("openvid.openProjectFirst"))?;
        let folder = project.join("openvid");
        let size = std::fs::metadata(path)
            .map_err(|error| format!("{}: {error}", path.display()))?
            .len();
        let target = folder.join(format!("{name}-{id}.mp4"));
        let target_text = target.to_string_lossy().into_owned();
        if let Some(item) = self
            .project()
            .media
            .iter()
            .find(|item| item.path == target_text)
        {
            return Ok(item.id.clone());
        }
        // A file of that name that is not this clip - the name carries
        // the dataset's own id, so this cannot happen, but a hand-made
        // file could sit there - is left alone and the copy takes a
        // numbered name, as a sound's does.
        let mut target = target;
        let mut n = 2;
        while target.exists()
            && std::fs::metadata(&target).map(|meta| meta.len()).ok() != Some(size)
        {
            target = folder.join(format!("{name} {n}-{id}.mp4"));
            n += 1;
        }
        let target_text = target.to_string_lossy().into_owned();
        if let Some(item) = self
            .project()
            .media
            .iter()
            .find(|item| item.path == target_text)
        {
            return Ok(item.id.clone());
        }
        if !target.exists() {
            std::fs::create_dir_all(&folder)
                .and_then(|()| std::fs::copy(path, &target).map(drop))
                .map_err(|error| tf("openvid.couldNotCopy", &[&name, &error]))?;
        }
        let summary = concat_host::media::probe(&target_text)?;
        let created = self.apply(concat_project::Command::AddMedia {
            item: summary.to_new_media(),
        });
        created
            .or_else(|| {
                self.project()
                    .media
                    .iter()
                    .find(|item| item.path == target_text)
                    .map(|item| item.id.clone())
            })
            .ok_or_else(|| tf("openvid.couldNotCopy", &[&name, &"?"]))
    }
}
