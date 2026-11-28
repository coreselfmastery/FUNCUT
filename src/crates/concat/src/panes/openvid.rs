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
use concat_host::stock::{self as live, Provider, StockKind};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::OpenvidCardData;

/// How many clips a page of results holds. Forty is two to three screens
/// of rows in a seat of the usual width, and a page is what a search
/// costs; the next page is one More away.
const PAGE: u32 = 40;

/// What to do with a clip once its bytes have arrived.
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    /// Play it through the source preview.
    Preview,
    /// Lay it at the playhead.
    Place,
    /// Lay it where a drag dropped it: the lane the pointer named, when
    /// it named one, and the moment it landed on.
    PlaceAt(Option<String>, f64),
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
    /// page asked for the next page of an existing search or a fresh one;
    /// the third is the search the answer belongs to, so an answer that
    /// arrives after a newer search was asked is dropped rather than
    /// laying its results over the newer one's.
    Searched(Result<Found, String>, bool, u64),
    /// The live shelf's answer: the same shape as [`OpenvidMsg::Searched`],
    /// for a Pexels or Pixabay search the page ran with the settings'
    /// own key - the epoch guard the local search keeps applies to it
    /// exactly the same way.
    LiveSearched(Result<live::Found, String>, bool, u64),
    /// A live-shelf key field was edited: which provider, and what the
    /// field says now.
    StockKey { provider: String, key: String },
    /// The next page of what the last search found.
    More,
    /// A card dragged onto the lanes: fetch it if need be, then lay it
    /// on the lane and at the moment the drop named.
    Place {
        id: String,
        track_id: Option<String>,
        start: f64,
    },
    /// A card clicked: preview the clip, fetching it first if need be.
    Preview(String),
    /// A card's plus: lay the clip at the playhead, fetching it first.
    Add(String),
    /// "Download the index": the one-time fetch, begun.
    Download,
    /// The download's own tick: the pane asks the library where it stands
    /// and publishes, four times a second, until it is done.
    Tick,
    /// The download's worker has returned, however it ended: the poll
    /// can stop once the library is no longer mid-download.
    Ended,
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
    /// The live shelf's hits, when the page is shopping one: Pexels or
    /// Pixabay live, rights-cleared, the key from the settings. A live
    /// search and a local one never share the page: whichever ran last
    /// is what the rows show.
    pub live: Vec<live::Hit>,
    /// How many the live shelf holds, this page aside.
    pub live_total: u64,
    /// The live search the page ran, for More: the provider, the words,
    /// the frame shape the words were narrowed by, and the page the next
    /// ask wants.
    live_last: Option<(Provider, String, Option<&'static str>, u32)>,
    /// Whether the rows are the live shelf's (a key field and a live
    /// shelf pick made them so) or the local index's.
    live_mode: bool,
    /// A search or its next page is running.
    pub searching: bool,
    /// The searches asked so far, counted up as each is sent: the number
    /// an answer must still carry to be the current search's, so one
    /// that arrives late - the workers are a pool, not a queue - is
    /// dropped rather than laid over a newer search's results.
    epoch: u64,
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
        OpenvidPane {
            shown: false,
            state: library::shared(dirs).status(),
            results: Vec::new(),
            total: 0,
            live: Vec::new(),
            live_total: 0,
            live_last: None,
            live_mode: false,
            searching: false,
            epoch: 0,
            last: None,
            fetching: HashMap::new(),
            cameras: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: Some(dirs.clone()),
            timer: slint::Timer::default(),
        }
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
                // The live shelves: source 4 is Pexels live, 5 Pixabay
                // live. The same filters the local search reads - the
                // words, the frame shape - go to the provider's own
                // dialect; the local index's own gates (the download
                // state, the camera list) are the local index's.
                if let Some(provider) = match source {
                    4 => Some(Provider::Pexels),
                    5 => Some(Provider::Pixabay),
                    _ => None,
                } {
                    let key = match provider {
                        Provider::Pexels => studio.prefs.stock.pexels.clone(),
                        Provider::Pixabay => studio.prefs.stock.pixabay.clone(),
                    };
                    let words = query.trim().to_owned();
                    let orientation = match shape {
                        1 => Some("square"),
                        2 => Some("vertical"),
                        3 => Some("horizontal"),
                        _ => None,
                    };
                    self.live_mode = true;
                    self.live_last = Some((provider, words.clone(), orientation, 1));
                    self.results.clear();
                    // A fresh ask empties the page it is about to answer:
                    // the stale rows are not the answer, and the searching
                    // mark is the honest state until the shelf replies.
                    self.live.clear();
                    self.live_total = 0;
                    self.search_live(provider, &words, 1, orientation, key, false);
                    return;
                }
                self.live_mode = false;
                self.live.clear();
                self.live_last = None;
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
            OpenvidMsg::LiveSearched(result, appended, epoch) => {
                if epoch != self.epoch {
                    return; // A newer search was asked; this answer is late.
                }
                self.searching = false;
                match result {
                    Ok(found) => {
                        self.live_total = found.total;
                        if appended {
                            self.live.extend(found.hits);
                        } else {
                            self.live = found.hits;
                        }
                    }
                    // The live shelves' refusals are already words a
                    // person can act on - a missing key names where to
                    // set one, a refused key says so.
                    Err(error) => studio.notify(&error, true),
                }
            }
            OpenvidMsg::StockKey { provider, key } => {
                let key = key.trim().to_owned();
                match provider.as_str() {
                    "pexels" => studio.prefs.stock.pexels = key,
                    "pixabay" => studio.prefs.stock.pixabay = key,
                    _ => return,
                }
                studio.prefs.save(&studio.host.dirs);
            }
            OpenvidMsg::More => {
                if self.searching {
                    return;
                }
                if let Some((provider, words, shape, page)) = self.live_last.clone() {
                    let key = match provider {
                        Provider::Pexels => studio.prefs.stock.pexels.clone(),
                        Provider::Pixabay => studio.prefs.stock.pixabay.clone(),
                    };
                    self.live_last = Some((provider, words.clone(), shape, page + 1));
                    self.search_live(provider, &words, page + 1, shape, key, true);
                    return;
                }
                let Some(mut search) = self.last.clone() else {
                    return;
                };
                search.offset += search.limit;
                // The bumped query is kept, not cloned away: More asks
                // more of *it*, and a query thrown away after the bump
                // re-fetches the second page on every press after the
                // first.
                self.last = Some(search.clone());
                self.search(search, true);
            }
            OpenvidMsg::Searched(result, appended, epoch) => {
                if epoch != self.epoch {
                    return; // A newer search was asked; this answer is late.
                }
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
            OpenvidMsg::Place {
                id,
                track_id,
                start,
            } => self.take(id, Then::PlaceAt(track_id, start), studio),
            OpenvidMsg::Download => {
                let Some(dirs) = self.dirs.clone() else {
                    return;
                };
                // The library's own word, not the pane's copy of it: a
                // download the API's `openvid_prepare` began leaves the
                // pane's state stale, and a second press would only be
                // told there is nothing to do.
                if self.library().is_some_and(|library| {
                    matches!(library.status(), library::Status::Downloading(_))
                }) {
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
                        studio.handle(Msg::Openvid(OpenvidMsg::Ended));
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
            OpenvidMsg::Ended => {
                let Some(library) = self.library() else {
                    self.timer.stop();
                    return;
                };
                self.state = library.status();
                // A download the API began in this window can still be
                // running when the pane's own worker returns with
                // "nothing to do"; its progress is still worth drawing.
                // A failed download leaves no file and no open index,
                // so `status` says `Missing` - the poll would run for
                // the page's life if it waited for `Failed`.
                if !matches!(self.state, library::Status::Downloading(_)) {
                    self.timer.stop();
                    self.refresh();
                }
            }
            OpenvidMsg::Fetched(id, result, then) => {
                self.fetching.remove(&id);
                // A live-shelf card ("pexels:2499611") carries its name in
                // the hit itself; a local one reads it out of the index.
                let live = id.split_once(':');
                let name = if let Some((_, hit_id)) = live {
                    self.live
                        .iter()
                        .find(|hit| hit.id == hit_id)
                        .map(|hit| hit.name.clone())
                        .unwrap_or_default()
                } else {
                    let Some(library) = self.library() else {
                        return;
                    };
                    let Ok(id_number) = id.parse::<i64>() else {
                        return;
                    };
                    library
                        .clip(id_number)
                        .ok()
                        .flatten()
                        .map(|clip| clip.name)
                        .unwrap_or_default()
                };
                match result {
                    // The frame size is free now that the bytes are local,
                    // and the card that asked is redrawn with it. On the
                    // failed road there is nothing on disk to read, and
                    // the network read `dims` would fall back to is not
                    // the window's thread's work: it can hold the window
                    // for the read's whole timeout.
                    Ok(path) => {
                        // A local card's frame size is free now that its
                        // bytes are local, and its card is redrawn with
                        // it; a live card's shelf measured nothing. On
                        // the failed road there is nothing on disk to
                        // read, and the network read `dims` would fall
                        // back to is not the window's thread's work: it
                        // can hold the window for the read's whole
                        // timeout.
                        if live.is_none()
                            && let Some(library) = self.library()
                            && let Ok(id_number) = id.parse::<i64>()
                        {
                            let _ = library.dims(id_number);
                        }
                        // A live card's placement id is its card id with the
                        // shelf's separator made file-safe: the file the
                        // project keeps is named for the clip, and a colon
                        // is not a character a name carries.
                        let file_id = if live.is_some() {
                            id.replace(':', "-")
                        } else {
                            id.clone()
                        };
                        match then {
                            Then::Preview => studio.preview_file(&id, &name, &path),
                            Then::Place => studio.place_openvid(&name, &file_id, &path),
                            Then::PlaceAt(track_id, start) => {
                                studio.place_openvid_at(&name, &file_id, &path, track_id, start)
                            }
                        }
                    }
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
        self.epoch += 1;
        let epoch = self.epoch;
        crate::host::spawn(
            move || library.search(&query),
            move |studio, _, _, result| {
                studio.handle(Msg::Openvid(OpenvidMsg::Searched(result, appended, epoch)));
            },
        );
    }

    /// Runs a live-shelf search on a worker: the same epoch guard the
    /// local one keeps, so an answer that arrives after a newer ask was
    /// made is dropped rather than laid over the newer one's. The key is
    /// the settings' own; a missing one is already the refusal the
    /// library words for a person.
    fn search_live(
        &mut self,
        provider: Provider,
        words: &str,
        page: u32,
        orientation: Option<&str>,
        key: String,
        appended: bool,
    ) {
        let Some(dirs) = self.dirs.clone() else {
            return;
        };
        self.searching = true;
        self.epoch += 1;
        let epoch = self.epoch;
        let words = words.to_owned();
        let orientation = orientation.map(str::to_owned);
        crate::host::spawn(
            move || {
                live::Library::shared(&dirs).search(
                    provider,
                    StockKind::Video,
                    &words,
                    PAGE,
                    page,
                    orientation.as_deref(),
                    Some(&key),
                )
            },
            move |studio, _, _, result| {
                studio.handle(Msg::Openvid(OpenvidMsg::LiveSearched(
                    result, appended, epoch,
                )));
            },
        );
    }

    /// A card's verb: the clip's bytes if they are not on this machine
    /// yet, then [`OpenvidMsg::Fetched`] with what they were for. A clip
    /// already on disk goes straight to its verb.
    fn take(&mut self, id: String, then: Then, studio: &mut Studio) {
        // A live-shelf card's id is "<provider>:<hit>": the shelf's own
        // bytes, from the URL the search page's hit carried.
        if let Some((provider, hit_id)) = id.split_once(':') {
            self.take_live(provider, hit_id, &id, then, studio);
            return;
        }
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
                Then::PlaceAt(track_id, start) => {
                    studio.place_openvid_at(&name, &id, &path, track_id, start)
                }
            }
            return;
        }
        if self.fetching.contains_key(&id) {
            return;
        }
        self.fetching.insert(id.clone(), then.clone());
        // The pane's own flag, so a stop the person asked for (or the
        // window closing) reaches the download instead of a private flag
        // nothing could ever set.
        let cancel = Arc::clone(&self.cancel);
        crate::host::spawn(
            move || library.fetch(number, &cancel),
            move |studio, _, _, result| {
                studio.handle(Msg::Openvid(OpenvidMsg::Fetched(id, result, then)));
            },
        );
    }

    /// The live card's verb: the hit's own URL fetched into the live
    /// cache when the bytes are not on this machine yet, then the same
    /// acts a local card's take lands in - a click previews, a plus lays
    /// at the playhead, a drag places where it landed. The card's id
    /// keeps its provider's name all the way to the placement, where it
    /// is made file-safe before it names a file.
    fn take_live(
        &mut self,
        provider_name: &str,
        hit_id: &str,
        card_id: &str,
        then: Then,
        studio: &mut Studio,
    ) {
        let Some(provider) = live::Provider::of(provider_name) else {
            return;
        };
        let Some(dirs) = self.dirs.clone() else {
            return;
        };
        let Some(hit) = self
            .live
            .iter()
            .find(|hit| hit.provider == provider && hit.id == hit_id)
            .cloned()
        else {
            return;
        };
        let library = live::Library::shared(&dirs);
        let file_id = card_id.replace(':', "-");
        if let Some(path) = library.fetched(provider, &hit.id) {
            match then {
                Then::Preview => studio.preview_file(card_id, &hit.name, &path),
                Then::Place => studio.place_openvid(&hit.name, &file_id, &path),
                Then::PlaceAt(track_id, start) => {
                    studio.place_openvid_at(&hit.name, &file_id, &path, track_id, start)
                }
            }
            return;
        }
        if self.fetching.contains_key(card_id) {
            return;
        }
        self.fetching.insert(card_id.to_owned(), then.clone());
        // The card's id is owned before the worker owns it: the answer
        // arrives after this frame is gone, so nothing the closure keeps
        // may point into it.
        let card_id = card_id.to_owned();
        let cancel = Arc::clone(&self.cancel);
        let url = hit.url.clone();
        let hit_id = hit.id.clone();
        crate::host::spawn(
            move || library.fetch_url(provider, &hit_id, &url, &cancel),
            move |studio, _, _, result| {
                studio.handle(Msg::Openvid(OpenvidMsg::Fetched(card_id, result, then)));
            },
        );
    }

    /// One result's name and seconds, for the ghost a drag shows while
    /// it crosses the lanes. `None` when the id is not on the page.
    pub fn summary(&self, id: &str) -> Option<(String, f64)> {
        // A live-shelf card's name and seconds are the hit's own.
        if let Some((provider, hit_id)) = id.split_once(':') {
            let provider = live::Provider::of(provider)?;
            return self
                .live
                .iter()
                .find(|hit| hit.provider == provider && hit.id == hit_id)
                .map(|hit| (hit.name.clone(), hit.seconds));
        }
        let id = id.parse::<i64>().ok()?;
        self.results
            .iter()
            .find(|clip| clip.id == id)
            .map(|clip| (clip.name.clone(), clip.seconds))
    }

    /// Whether the rows are the live shelves' rather than the local
    /// index's: the page's counts and its More follow the one in play.
    pub fn live_mode(&self) -> bool {
        self.live_mode
    }

    /// The cards, as Slint shows them. Everything display-ready: the
    /// numbers arrive as words because a card is a picture of a clip.
    pub fn rows(&self) -> Vec<OpenvidCardData> {
        // The live shelves' rows: the hit's own words and numbers, the
        // live cache's own ready mark - the same picture of a clip the
        // local rows are, said in the shelf's own terms. The aesthetic
        // badge is empty here: the live shelves carry no such score, and
        // a made-up number would be a lie where a picture asks for one.
        if self.live_mode {
            let library = self.dirs.as_ref().map(|dirs| live::Library::shared(dirs));
            return self
                .live
                .iter()
                .map(|hit| {
                    let mut meta = format!("{:.1}s", hit.seconds);
                    if hit.bytes > 0 {
                        meta.push_str(&format!(" \u{00b7} {:.1} MB", hit.bytes as f64 / 1e6));
                    }
                    meta.push_str(&format!(" \u{00b7} {}\u{d7}{}", hit.width, hit.height));
                    if let Some(fps) = hit.fps {
                        meta.push_str(&format!(" \u{00b7} {:.0} fps", fps));
                    }
                    meta.push_str(&format!(" \u{00b7} {}", hit.provider.name()));
                    let card_id = format!("{}:{}", hit.provider.name(), hit.id);
                    OpenvidCardData {
                        id: card_id.as_str().into(),
                        name: hit.name.as_str().into(),
                        caption: hit.caption.as_str().into(),
                        meta: meta.into(),
                        quality: String::new().into(),
                        ready: library.is_some_and(|library| {
                            library.fetched(hit.provider, &hit.id).is_some()
                        }),
                        busy: self.fetching.contains_key(&card_id),
                    }
                })
                .collect();
        }
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
            live: Vec::new(),
            live_total: 0,
            live_last: None,
            live_mode: false,
            searching: false,
            epoch: 0,
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
        self.place_openvid_at(name, id, path, None, f64::from(self.playhead.max(0.0)));
    }

    /// [`Studio::place_openvid`], where a drag dropped the clip: the lane
    /// the pointer named when it named one, the moment it landed on, and
    /// the first lane with room below the picture otherwise.
    pub fn place_openvid_at(
        &mut self,
        name: &str,
        id: &str,
        path: &PathBuf,
        track_id: Option<String>,
        start: f64,
    ) {
        let media_id = match self.adopt_openvid(name, id, path) {
            Ok(id) => id,
            Err(error) => {
                self.notify(&error, true);
                return;
            }
        };
        self.close_source();
        let created = match track_id {
            // A clip dragged onto a lane ripples it, the way a clip from
            // the bin dropped there does.
            Some(track_id) => self.apply(concat_project::Command::AddClip {
                media_id,
                track_id,
                start,
                ripple: true,
            }),
            None => self.apply(concat_project::Command::AddClipAtFirstFree {
                media_id,
                start,
                above: false,
            }),
        };
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
        // The id names the file the clip's copy lands in, so whatever
        // road it came by it is made a name's own characters on its way
        // in - a live card's shelf separator is not one of them, and a
        // hand-made id with stranger ones is not either.
        let id: String = id
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect::<String>();
        // The name is a provider's own title, not the id's guarded form:
        // live cards carry slashes in their titles ("City/Skyline"), and a
        // path joined on such a name cannot be created under the
        // project's own folder - the clip could never be placed. The
        // name is made a file name the way the BGM shelf makes its own.
        let name: String = name
            .chars()
            .map(|ch| {
                if ch.is_control()
                    || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                {
                    '_'
                } else {
                    ch
                }
            })
            .collect::<String>();
        let name: &str = name.trim().trim_matches('.');
        let name: &str = if name.is_empty() { "Clip" } else { name };
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
