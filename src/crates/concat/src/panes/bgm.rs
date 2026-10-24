// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The library's Music page: 1,191 whole CC0 background tracks, searched
//! locally, fetched one track at a time.
//!
//! Two shelves under one page: FreePD - the Internet Archive's mirror of
//! Kevin MacLeod's CC0 library - and Lo-Fi - Moby's MobyGratis
//! instrumentals, as the OpenLo-Fi pack. Neither is in the repository or
//! the app: a shelf's list arrives once, a few MB, and a track's own
//! bytes move only when the track is picked - a click previews one, a
//! plus lays one at the playhead. The Lo-Fi shelf's zip is never
//! downloaded at all; each of its tracks is one ranged read of exactly
//! that track's span. The search, the catalogs and the byte-range trick
//! are `concat_host::bgm`'s; see there for how the whole thing stays off
//! the network until a track is picked.
//!
//! A search runs on a worker and comes back as [`MusicMsg::Searched`],
//! as a fetch comes back as `MusicMsg::Fetched`: the window's thread
//! never waits on a query, however quick the local catalogs have made
//! them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use concat_host::AppDirs;
use concat_host::bgm::{self as library, Found, Query, Sort, Source};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::MusicCardData;

/// How many tracks a page of results holds. Forty is two to three screens
/// of rows in a seat of the usual width, and a page is what a search
/// costs; the next page is one More away.
const PAGE: u32 = 40;

/// What to do with a track once its bytes have arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Then {
    /// Play it through the source preview.
    Preview,
    /// Lay it at the playhead.
    Place,
}

/// Everything that can happen to the page.
#[derive(Clone, Debug)]
pub enum MusicMsg {
    /// A library pane turned to the page or away from it.
    Shown(bool),
    /// The search field's words changed, or a filter moved. Every filter
    /// arrives with it, because the page's filters live in the UI global
    /// and this is the one moment they are read.
    Search {
        query: String,
        source: i32,
        category: i32,
        order: i32,
    },
    /// The search's worker is done. The second field says whether the
    /// page asked for the next page of an existing search or a fresh one.
    Searched(Result<Found, String>, bool),
    /// The next page of what the last search found.
    More,
    /// A card clicked: preview the track, fetching it first if need be.
    Preview(String),
    /// A card's plus: lay the track at the playhead, fetching it first.
    Add(String),
    /// "Load the music library": the one-time catalog fetches, begun.
    Download,
    /// The loads' own tick: the pane asks each shelf where it stands and
    /// publishes, four times a second, until they are done.
    Tick,
    /// A fetch is done, and what its bytes were for.
    Fetched(String, Result<PathBuf, String>, Then),
}

/// The page's state.
pub struct MusicPane {
    /// A library pane is showing the page.
    pub shown: bool,
    /// The shelves as the pane last read them.
    pub freepd: library::Status,
    pub lofi: library::Status,
    /// The tracks on the page, in the order the sort put them.
    pub results: Vec<library::Track>,
    /// How many tracks match in all, this page aside.
    pub total: u64,
    /// A search or its next page is running.
    pub searching: bool,
    /// The last search, as it was asked: what More asks more of.
    last: Option<Query>,
    /// Fetches running, by card id: what the bytes are for.
    fetching: HashMap<String, Then>,
    /// Both shelves' own groupings, as the category filter lists them
    /// after "All": the union of the two shelves' own spellings, shown
    /// with their first letter standing up.
    pub categories: Vec<(String, String)>,
    /// Stops the loads when the window goes.
    cancel: Arc<AtomicBool>,
    /// The app's own folders, for the library's own.
    dirs: Option<AppDirs>,
    /// The loads' progress poll, running only while one runs.
    timer: slint::Timer,
}

impl MusicPane {
    /// The pane, with the shelves as they stand on this machine. A
    /// catalog on disk from an earlier run is taken in here, so the page
    /// is ready from the first frame and nothing is fetched until its
    /// Load button or a Refresh asks for it.
    pub fn open(dirs: &AppDirs) -> MusicPane {
        MusicPane {
            shown: false,
            freepd: library::shared(dirs).status(Source::FreePd),
            lofi: library::shared(dirs).status(Source::Lofi),
            results: Vec::new(),
            total: 0,
            searching: false,
            last: None,
            fetching: HashMap::new(),
            categories: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: Some(dirs.clone()),
            timer: slint::Timer::default(),
        }
    }

