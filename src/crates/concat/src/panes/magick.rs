// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The library's Stickers page: 149,758 transparent PNGs over two
//! shelves - MAGICK (OneOverZero's captioned AI-generated pictures) and
//! OpenMoji (the colour emoji set) - searched in a local index, fetched
//! one picture at a time.
//!
//! Neither shelf is in the repository or the app: the indexes arrive
//! once, ~18 MB, and a picture's own bytes move only when the picture is
//! picked - a click previews one, a plus lays it over the video at the
//! playhead, a heart keeps it on the list. The search engine, the index
//! downloads and the transparency check are `concat_host::magick`'s; see
//! there for how the whole thing stays off the network until a picture
//! is picked.
//!
//! A search runs on a worker and comes back as [`MagickMsg::Searched`],
//! and a fetch as `MagickMsg::Fetched`: the window's thread never waits
//! on a query, however quick the local indexes have made them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use concat_host::AppDirs;
use concat_host::magick::{self as library, Found, Query, Source};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::MagickCardData;

/// How many pictures a page of results holds: the tiles are ~100px
/// wide, so a page is four to eight screens of them in a seat of the
/// usual width, and a page is what a search costs; the next page is one
/// More away.
const PAGE: u32 = 48;

/// What to do with a picture once its bytes have arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Then {
    /// Show it through the source preview.
    Preview,
    /// Lay it over the video at the playhead.
    Place,
}

/// Everything that can happen to the page.
#[derive(Clone, Debug)]
pub enum MagickMsg {
    /// A library pane turned to the page or away from it.
    Shown(bool),
    /// The search field's words changed, or a filter moved. Every filter
    /// arrives with it, because the page's filters live in the UI global
    /// and this is the one moment they are read.
    Search {
        query: String,
        source: i32,
        favourites: bool,
    },
    /// The search's worker is done. The second field says whether the
    /// page asked for the next page of an existing search or a fresh one.
    Searched(Result<Found, String>, bool),
    /// The next page of what the last search found.
    More,
    /// A tile clicked: preview the picture, fetching it first if need be.
    Preview(String),
    /// A tile's plus: lay the picture at the playhead, fetching it first.
    Add(String),
    /// A tile's heart: press or lift it.
    Heart(String),
    /// "Load the sticker library": the one-time index downloads, begun.
    Download,
    /// The loads' own tick: the pane asks each shelf where it stands and
    /// publishes, four times a second, until they are done.
    Tick,
    /// A fetch is done, and what its bytes were for.
    Fetched(String, Result<PathBuf, String>, Then),
}

/// The page's state.
pub struct MagickPane {
    /// A library pane is showing the page.
    pub shown: bool,
    /// The shelves as the pane last read them.
    pub magick: library::Status,
    pub openmoji: library::Status,
    /// The pictures on the page, in the order the search ranked them.
    pub results: Vec<library::Sticker>,
    /// How many pictures match in all, this page aside.
    pub total: u64,
    /// A search or its next page is running.
    pub searching: bool,
    /// The last search, as it was asked: what More asks more of.
    last: Option<Query>,
    /// Fetches running, by picture id: what the bytes are for.
    fetching: HashMap<String, Then>,
    /// The heart's list, read once when the page needs it.
    hearts: Vec<String>,
    /// Stops the loads when the window goes.
    cancel: Arc<AtomicBool>,
    /// The app's own folders, for the library's own.
    dirs: Option<AppDirs>,
    /// The loads' progress poll, running only while one runs.
    timer: slint::Timer,
}

impl MagickPane {
    /// The pane, with the shelves as they stand on this machine. An
    /// index on disk from an earlier run is taken in here, so the page
    /// is ready from the first frame and nothing is fetched until its
    /// Load button or a search asks for it.
    pub fn open(dirs: &AppDirs) -> MagickPane {
        MagickPane {
            shown: false,
            magick: library::shared(dirs).status(Source::Magick),
            openmoji: library::shared(dirs).status(Source::Openmoji),
            results: Vec::new(),
            total: 0,
            searching: false,
            last: None,
            fetching: HashMap::new(),
            hearts: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: Some(dirs.clone()),
            timer: slint::Timer::default(),
        }
    }

    /// The library this pane reads, when it has folders to read from.
    fn library(&self) -> Option<&'static library::Library> {
        self.dirs.as_ref().map(library::shared)
    }

    /// How many pictures a shelf holds, once ready.
    pub fn shelf_rows(&self, source: Source) -> Option<u64> {
        self.library().and_then(|library| library.rows(source))
    }

    /// The library's own word for a shelf, read fresh: the publish asks
    /// it rather than the pane's copy, which only moves when the page is
    /// shown or its loads tick.
    pub fn library_status(&self, source: Source) -> library::Status {
        self.library()
            .map_or(library::Status::Missing, |library| library.status(source))
    }

    /// The page's one word for the shelves, as the status card takes it:
    /// 0 both missing, 1 a load running, 2 a shelf ready, 3 both
    /// failed.
    pub fn combined(&self) -> i32 {
        let ready = |status: &library::Status| matches!(status, library::Status::Ready);
        let loading = |status: &library::Status| matches!(status, library::Status::Downloading(_));
        let failed = |status: &library::Status| matches!(status, library::Status::Failed(_));
        if loading(&self.magick) || loading(&self.openmoji) {
            1
        } else if ready(&self.magick) || ready(&self.openmoji) {
            2
        } else if failed(&self.magick) && failed(&self.openmoji) {
            3
        } else {
            0
        }
    }

    /// Applies one message. The studio is the rest of the window; while
    /// this runs the studio's copy of the pane is a blank it must not read.
    pub fn update(&mut self, msg: MagickMsg, studio: &mut Studio) {
        match msg {
            MagickMsg::Shown(on) => {
                self.shown = on;
                if !on {
                    self.timer.stop();
                    return;
                }
                self.refresh();
            }
            MagickMsg::Search {
                query,
                source,
                favourites,
            } => {
                let search = Query {
                    text: (!query.trim().is_empty()).then(|| query.clone()),
                    source: match source {
                        1 => Some(Source::Magick),
                        2 => Some(Source::Openmoji),
                        _ => None,
                    },
                    favourites,
                    limit: PAGE,
                    ..Query::default()
                };
                self.last = Some(search.clone());
                self.search(search, false);
            }
            MagickMsg::More => {
                let Some(mut search) = self.last.clone() else {
                    return;
                };
                search.offset += search.limit;
                self.search(search, true);
            }
            MagickMsg::Searched(result, appended) => {
                self.searching = false;
                match result {
                    Ok(found) => {
                        self.total = found.total;
                        if appended {
                            self.results.extend(found.stickers);
                        } else {
                            self.results = found.stickers;
                        }
                    }
                    Err(error) => studio.notify(&error, true),
                }
            }
            MagickMsg::Preview(id) => self.take(id, Then::Preview, studio),
            MagickMsg::Add(id) => self.take(id, Then::Place, studio),
            MagickMsg::Heart(id) => {
                let Some(library) = self.library() else {
                    return;
                };
                match library.toggle_favourite(&id) {
                    Ok(_) => self.hearts = library.favourite_ids(),
                    Err(error) => studio.notify(&error, true),
                }
            }
            MagickMsg::Download => {
                let Some(dirs) = self.dirs.clone() else {
                    return;
                };
                if matches!(self.magick, library::Status::Downloading(_))
                    || matches!(self.openmoji, library::Status::Downloading(_))
                {
                    return;
                }
                self.magick = library::Status::Downloading(0);
                self.openmoji = library::Status::Downloading(0);
                // The poll: the loads tick the shelves' own counters, and
                // the page reads them four times a second until they end.
                self.timer.start(
                    slint::TimerMode::Repeated,
                    std::time::Duration::from_millis(250),
                    || {
                        crate::host::Shell::with(|shell, _| {
                            let mut studio = shell.studio.borrow_mut();
                            studio.handle(Msg::Magick(MagickMsg::Tick));
                        });
                    },
                );
                let cancel = Arc::clone(&self.cancel);
                for source in Source::ALL {
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
                                    Source::Magick => t("magick.magickReady"),
                                    Source::Openmoji => t("magick.openmojiReady"),
                                },
                                Err(error) => format!(
                                    "{}: {error}",
                                    match source {
                                        Source::Magick => t("magick.magick"),
                                        Source::Openmoji => t("magick.openmoji"),
                                    }
                                ),
                            };
                            studio.notify(&words, failed);
                        },
                    );
                }
            }
            MagickMsg::Tick => {
                let Some(library) = self.library() else {
                    self.timer.stop();
                    return;
                };
                self.magick = library.status(Source::Magick);
                self.openmoji = library.status(Source::Openmoji);
                let loading = matches!(self.magick, library::Status::Downloading(_))
                    || matches!(self.openmoji, library::Status::Downloading(_));
                if !loading {
                    self.timer.stop();
                    self.refresh();
                }
            }
            MagickMsg::Fetched(id, result, then) => {
                self.fetching.remove(&id);
                let Some(library) = self.library() else {
                    return;
                };
                let name = library
                    .find(&id)
                    .map(|sticker| sticker.subject)
                    .unwrap_or_default();
                match result {
                    Ok(path) => match then {
                        Then::Preview => studio.preview_file(&id, &name, &path),
                        Then::Place => studio.place_magick(&name, &id, &path),
                    },
                    Err(error) if error == "cancelled" => {}
                    Err(error) => studio.notify(&tf("magick.fetchFailed", &[&name, &error]), true),
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
        self.magick = library.status(Source::Magick);
        self.openmoji = library.status(Source::Openmoji);
        // The heart's list, read here so the tiles light up as they are
        // laid out rather than after the first press.
        self.hearts = library.favourite_ids();
        if self.combined() == 2 && self.results.is_empty() && !self.searching && self.last.is_none()
        {
            self.search(
                Query {
                    limit: PAGE,
                    ..Query::default()
                },
                false,
            );
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
                studio.handle(Msg::Magick(MagickMsg::Searched(result, appended)));
            },
        );
    }

    /// A tile's verb: the picture's bytes if they are not on this
    /// machine yet, then [`MagickMsg::Fetched`] with what they were for.
    /// A picture already on disk goes straight to its verb.
    fn take(&mut self, id: String, then: Then, studio: &mut Studio) {
        let Some(library) = self.library() else {
            return;
        };
        if let Some(path) = library.fetched(&id) {
            let name = library
                .find(&id)
                .map(|sticker| sticker.subject)
                .unwrap_or_default();
            match then {
                Then::Preview => studio.preview_file(&id, &name, &path),
                Then::Place => studio.place_magick(&name, &id, &path),
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
        let source = library
            .find(&id)
            .map(|sticker| sticker.source)
            .unwrap_or(Source::Magick);
        crate::host::spawn(
            move || {
                let mut progress = |_, _| {};
                library.fetch(source, &fetching, &cancel, &mut progress)
            },
            move |studio, _, _, result| {
                studio.handle(Msg::Magick(MagickMsg::Fetched(id, result, then)));
            },
        );
    }

    /// The heart's list, read once per change so a tile asks no question
    /// a list walk answers.
    fn hearted(&self, id: &str) -> bool {
        self.hearts.iter().any(|held| held == id)
    }

    /// The tiles, as Slint shows them. Everything display-ready: the
    /// picture itself only when it is on this machine, and the shelf as
    /// the corner's own word.
    pub fn rows(&self) -> Vec<MagickCardData> {
        self.results
            .iter()
            .map(|sticker| {
                // The picture's own bytes, only when they are on this
                // machine; an empty image until then, so the tile draws
                // its glyph.
                let cached = self
                    .library()
                    .and_then(|library| library.fetched(&sticker.id));
                let picture = cached
                    .as_ref()
                    .and_then(|path| crate::host::image_at(path))
                    .unwrap_or_default();
                MagickCardData {
                    id: sticker.id.as_str().into(),
                    subject: sticker.subject.as_str().into(),
                    shelf: match sticker.source {
                        Source::Magick => "MAGICK",
                        Source::Openmoji => "OpenMoji",
                    }
                    .into(),
                    image: picture,
                    ready: cached.is_some(),
                    busy: self.fetching.contains_key(&sticker.id),
                    heart: self.hearted(&sticker.id),
                }
            })
            .collect()
    }
}

impl Default for MagickPane {
    fn default() -> Self {
        MagickPane {
            shown: false,
            magick: library::Status::Missing,
            openmoji: library::Status::Missing,
            results: Vec::new(),
            total: 0,
            searching: false,
            last: None,
            fetching: HashMap::new(),
            hearts: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: None,
            timer: slint::Timer::default(),
        }
    }
}

impl Studio {
    /// Lays a sticker over the video: copied into the project's own
    /// `magick` folder and imported into the bin first - once; a second
    /// use finds the copy already there - then cut onto the first free
    /// lane *above* the highest occupied at the playhead, so a sticker
    /// never hides beneath the video. Selects what it made, as laying a
    /// sound or a stock clip does.
    pub fn place_magick(&mut self, name: &str, id: &str, path: &PathBuf) {
        let media_id = match self.adopt_magick(name, id, path) {
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
            above: true,
        });
        if let Some(id) = created {
            self.selection = vec![id];
        }
    }

    /// The bin's id for a sticker in this project: its copy in the
    /// project's `magick` folder, made and imported when it is not there
    /// yet. The picture's id travels in the name (`{caption}-{id}.png`),
    /// so the same picture is always the same file and two different
    /// pictures never share one.
    fn adopt_magick(&mut self, name: &str, id: &str, path: &PathBuf) -> Result<String, String> {
        let project = self
            .session
            .as_ref()
            .map(|session| PathBuf::from(session.path()))
            .ok_or_else(|| t("magick.openProjectFirst"))?;
        let folder = project.join("magick");
        let size = std::fs::metadata(path)
            .map_err(|error| format!("{}: {error}", path.display()))?
            .len();
        let target = folder.join(format!("{}-{id}.png", clean(name)));
        let target_text = target.to_string_lossy().into_owned();
        if let Some(item) = self
            .project()
            .media
            .iter()
            .find(|item| item.path == target_text)
        {
            return Ok(item.id.clone());
        }
        // A file of that name that is not this picture - the name carries
        // the library's own id, so this cannot happen, but a hand-made
        // file could sit there - is left alone and the copy takes a
        // numbered name, as a sound's does.
        let mut target = target;
        let mut n = 2;
        while target.exists()
            && std::fs::metadata(&target).map(|meta| meta.len()).ok() != Some(size)
        {
            target = folder.join(format!("{} {n}-{id}.png", clean(name)));
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
                .map_err(|error| tf("magick.couldNotCopy", &[&name, &error]))?;
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
            .ok_or_else(|| tf("magick.couldNotCopy", &[&name, &"?"]))
    }
}

/// A picture's caption as a file name a project can hold: the characters
/// a plain name cannot carry taken out, the whole thing cut to a length
/// a name was meant to be. The library's own id beside it is what makes
/// the name unique; this only makes it polite.
fn clean(subject: &str) -> String {
    let clean: String = subject
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
        "Sticker".to_owned()
    } else {
        cut
    }
}