    /// The library this pane reads, when it has folders to read from.
    fn library(&self) -> Option<&'static library::Library> {
        self.dirs.as_ref().map(library::shared)
    }

    /// Both shelves' own groupings, as the filter lists them: the
    /// category's own spelling for the search, and the same word with its
    /// first letter standing up for the row.
    fn read_categories(&mut self) {
        self.categories = self
            .library()
            .map(|library| {
                let mut seen: Vec<String> = library
                    .categories(Source::FreePd)
                    .into_iter()
                    .chain(library.categories(Source::Lofi))
                    .collect();
                seen.sort();
                seen.dedup();
                seen.into_iter()
                    .map(|category| {
                        let shown = {
                            let mut chars = category.chars();
                            match chars.next() {
                                Some(first) => {
                                    first.to_uppercase().collect::<String>() + chars.as_str()
                                }
                                None => String::new(),
                            }
                        };
                        (category, shown)
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    /// How many tracks a shelf holds, once ready.
    pub fn shelf_rows(&self, source: Source) -> Option<u64> {
        self.library().and_then(|library| library.rows(source))
    }

    /// The page's one word for the shelves, as the status card takes it:
    /// 0 both missing, 1 a load running, 2 a shelf ready, 3 both failed.
    pub fn combined(&self) -> i32 {
        let ready = |status: &library::Status| matches!(status, library::Status::Ready);
        let loading = |status: &library::Status| matches!(status, library::Status::Loading(_));
        let failed = |status: &library::Status| matches!(status, library::Status::Failed(_));
        if loading(&self.freepd) || loading(&self.lofi) {
            1
        } else if ready(&self.freepd) || ready(&self.lofi) {
            2
        } else if failed(&self.freepd) && failed(&self.lofi) {
            3
        } else {
            0
        }
    }

    /// Applies one message. The studio is the rest of the window; while
    /// this runs the studio's copy of the pane is a blank it must not read.
    pub fn update(&mut self, msg: MusicMsg, studio: &mut Studio) {
        match msg {
            MusicMsg::Shown(on) => {
                self.shown = on;
                if !on {
                    self.timer.stop();
                    return;
                }
                self.refresh();
            }
            MusicMsg::Search {
                query,
                source,
                category,
                order,
            } => {
                let search = Query {
                    text: (!query.trim().is_empty()).then(|| query.clone()),
                    source: match source {
                        1 => Some(Source::FreePd),
                        2 => Some(Source::Lofi),
                        _ => None,
                    },
                    category: category
                        .checked_sub(1)
                        .and_then(|index| self.categories.get(index as usize))
                        .map(|(category, _)| category.clone()),
                    sort: match order {
                        1 => Sort::Shortest,
                        2 => Sort::Longest,
                        3 => Sort::Surprise,
                        _ => Sort::Relevance,
                    },
                    ..Query::default()
                };
                let mut search = search;
                search.limit = PAGE;
                self.last = Some(search.clone());
                self.search(search, false);
            }
            MusicMsg::More => {
                let Some(mut search) = self.last.clone() else {
                    return;
                };
                search.offset += search.limit;
                self.search(search, true);
            }
            MusicMsg::Searched(result, appended) => {
                self.searching = false;
                match result {
                    Ok(found) => {
                        self.total = found.total;
                        if appended {
                            self.results.extend(found.tracks);
                        } else {
                            self.results = found.tracks;
                        }
                    }
                    Err(error) => studio.notify(&error, true),
                }
            }
            MusicMsg::Preview(id) => self.take(id, Then::Preview, studio),
            MusicMsg::Add(id) => self.take(id, Then::Place, studio),
            MusicMsg::Download => {
                let Some(dirs) = self.dirs.clone() else {
                    return;
                };
                if matches!(self.freepd, library::Status::Loading(_))
                    || matches!(self.lofi, library::Status::Loading(_))
                {
                    return;
                }
                self.freepd = library::Status::Loading(0);
                self.lofi = library::Status::Loading(0);
                // The poll: the loads tick the shelves' own counters, and
                // the page reads them four times a second until they end.
                self.timer.start(
                    slint::TimerMode::Repeated,
                    std::time::Duration::from_millis(250),
                    || {
                        crate::host::Shell::with(|shell, _| {
                            let mut studio = shell.studio.borrow_mut();
                            studio.handle(Msg::Music(MusicMsg::Tick));
                        });
                    },
                );
                let cancel = Arc::clone(&self.cancel);
                for source in [Source::FreePd, Source::Lofi] {
                    // Each spawn takes its own copy: the loop's second
                    // iteration cannot borrow what the first moved.
                    let dirs = dirs.clone();
                    let cancel = Arc::clone(&cancel);
                    crate::host::spawn(
                        move || library::shared(&dirs).prepare(source, &cancel),
                        move |studio, _, _, result| {
                            // The verdict before the words: the words
                            // consume the result, and a half-moved value
                            // cannot be asked what it was.
                            let failed = result.is_err();
                            let words = match result {
                                Ok(()) => match source {
                                    Source::FreePd => t("bgm.freepdReady"),
                                    Source::Lofi => t("bgm.lofiReady"),
                                },
                                Err(error) => format!(
                                    "{}: {error}",
                                    match source {
                                        Source::FreePd => t("bgm.freepd"),
                                        Source::Lofi => t("bgm.lofi"),
                                    }
                                ),
                            };
                            studio.notify(&words, failed);
                        },
                    );
                }
            }
            MusicMsg::Tick => {
                let Some(library) = self.library() else {
                    self.timer.stop();
                    return;
                };
                self.freepd = library.status(Source::FreePd);
                self.lofi = library.status(Source::Lofi);
                let loading = matches!(self.freepd, library::Status::Loading(_))
                    || matches!(self.lofi, library::Status::Loading(_));
                if !loading {
                    self.timer.stop();
                    self.refresh();
                }
            }
            MusicMsg::Fetched(id, result, then) => {
                self.fetching.remove(&id);
                let Some(library) = self.library() else {
                    return;
                };
                let name = library
                    .track(&id)
                    .ok()
                    .flatten()
                    .map(|track| track.title)
                    .unwrap_or_default();
                match result {
                    Ok(path) => match then {
                        Then::Preview => studio.preview_file(&id, &name, &path),
                        Then::Place => studio.place_bgm(&name, &id, &path),
                    },
                    Err(error) if error == "cancelled" => {}
                    Err(error) => studio.notify(&tf("bgm.fetchFailed", &[&name, &error]), true),
                }
            }
        }
    }

    /// A fresh read of the shelves' state, and the first search when the
    /// page has nothing to show yet. Called when the page comes up and
    /// when its loads end; cheap, and off the window's thread by being
    /// only a status read.
    fn refresh(&mut self) {
        let Some(library) = self.library() else {
            return;
        };
        self.freepd = library.status(Source::FreePd);
        self.lofi = library.status(Source::Lofi);
        if self.combined() == 2 {
            if self.categories.is_empty() {
                self.read_categories();
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
        if self.combined() != 2 {
            return;
        }
        self.searching = true;
        crate::host::spawn(
            move || library.search(&query),
            move |studio, _, _, result| {
                studio.handle(Msg::Music(MusicMsg::Searched(result, appended)));
            },
        );
    }

    /// A card's verb: the track's bytes if they are not on this machine
    /// yet, then [`MusicMsg::Fetched`] with what they were for. A track
    /// already on disk goes straight to its verb.
    fn take(&mut self, id: String, then: Then, studio: &mut Studio) {
        let Some(library) = self.library() else {
            return;
        };
        if let Some(path) = library.fetched(&id) {
            let name = library
                .track(&id)
                .ok()
                .flatten()
                .map(|track| track.title)
                .unwrap_or_default();
            match then {
                Then::Preview => studio.preview_file(&id, &name, &path),
                Then::Place => studio.place_bgm(&name, &id, &path),
            }
            return;
        }
        if self.fetching.contains_key(&id) {
            return;
        }
        self.fetching.insert(id.clone(), then);
        let cancel = AtomicBool::new(false);
        // The worker takes its own copy of the id: the completion's copy
        // is the one the message carries.
        let fetching = id.clone();
        crate::host::spawn(
            move || {
                let mut progress = |_, _| {};
                library.fetch(&fetching, &cancel, &mut progress)
            },
            move |studio, _, _, result| {
                studio.handle(Msg::Music(MusicMsg::Fetched(id, result, then)));
            },
        );
    }

    /// The cards, as Slint shows them. Everything display-ready: the
    /// numbers arrive as words because a card is a picture of a track.
    pub fn rows(&self) -> Vec<MusicCardData> {
        self.results
            .iter()
            .map(|track| {
                let seconds = if track.duration > 0.0 {
                    format!("{:.0}s", track.duration)
                } else {
                    "?s".to_owned()
                };
                let category = {
                    let mut chars = track.category.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                };
                MusicCardData {
                    id: track.id.as_str().into(),
                    title: track.title.as_str().into(),
                    detail: if track.artist.is_empty() {
                        category
                    } else if category.is_empty() {
                        track.artist.clone()
                    } else {
                        format!("{} · {}", track.artist, category)
                    }
                    .into(),
                    meta: format!(
                        "{} · {:.1} MB · {}",
                        seconds,
                        track.bytes as f64 / 1e6,
                        track.source.name(),
                    )
                    .into(),
                    ready: self
                        .library()
                        .is_some_and(|library| library.fetched(&track.id).is_some()),
                    busy: self.fetching.contains_key(&track.id),
                }
            })
            .collect()
    }
}

impl Default for MusicPane {
    fn default() -> Self {
        MusicPane {
            shown: false,
            freepd: library::Status::Missing,
            lofi: library::Status::Missing,
            results: Vec::new(),
            total: 0,
            searching: false,
            last: None,
            fetching: HashMap::new(),
            categories: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: None,
            timer: slint::Timer::default(),
        }
    }
}

impl Studio {
    /// Lays a music track on the timeline: copied into the project's own
    /// `bgm` folder and imported into the bin first - once; a second use
    /// finds the copy already there - then cut onto the first lane with
    /// room at the playhead. Selects what it made, as laying a sound or a
    /// stock clip does.
    pub fn place_bgm(&mut self, name: &str, id: &str, path: &PathBuf) {
        let media_id = match self.adopt_bgm(name, id, path) {
            Ok(id) => id,
            Err(error) => {
                self.notify(&error, true);
                return;
            }
        };
        self.close_source();
        let start = f64::from(self.playhead.max(0.0));
        let created = self.apply(concat_project::Command::AddClipAtFirstFree {
            media_id,
            start,
            above: false,
        });
        if let Some(id) = created {
            self.selection = vec![id];
        }
    }

    /// The bin's id for a music track in this project: its copy in the
    /// project's `bgm` folder, made and imported when it is not there
    /// yet. The track's id travels in the name - `{title}-{id tail}.mp3` -
    /// so the same track is always the same file and two different
    /// tracks never share one.
    fn adopt_bgm(&mut self, name: &str, id: &str, path: &PathBuf) -> Result<String, String> {
        let project = self
            .session
            .as_ref()
            .map(|session| PathBuf::from(session.path()))
            .ok_or_else(|| t("bgm.openProjectFirst"))?;
        let folder = project.join("bgm");
        let size = std::fs::metadata(path)
            .map_err(|error| format!("{}: {error}", path.display()))?
            .len();
        let target = folder.join(format!(
            "{}-{}.mp3",
            clean(name),
            &id[id.len().saturating_sub(6)..]
        ));
        let target_text = target.to_string_lossy().into_owned();
        if let Some(item) = self
            .project()
            .media
            .iter()
            .find(|item| item.path == target_text)
        {
            return Ok(item.id.clone());
        }
        // A file of that name that is not this track - the name carries
        // the library's own id, so this cannot happen, but a hand-made
        // file could sit there - is left alone and the copy takes a
        // numbered name, as a sound's does.
        let mut target = target;
        let mut n = 2;
        while target.exists()
            && std::fs::metadata(&target).map(|meta| meta.len()).ok() != Some(size)
        {
            target = folder.join(format!(
                "{} {n}-{}.mp3",
                clean(name),
                &id[id.len().saturating_sub(6)..]
            ));
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
                .map_err(|error| tf("bgm.couldNotCopy", &[&name, &error]))?;
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
            .ok_or_else(|| tf("bgm.couldNotCopy", &[&name, &"?"]))
    }
}

/// A track's title as a file name a project can hold: the characters a
/// plain name cannot carry taken out, the whole thing cut to a length a
/// name was meant to be.
fn clean(title: &str) -> String {
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
