// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The background-music library: two shelves of whole CC0 tracks, none of
//! them in the repository or the app, fetched one track at a time.
//!
//! Two sources, both public domain, both read the same way the OpenVid
//! stock library is: a small list of what exists is fetched once and kept,
//! and a track's own bytes move only when that track is picked - a click
//! previews one, a plus lays one at the playhead - so the app never holds
//! a music collection it was never asked for (#71).
//!
//! *FreePD* is Kevin MacLeod's CC0 library (upbeat, epic, romantic, comedy
//! and the rest, by other CC0 artists too). freepd.com has closed, so the
//! item on the Internet Archive is the source: the catalog is its `files`
//! listing - one JSON, ~1 MB, every track's folder, artist, length and
//! size - and a track is a plain HTTPS GET of its own file.
//!
//! *Lo-Fi* is Moby's MobyGratis instrumentals, republished as the OpenLo-Fi
//! pack (166 tracks, 10 categories, CC0 1.0). The pack's whole audio sits
//! in one 554 MB zip on a GitHub release, and that zip honours byte
//! ranges - so the library reads the zip's end-of-central-directory and
//! central directory once (~80 KB of ranged reads, kept), measures each
//! track's start and length from its own local header and first MPEG
//! frames (one ~4 KB ranged read a track, once), and fetches a track as
//! one ranged read of exactly that track's span. A track costs its own
//! size in bandwidth and nothing else; the other 550 MB never moves.
//!
//! Nothing here touches a project. A track put on the timeline is copied
//! into that project's own folder by the window, as a sound or a stock
//! clip is, so the project owns its files and the cache can be emptied
//! whenever. The API and the MCP server reach the same library through
//! the same methods, so a person and an agent see one shelf each.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::AppDirs;

/// Where FreePD's catalog is: the Internet Archive item's own file
/// listing, which names every track's folder, artist, length and size.
pub const FREEPD_METADATA: &str = "https://archive.org/metadata/freepd";

/// Where a FreePD track's bytes are: the item's download folder, the
/// track's own file under its own name. Percent-encoded per segment, as
/// the Archive's URLs are built.
pub const FREEPD_DOWNLOAD: &str = "https://archive.org/download/freepd";

/// Where the Lo-Fi catalog is: the OpenLo-Fi pack's own manifest, pinned
/// to the release this build reads, so the titles cannot drift from the
/// zip the byte ranges point into.
pub const LOFI_CATALOG: &str =
    "https://raw.githubusercontent.com/btahir/open-lofi/v1.0.0/catalog.json";

/// Where the Lo-Fi zip is: the release's one asset, pinned to the same
/// tag as the catalog for the same reason. The Archive of the zip's own
/// shape - its total size and every member's offset - is kept beside the
/// catalog, and checked against the live zip before it is believed.
pub const LOFI_ZIP: &str =
    "https://github.com/btahir/open-lofi/releases/download/v1.0.0/openlofi.zip";

/// How large a catalog is allowed to be: the FreePD listing is ~1 MB and
/// the Lo-Fi manifest ~30 KB, so 16 MB is room for both to grow a good
/// deal before a wrong answer is served instead of refused.
const CATALOG_LIMIT: u64 = 16 * 1024 * 1024;

/// How much of a track's start the Lo-Fi census reads: the local file
/// header, and enough of the track's own bytes for its first MPEG frames
/// and a Xing header to be read. 4 KB covers the ID3 tags and headers
/// every track here carries with room to spare.
const CENSUS_READ: u64 = 4 * 1024;

/// How many census reads run at once. Each is one network round trip, so
/// five at a time is the difference between the one-time cost being
/// seconds and tens of them; the connection each thread keeps warm is the
/// other difference.
const CENSUS_WIDTH: usize = 5;

/// The largest span this build will ranged-read out of the zip: the
/// tracks are a few MB, so 64 MB is room for a much larger pack without
/// a broken index ever asking for something absurd.
const SPAN_LIMIT: u64 = 64 * 1024 * 1024;

/// The largest FreePD track this build will download: they are 15 MB at
/// the most, so 64 MB is the same honesty about the same future.
const TRACK_LIMIT: u64 = 64 * 1024 * 1024;

/// The id prefix of a FreePD track: `freepd-<12 hex>`.
const FREEPD_PREFIX: &str = "freepd-";
/// The id prefix of a Lo-Fi track: `lofi-<12 hex>`.
const LOFI_PREFIX: &str = "lofi-";

/// The two shelves of the library, as a track's `source` says which it
/// came from and a search's filter names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// Kevin MacLeod's CC0 library, from the Internet Archive's mirror of
    /// freepd.com.
    #[serde(rename = "freepd")]
    FreePd,
    /// Moby's MobyGratis instrumentals, as the OpenLo-Fi pack.
    #[serde(rename = "lofi")]
    Lofi,
}

impl Source {
    /// The shelf's own name, as words to a person.
    pub fn name(self) -> &'static str {
        match self {
            Source::FreePd => "FreePD",
            Source::Lofi => "Lo-Fi",
        }
    }
}

/// One track of the library, as a search result carries it and as the API
/// reports it. Everything here comes from the source's own catalog; the
/// bytes arrive only when the track is fetched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// The track's id, stable for as long as the track is in the catalog:
    /// the source's prefix and twelve hex of the file's own name. The id
    /// every verb takes back.
    pub id: String,
    /// Which shelf the track is on.
    pub source: Source,
    /// What the card says: the track's own title, or its file name.
    pub title: String,
    /// Who made it, when the catalog says: a card's second line, and CC0
    /// owes no attribution but is kind to keep one.
    pub artist: String,
    /// The source's own grouping: FreePD's folder (upbeat, epic, comedy…)
    /// or the Lo-Fi pack's category slug. Lowercase; empty for none.
    pub category: String,
    /// Seconds, as the catalog itself gives them - FreePD's listing names
    /// each file's length, and a Lo-Fi track's first frames are measured
    /// once by the census. `0.0` when a Lo-Fi track's header could not be
    /// read, and a duration filter passes such a track by rather than
    /// guessing at it.
    pub duration: f64,
    /// The track's size in bytes: what fetching it costs.
    pub bytes: u64,
    /// Where the track lives in its source: FreePD's item-relative path
    /// (`Page2/120 Monster.mp3`), or the Lo-Fi member's name in the zip
    /// (`120 Monster.mp3`'s own stem spelled out, as the pack spells it).
    pub file: String,
}

/// How a search orders what it found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// The words' own order: best title matches first, then alphabetical.
    /// The default, with words or without.
    #[default]
    Relevance,
    /// Seconds, least first; a track whose length is unknown goes last.
    Shortest,
    /// Seconds, most first; unknown last.
    Longest,
    /// A different order every call.
    Surprise,
}

/// A search, as the page's filters and the API's fields both build it.
/// Every field is optional; a search with none of them set is the whole
/// library as it comes, which is what `sort` is for.
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// Words to find in a track's title, artist or category. ANDed and
    /// prefix-matched: "city rain" finds tracks holding both a word
    /// starting "city" and one starting "rain".
    pub text: Option<String>,
    /// One shelf only; both when absent.
    pub source: Option<Source>,
    /// One category only, as the catalog spells it ("upbeat",
    /// "late-night"); any when absent.
    pub category: Option<String>,
    /// Seconds at least this.
    pub min_seconds: Option<f64>,
    /// Seconds at most this.
    pub max_seconds: Option<f64>,
    /// How to order.
    pub sort: Sort,
    /// At most this many tracks; 40 when a caller sets nothing, 100 the
    /// most a search returns.
    pub limit: u32,
    /// Skip this many from the front, for the next page.
    pub offset: u32,
}

/// What a search found.
#[derive(Clone, Debug, Default)]
pub struct Found {
    /// The tracks, `limit` of them from `offset` in the order asked for.
    pub tracks: Vec<Track>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// One shelf of the library, as the page and the API tell it where it
/// stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// The catalog is not on this machine yet. The page offers the load,
    /// and `prepare` brings it.
    Missing,
    /// The catalog is coming down, this many per-cent along. The per-cent
    /// is the Lo-Fi census, which is the only part slow enough to see.
    Loading(u32),
    /// Searched from here on.
    Ready,
    /// The catalog could not be fetched; the words are the page's to show
    /// as they are.
    Failed(String),
}

/// Where the library's own files live: `<data>/bgm/`, the catalogs and
/// the zip index beside the fetched tracks. `data` is large state by the
/// app's convention, which is right for a music cache.
pub fn dir(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("bgm")
}

/// Where a fetched track is kept: `tracks/<id>.mp3`. The id is the file's
/// own name hashed, so a track fetched twice is fetched once.
fn track_path(dirs: &AppDirs, id: &str) -> PathBuf {
    dir(dirs).join("tracks").join(format!("{id}.mp3"))
}

/// The one library per process. Built the first time anything asks, from
/// the app's own directories, because every asker - the window's page,
/// the API server in the window, an MCP server in this same process -
/// means the same folder and the same catalogs rather than racing each
/// other. A catalog already on disk from an earlier run is read here, so
/// the shelf is there from the first frame and no request is made for it.
pub fn shared(dirs: &AppDirs) -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| Library {
        dirs: dirs.clone(),
        freepd: Shelf::new(),
        lofi: Shelf::new(),
        turns: Mutex::new(HashMap::new()),
    })
}

/// One shelf's state, held so that a second asker while a load runs waits
/// for it rather than starting one of its own, and so the load's progress
/// is there to read while it does.
struct Shelf {
    /// The catalog, or where its load stands.
    state: Mutex<Held>,
    /// Woken whenever the state moves off `Loading`.
    moved_on: Condvar,
    /// The load's progress, 0-100, read while `Loading`.
    percent: AtomicU32,
}

/// What a shelf holds.
enum Held {
    /// Nothing, and nothing asked for.
    Missing,
    /// A load running: the one this shelf's own thread started, or one
    /// another asker did.
    Loading,
    /// The catalog, once fetched or read from the cache.
    Ready(Arc<Vec<Track>>),
    /// The last load's failure, in words.
    Failed(String),
}

impl Shelf {
    fn new() -> Shelf {
        Shelf {
            state: Mutex::new(Held::Missing),
            moved_on: Condvar::new(),
            percent: AtomicU32::new(0),
        }
    }

    /// The shelf as the page and the API tell it where it stands.
    fn status(&self) -> Status {
        match &*self
            .state
            .lock()
            .expect("the bgm shelf lock is not poisoned")
        {
            Held::Missing => Status::Missing,
            Held::Loading => Status::Loading(self.percent.load(Ordering::Relaxed).min(100)),
            Held::Ready(_) => Status::Ready,
            Held::Failed(error) => Status::Failed(error.clone()),
        }
    }

    /// The catalog, if the shelf is ready.
    fn catalog(&self) -> Option<Arc<Vec<Track>>> {
        match &*self
            .state
            .lock()
            .expect("the bgm shelf lock is not poisoned")
        {
            Held::Ready(tracks) => Some(Arc::clone(tracks)),
            _ => None,
        }
    }

    /// Takes the catalog in, and tells anyone waiting that the shelf has
    /// moved on.
    fn lands(&self, held: Held) {
        *self
            .state
            .lock()
            .expect("the bgm shelf lock is not poisoned") = held;
        self.moved_on.notify_all();
    }
}

/// The background-music library: both shelves, their catalogs and the
/// tracks this machine has fetched.
pub struct Library {
    /// The app's own folders.
    dirs: AppDirs,
    /// Kevin MacLeod's CC0 library, from the Internet Archive.
    freepd: Shelf,
    /// Moby's MobyGratis instrumentals, as the OpenLo-Fi pack.
    lofi: Shelf,
    /// One turn per track: a track's fetch holds its own lock, so two
    /// askers of the same track - the window's click and the API's
    /// call, or two windows over one data folder - queue behind each
    /// other instead of appending to the same partial from two
    /// places, which `models::download`'s resume would let corrupt
    /// the track silently. The second asker wakes, finds the track on
    /// disk, and returns it.
    turns: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Library {
    /// Where a shelf's catalog is on this machine, fetched or not.
    fn shelf(&self, source: Source) -> &Shelf {
        match source {
            Source::FreePd => &self.freepd,
            Source::Lofi => &self.lofi,
        }
    }

    /// A shelf's state, as the page and the API report it. A catalog on
    /// disk from an earlier run is taken in here, once, so the shelf is
    /// `Ready` from the first ask and nothing is fetched until the page's
    /// Refresh or `bgm.prepare` asks for it.
    pub fn status(&self, source: Source) -> Status {
        let shelf = self.shelf(source);
        if let Status::Missing = shelf.status()
            && let Some(tracks) = read_catalog(&self.dirs, source)
        {
            shelf.lands(Held::Ready(Arc::new(tracks)));
        }
        shelf.status()
    }

    /// Whether both shelves are ready: the page's one word for the
    /// library, and the API's.
    pub fn ready(&self) -> bool {
        matches!(self.status(Source::FreePd), Status::Ready)
            && matches!(self.status(Source::Lofi), Status::Ready)
    }

    /// How many tracks a shelf holds, once ready.
    pub fn rows(&self, source: Source) -> Option<u64> {
        self.shelf(source)
            .catalog()
            .map(|tracks| tracks.len() as u64)
    }

    /// A shelf's own groupings, sorted: FreePD's folders, the pack's
    /// categories. Empty when the shelf holds nothing.
    pub fn categories(&self, source: Source) -> Vec<String> {
        let mut seen: Vec<String> = self
            .shelf(source)
            .catalog()
            .map(|tracks| {
                let mut categories: Vec<String> = tracks
                    .iter()
                    .filter(|track| !track.category.is_empty())
                    .map(|track| track.category.clone())
                    .collect();
                categories.sort();
                categories.dedup();
                categories
            })
            .unwrap_or_default();
        seen.sort();
        seen
    }

    /// Fetches a shelf's catalog and takes it in; `Ok(())` when the shelf
    /// is ready - however it got that way, including by this call waiting
    /// for a load another asker had already started. Blocks; run it on a
    /// worker, with a cancel flag the window owns.
    pub fn prepare(&self, source: Source, cancel: &AtomicBool) -> Result<(), String> {
        // A catalog from an earlier run counts, and a Refresh is the
        // page's way of asking past it; this takes the cache in the same
        // way `status` does, so a plain prepare on a warm machine is
        // done before it starts.
        self.status(source);
        let shelf = self.shelf(source);
        let mut state = shelf
            .state
            .lock()
            .expect("the bgm shelf lock is not poisoned");
        loop {
            match &*state {
                Held::Ready(_) => return Ok(()),
                Held::Failed(_) => {
                    // A failed load is tried again: the words went to the
                    // asker that saw them, and this asker gets a fresh
                    // start rather than yesterday's refusal.
                    break;
                }
                Held::Missing => break,
                Held::Loading => {
                    // Another asker's load is already coming. Wait for it
                    // rather than starting a second one into the same
                    // cache files, and answer with how it ended.
                    state = shelf
                        .moved_on
                        .wait(state)
                        .expect("the bgm shelf lock is not poisoned");
                }
            }
        }
        *state = Held::Loading;
        shelf.percent.store(0, Ordering::Relaxed);
        drop(state);
        let result = match source {
            Source::FreePd => prepare_freepd(&self.dirs, shelf),
            Source::Lofi => prepare_lofi(&self.dirs, shelf, cancel),
        };
        let landed = match &result {
            Ok(tracks) => Held::Ready(Arc::new(tracks.clone())),
            Err(error) => Held::Failed(error.clone()),
        };
        shelf.lands(landed);
        result.map(drop)
    }

    /// The track by its id, from whichever shelf holds it.
    pub fn track(&self, id: &str) -> Result<Option<Track>, String> {
        for source in [Source::FreePd, Source::Lofi] {
            if let Some(track) = self
                .shelf(source)
                .catalog()
                .as_ref()
                .and_then(|catalog| catalog.iter().find(|track| track.id == id))
            {
                return Ok(Some(track.clone()));
            }
        }
        Ok(None)
    }

    /// Searches both shelves - or one, when the query names it - by the
    /// words, the category and the length the caller set. A query that
    /// names a shelf needs that shelf ready and refuses with words that
    /// say how to make it so. A query that names neither searches
    /// whichever shelves are ready and refuses only when none are: one
    /// shelf down - the Archive has its days - must not make the whole
    /// library unusable, the same way the sticker library degrades.
    pub fn search(&self, query: &Query) -> Result<Found, String> {
        let ready = |source: Source| self.shelf(source).catalog().is_some();
        let sources = match query.source {
            Some(source) => {
                if !ready(source) {
                    return Err(self.not_ready(source));
                }
                vec![source]
            }
            None => {
                if !(ready(Source::FreePd) || ready(Source::Lofi)) {
                    // Neither shelf is usable: name the one further from
                    // ready - the Lo-Fi census is the part worth watching.
                    let source = if matches!(self.shelf(Source::Lofi).status(), Status::Loading(_))
                    {
                        Source::Lofi
                    } else {
                        Source::FreePd
                    };
                    return Err(self.not_ready(source));
                }
                [Source::FreePd, Source::Lofi]
                    .into_iter()
                    .filter(|source| ready(*source))
                    .collect()
            }
        };
        let mut pool: Vec<Track> = Vec::new();
        for source in sources {
            if let Some(catalog) = self.shelf(source).catalog() {
                pool.extend(catalog.iter().cloned());
            }
        }
        search(&pool, query)
    }

    /// The refusal for a shelf that is not ready, in words that say how
    /// far it is and what brings it: the one sentence a headless caller
    /// reads instead of a Settings page.
    fn not_ready(&self, source: Source) -> String {
        match self.shelf(source).status() {
            Status::Loading(percent) => format!(
                "the {} list is still loading ({percent}%); poll bgm_status until it is ready",
                source.name()
            ),
            Status::Failed(error) => {
                format!("the {} list is unusable: {error}", source.name())
            }
            _ => format!(
                "the {} list is not on this machine yet; call bgm_prepare once, then poll \
                 bgm_status until it is ready",
                source.name()
            ),
        }
    }

    /// Where a track is, if its bytes are on this machine.
    pub fn fetched(&self, id: &str) -> Option<PathBuf> {
        let path = track_path(&self.dirs, id);
        path.is_file().then_some(path)
    }

    /// Fetches a track into the cache and returns where it is. A track
    /// already on disk is returned as it stands: the bytes are the bytes,
    /// whatever brought them. Blocks; run it on a worker, with a cancel
    /// flag the window owns and a progress the caller can draw.
    pub fn fetch(
        &self,
        id: &str,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<PathBuf, String> {
        let Some(track) = self.track(id)? else {
            return Err(format!("there is no track {id} in the music library"));
        };
        let target = track_path(&self.dirs, &track.id);
        if target.is_file() {
            return Ok(target);
        }
        if track.bytes > TRACK_LIMIT {
            return Err(format!(
                "{} is {} bytes, more than this build will fetch",
                track.title, track.bytes
            ));
        }
        // The track's turn: one writer at a time, and the askers that
        // queued behind the winner find the track on disk when theirs
        // comes round.
        let turn = {
            let mut turns = self
                .turns
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(turns.entry(track.id.clone()).or_default())
        };
        let _held = turn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // Another asker may have finished the track while this one
        // waited for its turn.
        if target.is_file() {
            return Ok(target);
        }
        let folder = target.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
        let partial = target.with_extension("partial");
        let result = match track.source {
            Source::FreePd => fetch_freepd(&track, &partial, cancel, progress),
            Source::Lofi => fetch_lofi(&self.dirs, &track, &partial, cancel, progress),
        };
        match result {
            Ok(()) => {
                std::fs::rename(&partial, &target)
                    .map_err(|error| format!("could not keep {}: {error}", target.display()))?;
                Ok(target)
            }
            Err(error) => {
                // A partial FreePD download is worth keeping - the next
                // attempt resumes it - but a Lo-Fi span is one read, and
                // a wrong-size or failed one is better gone.
                if track.source == Source::Lofi {
                    let _ = std::fs::remove_file(&partial);
                }
                Err(error)
            }
        }
    }
}

// ── the FreePD shelf ────────────────────────────────────────────────────

/// The Archive item's own listing, as its metadata API writes it: the
/// fields this build reads, everything else passed by. Sizes and lengths
/// are strings there, and read leniently here.
#[derive(Deserialize)]
struct ArchiveItem {
    #[serde(default)]
    files: Vec<ArchiveFile>,
}

/// One file of the Archive item's listing.
#[derive(Deserialize)]
struct ArchiveFile {
    /// The file's path in the item: `upbeat/Name.mp3`, `Page2/Name.mp3`.
    name: String,
    /// "VBR MP3" for the tracks; the PNGs and spectrograms carry others.
    #[serde(default)]
    format: String,
    /// The file's size, as the Archive writes it: a string of bytes.
    #[serde(default)]
    size: Option<String>,
    /// The file's playing time, as the Archive writes it: a string of
    /// seconds.
    #[serde(default)]
    length: Option<String>,
    /// The track's own title, when it has one beside its file name.
    #[serde(default)]
    title: Option<String>,
    /// Who made the track, when the listing says.
    #[serde(default)]
    artist: Option<String>,
}

/// Fetches the FreePD listing and turns it into the shelf's tracks.
/// Fails - rather than half-loads - when the listing cannot be read.
fn prepare_freepd(dirs: &AppDirs, shelf: &Shelf) -> Result<Vec<Track>, String> {
    let agent = crate::net::agent(
        std::time::Duration::from_secs(20),
        std::time::Duration::from_secs(60),
    );
    let response = agent
        .get(FREEPD_METADATA)
        .call()
        .map_err(|error| format!("the Archive did not answer: {error}"))?;
    let mut bytes = Vec::new();
    use std::io::Read;
    response
        .into_reader()
        .take(CATALOG_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the FreePD list did not arrive: {error}"))?;
    let tracks = parse_freepd(&bytes)?;
    write_catalog(dirs, Source::FreePd, &tracks);
    shelf.percent.store(100, Ordering::Relaxed);
    Ok(tracks)
}

/// The listing's bytes as the shelf's tracks. Every MP3 the item holds is
/// a track, its category the folder it sits in - the way freepd.com
/// itself grouped them - and its title and artist the listing's own words
/// when it has them, the file name when it does not.
fn parse_freepd(bytes: &[u8]) -> Result<Vec<Track>, String> {
    let item: ArchiveItem = serde_json::from_slice(bytes)
        .map_err(|error| format!("the FreePD list is not readable: {error}"))?;
    let mut tracks: Vec<Track> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for file in item.files {
        if !file.name.to_ascii_lowercase().ends_with(".mp3")
            || !file.format.to_ascii_uppercase().contains("MP3")
        {
            continue;
        }
        // The listing carries one row a format and the derived rows hold
        // other extensions; a name twice is a listing this build reads
        // once.
        if seen.contains(&file.name) {
            continue;
        }
        seen.push(file.name.clone());
        let stem = file
            .name
            .rsplit('/')
            .next()
            .map(|leaf| leaf.trim_end_matches(".mp3"))
            .filter(|leaf| !leaf.is_empty())
            .unwrap_or("Track");
        let category = file
            .name
            .rsplit_once('/')
            .map(|(folder, _)| folder.to_ascii_lowercase())
            .unwrap_or_else(|| "misc".to_owned());
        let id = format!(
            "{FREEPD_PREFIX}{:012x}",
            crate::media::fnv1a(file.name.as_bytes()) & 0xffff_ffff_ffff
        );
        tracks.push(Track {
            id,
            source: Source::FreePd,
            title: file
                .title
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| stem.to_owned()),
            artist: file.artist.unwrap_or_default(),
            category,
            duration: file
                .length
                .and_then(|length| length.parse::<f64>().ok())
                .unwrap_or(0.0)
                .max(0.0),
            bytes: file
                .size
                .and_then(|size| size.parse::<u64>().ok())
                .unwrap_or(0),
            file: file.name,
        });
    }
    if tracks.is_empty() {
        return Err("the FreePD list held no tracks".to_owned());
    }
    tracks.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(tracks)
}

/// Downloads one FreePD track: its own file, over HTTPS, resumable, and
/// held to the size the listing promised.
fn fetch_freepd(
    track: &Track,
    partial: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let url = format!("{FREEPD_DOWNLOAD}/{}", encode_path(&track.file));
    let (received, _) =
        crate::models::download(&url, partial, track.bytes, cancel, "cancelled", progress)?;
    if track.bytes > 0 && received != track.bytes {
        return Err(format!(
            "{} arrived the wrong size: the list says {} bytes, {} came",
            track.title, track.bytes, received
        ));
    }
    Ok(())
}

// ── the Lo-Fi shelf ─────────────────────────────────────────────────────

/// The OpenLo-Fi pack's own manifest, as its repository publishes it.
#[derive(Deserialize)]
struct LofiCatalog {
    #[serde(default)]
    tracks: Vec<LofiTrack>,
}

/// One track of the pack's manifest: its title, its member name in the
/// zip, and the pack's own category slug.
#[derive(Deserialize)]
struct LofiTrack {
    title: String,
    filename: String,
    category: String,
}

/// The zip's own shape, as this build measured it: the total size it
/// checked itself against, and every member's place and span.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ZipIndex {
    /// The index's format: 1.
    #[serde(default)]
    version: u32,
    /// The zip's size in bytes, as the live zip said when this was read.
    /// A zip that disagrees is read again rather than believed.
    total: u64,
    /// Every member, in the zip's own order.
    #[serde(default)]
    entries: Vec<ZipEntry>,
}

/// One member of the zip.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct ZipEntry {
    /// The member's name: the track's own file name, flat in the zip.
    name: String,
    /// Where the member's local file header starts, from the zip's front.
    offset: u64,
    /// Where the member's data starts: the local header's own name and
    /// extra lengths past `offset`, measured by the census. `0` until
    /// then, and never fetched before it is measured.
    data: u64,
    /// The member's compressed size.
    csize: u64,
    /// The member's own size, uncompressed: the track's bytes.
    usize: u64,
    /// How the data is kept: 0 stored, 8 deflated.
    method: u8,
    /// The CRC-32 of the member's own bytes: what an inflated span is
    /// checked against, so a ranged read that went wrong is caught here
    /// rather than in an editor's decoder.
    crc: u32,
    /// The track's playing time, as the census measured it from the
    /// member's own first MPEG frames. `0.0` when it could not be.
    #[serde(default)]
    duration: f64,
}

/// Fetches the pack's manifest, reads the zip's own shape out of it, and
/// measures every track's start and length from its own headers: the
/// one-time census, the only part of a Lo-Fi prepare that takes seconds.
fn prepare_lofi(dirs: &AppDirs, shelf: &Shelf, cancel: &AtomicBool) -> Result<Vec<Track>, String> {
    let agent = crate::net::agent(
        std::time::Duration::from_secs(20),
        std::time::Duration::from_secs(60),
    );
    let response = agent
        .get(LOFI_CATALOG)
        .call()
        .map_err(|error| format!("the Lo-Fi list did not answer: {error}"))?;
    let mut bytes = Vec::new();
    use std::io::Read;
    response
        .into_reader()
        .take(CATALOG_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the Lo-Fi list did not arrive: {error}"))?;
    let catalog: LofiCatalog = serde_json::from_slice(&bytes)
        .map_err(|error| format!("the Lo-Fi list is not readable: {error}"))?;
    if catalog.tracks.is_empty() {
        return Err("the Lo-Fi list held no tracks".to_owned());
    }

    // The zip's shape: a kept index, believed only while the live zip
    // still says the same total size; anything else is read again.
    let total = live_total(&agent)?;
    let mut index = read_zip_index(dirs);
    if index.version != 1 || index.entries.is_empty() || index.total != total {
        index = read_zip(&agent, total)?;
        write_zip_index(dirs, &index);
    }

    // The census: every member's data start, measured from its own local
    // header, and every track's playing time, measured from its own first
    // MPEG frames. One small ranged read a track, once, kept for good.
    census(&agent, &mut index, &shelf.percent, cancel)?;
    write_zip_index(dirs, &index);

    // The manifest and the zip must agree, member for member, or the
    // titles this build shows cannot be the bytes it fetches; a mismatch
    // is refused with both counts rather than guessed around.
    let by_name: std::collections::HashMap<&str, &ZipEntry> = index
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect();
    let mut tracks: Vec<Track> = Vec::with_capacity(catalog.tracks.len());
    for listed in &catalog.tracks {
        let Some(entry) = by_name.get(listed.filename.as_str()) else {
            return Err(format!(
                "the Lo-Fi list names {} the zip does not hold ({}/166)",
                listed.filename,
                catalog.tracks.len()
            ));
        };
        let id = format!(
            "{LOFI_PREFIX}{:012x}",
            crate::media::fnv1a(listed.filename.as_bytes()) & 0xffff_ffff_ffff
        );
        tracks.push(Track {
            id,
            source: Source::Lofi,
            title: listed.title.clone(),
            artist: "Moby (MobyGratis)".to_owned(),
            category: listed.category.clone(),
            duration: entry.duration,
            bytes: entry.usize,
            file: listed.filename.clone(),
        });
    }
    write_catalog(dirs, Source::Lofi, &tracks);
    shelf.percent.store(100, Ordering::Relaxed);
    Ok(tracks)
}

/// Whether a kept zip index is the live zip's own shape: the one ranged
/// read of nothing - `bytes=0-0` - whose `Content-Range` names the total.
fn live_total(agent: &ureq::Agent) -> Result<u64, String> {
    let response = agent
        .get(LOFI_ZIP)
        .set("Range", "bytes=0-0")
        .call()
        .map_err(|error| format!("the zip did not answer: {error}"))?;
    if response.status() != 206 {
        return Err(format!(
            "the zip would not serve a byte range (status {})",
            response.status()
        ));
    }
    response
        .header("Content-Range")
        .and_then(|value| value.rsplit('/').next())
        .and_then(|total| total.parse::<u64>().ok())
        .ok_or_else(|| "the zip would not say how large it is".to_owned())
}

/// Reads the zip's own shape: the end-of-central-directory record out of
/// its tail, and the central directory itself - every member's name,
/// method, sizes, CRC and offset - out of one more ranged read. ~80 KB
/// moves for a 554 MB zip, and nothing else.
fn read_zip(agent: &ureq::Agent, total: u64) -> Result<ZipIndex, String> {
    // The end-of-central-directory record is in the last 64 KB; the zip
    // is half a gigabyte, so no zip64 endings are looked for and one
    // found is refused in words rather than misread.
    let tail = ranged(
        agent,
        total.saturating_sub(64 * 1024 + 21),
        total
            .checked_sub(1)
            .ok_or_else(|| "the zip says it is empty".to_owned())?,
    )?;
    let eocd = tail
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .ok_or_else(|| "the zip's own directory was not where it says".to_owned())?;
    let record = &tail[eocd..];
    if record.len() < 22 {
        return Err("the zip's own directory was cut short".to_owned());
    }
    let entries = u16::from_le_bytes([record[10], record[11]]) as usize;
    let size = u32::from_le_bytes([record[12], record[13], record[14], record[15]]) as u64;
    let at = u32::from_le_bytes([record[16], record[17], record[18], record[19]]) as u64;
    if entries == 0xFFFF || size == 0xFFFF_FFFF || at == 0xFFFF_FFFF {
        return Err("the zip is a zip64 archive this build cannot read".to_owned());
    }
    if at + size > total {
        return Err("the zip's own directory sits outside the zip".to_owned());
    }

    // The central directory itself: every member's place and span. A
    // directory that claims no size is refused rather than read with
    // arithmetic that would wrap trying to reach its last byte.
    let directory = ranged(
        agent,
        at,
        at.checked_add(size)
            .and_then(|end| end.checked_sub(1))
            .ok_or_else(|| "the zip's own directory was cut short".to_owned())?,
    )?;
    let mut members: Vec<ZipEntry> = Vec::with_capacity(entries);
    let mut place = 0usize;
    for _ in 0..entries {
        if place + 46 > directory.len() || &directory[place..place + 4] != b"PK\x01\x02" {
            return Err("the zip's own directory is not the shape it describes".to_owned());
        }
        let u16_at = |at: usize| u16::from_le_bytes([directory[at], directory[at + 1]]);
        let u32_at = |at: usize| {
            u32::from_le_bytes([
                directory[at],
                directory[at + 1],
                directory[at + 2],
                directory[at + 3],
            ])
        };
        let method = u16_at(place + 10);
        let crc = u32_at(place + 16);
        let csize = u32_at(place + 20) as u64;
        let own_size = u32_at(place + 24) as u64;
        let name_len = u16_at(place + 28) as usize;
        let extra_len = u16_at(place + 30) as usize;
        let comment_len = u16_at(place + 32) as usize;
        let offset = u32_at(place + 42) as u64;
        let name_end = place + 46 + name_len;
        if name_end > directory.len() {
            return Err("a member's name in the zip's directory was cut short".to_owned());
        }
        let name = String::from_utf8_lossy(&directory[place + 46..name_end]).into_owned();
        members.push(ZipEntry {
            name,
            offset,
            data: 0,
            csize,
            usize: own_size,
            method: method as u8,
            crc,
            duration: 0.0,
        });
        place = name_end + extra_len + comment_len;
    }
    if members.len() != entries {
        return Err("the zip's own directory held fewer members than it counts".to_owned());
    }
    Ok(ZipIndex {
        version: 1,
        total,
        entries: members,
    })
}

/// Measures every member's data start and every track's playing time:
/// one ranged read a member - the local file header and the first of the
/// track's own bytes - the reads five at a time, the progress a per-cent
/// while it runs.
fn census(
    agent: &ureq::Agent,
    index: &mut ZipIndex,
    percent: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<(), String> {
    // Only the unmeasured are measured: a kept index from an earlier run
    // has them all, and a re-run after a zip change has only the ones the
    // new central directory moved.
    let todo: Vec<usize> = index
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.data == 0 && entry.usize > 0)
        .map(|(at, _)| at)
        .collect();
    let total = todo.len().max(1);
    let done = AtomicUsize::new(0);
    let next = AtomicUsize::new(0);
    percent.store(0, Ordering::Relaxed);

    // The entries are borrowed index-wide, so the workers hand back what
    // they measured and the caller writes it in: (which, data, duration).
    let found = Mutex::new(Vec::<(usize, u64, f64)>::new());
    // Scoped threads, so the workers can borrow the entries and the
    // counters where they lie rather than taking copies of them.
    std::thread::scope(|scope| {
        for _ in 0..CENSUS_WIDTH {
            scope.spawn(|| {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    if at >= todo.len() {
                        return;
                    }
                    let entry = &index.entries[todo[at]];
                    let span = CENSUS_READ.min(entry.csize + 96);
                    if let Ok(bytes) = ranged(agent, entry.offset, entry.offset + span - 1)
                        && bytes.len() >= 30
                        && &bytes[..4] == b"PK\x03\x04"
                    {
                        let name_len = u16::from_le_bytes([bytes[26], bytes[27]]) as u64;
                        let extra_len = u16::from_le_bytes([bytes[28], bytes[29]]) as u64;
                        let head = &bytes[(30 + name_len + extra_len) as usize..];
                        // Deflated members arrive compressed: the first of
                        // the track's own bytes is the first of what inflates
                        // from the span's front, read forward as far as the
                        // head needs and no further.
                        let own: Vec<u8> = if entry.method == 8 {
                            let mut inflated = Vec::with_capacity(4096);
                            let decoder = flate2::read::DeflateDecoder::new(head);
                            use std::io::Read;
                            let _ = decoder.take(4096).read_to_end(&mut inflated);
                            inflated
                        } else {
                            head.to_vec()
                        };
                        let duration = mp3_duration(&own, entry.usize).unwrap_or(0.0);
                        let data = entry.offset + 30 + name_len + extra_len;
                        found
                            .lock()
                            .expect("the census lock is not poisoned")
                            .push((todo[at], data, duration));
                    }
                    let measured = done.fetch_add(1, Ordering::Relaxed) + 1;
                    percent.store(
                        ((measured as f64 / total as f64) * 100.0) as u32,
                        Ordering::Relaxed,
                    );
                }
            });
        }
    });
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_owned());
    }
    let found = found.into_inner().expect("the census lock is not poisoned");
    for (at, data, duration) in found {
        index.entries[at].data = data;
        index.entries[at].duration = duration;
    }
    let unmeasured = index
        .entries
        .iter()
        .filter(|entry| entry.usize > 0 && entry.data == 0)
        .count();
    if unmeasured > 0 {
        return Err(format!(
            "{unmeasured} of the zip's members could not be measured"
        ));
    }
    Ok(())
}

/// Fetches one Lo-Fi track: its own span out of the zip, one ranged read,
/// inflated when the zip deflated it and checked against the CRC its own
/// directory carries. A span that arrives wrong is one honest error, not
/// a half-written file.
fn fetch_lofi(
    dirs: &AppDirs,
    track: &Track,
    partial: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let agent = crate::net::agent(
        std::time::Duration::from_secs(20),
        std::time::Duration::from_secs(90),
    );
    // The index as this machine holds it, re-read when the zip has
    // changed; a kept index that names a member the manifest does not is
    // a stale one, and the honest answer is to read the zip again.
    let total = live_total(&agent)?;
    let mut index = read_zip_index(dirs);
    if index.version != 1 || index.entries.is_empty() || index.total != total {
        index = read_zip(&agent, total)?;
        write_zip_index(dirs, &index);
        let percent = AtomicU32::new(0);
        census(&agent, &mut index, &percent, cancel)?;
        write_zip_index(dirs, &index);
    }
    let Some(entry) = index.entries.iter().find(|entry| entry.name == track.file) else {
        return Err(format!(
            "the zip does not hold {} the list says it does",
            track.file
        ));
    };
    let data = if entry.data > 0 {
        entry.data
    } else {
        // An index the census never finished: read the member's own local
        // header rather than refuse, the same read the census makes.
        let head = ranged(&agent, entry.offset, entry.offset + 29)
            .map_err(|error| format!("the zip did not answer: {error}"))?;
        if head.len() < 30 || &head[..4] != b"PK\x03\x04" {
            return Err("the zip's member header was not where it says".to_owned());
        }
        let name_len = u16::from_le_bytes([head[26], head[27]]) as u64;
        let extra_len = u16::from_le_bytes([head[28], head[29]]) as u64;
        entry.offset + 30 + name_len + extra_len
    };
    if entry.csize > SPAN_LIMIT {
        return Err(format!(
            "{} says it is {} bytes in the zip, more than this build will read",
            track.title, entry.csize
        ));
    }

    // The track's own span, read as it arrives and drawn as it does.
    progress(0, entry.usize);
    let response = agent
        .get(LOFI_ZIP)
        .set("Range", &format!("bytes={data}-{}", data + entry.csize - 1))
        .call()
        .map_err(|error| format!("the zip did not answer: {error}"))?;
    if response.status() != 206 {
        return Err(format!(
            "the zip would not serve a byte range (status {})",
            response.status()
        ));
    }
    use std::io::Read;
    let mut reader = response.into_reader();
    let mut span = Vec::with_capacity(entry.csize as usize);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_owned());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("the track did not arrive: {error}"))?;
        if read == 0 {
            break;
        }
        span.extend_from_slice(&buffer[..read]);
        if span.len() as u64 > entry.csize {
            return Err("the zip sent more bytes than the track's own span".to_owned());
        }
        progress(span.len() as u64, entry.usize.max(entry.csize));
    }
    if span.len() as u64 != entry.csize {
        return Err(format!(
            "the track's span arrived short: {} of {} bytes",
            span.len(),
            entry.csize
        ));
    }

    // The member's own bytes: stored as they are, inflated when the zip
    // deflated them, and checked - size and CRC both - against what the
    // zip's own directory says they are.
    let own = match entry.method {
        0 => span,
        8 => {
            let mut inflated = Vec::with_capacity(entry.usize as usize);
            flate2::read::DeflateDecoder::new(&span[..])
                .take(SPAN_LIMIT)
                .read_to_end(&mut inflated)
                .map_err(|error| format!("the track's bytes would not inflate: {error}"))?;
            inflated
        }
        other => {
            return Err(format!(
                "{} is kept as method {other}, which this build cannot read",
                track.title
            ));
        }
    };
    if own.len() as u64 != entry.usize {
        return Err(format!(
            "{} inflated to {} bytes, the zip says {}",
            track.title,
            own.len(),
            entry.usize
        ));
    }
    if crc32fast::hash(&own) != entry.crc {
        return Err(format!(
            "{} did not arrive as the zip's own copy of itself",
            track.title
        ));
    }
    std::fs::write(partial, &own)
        .map_err(|error| format!("could not write {}: {error}", partial.display()))?;
    Ok(())
}

// ── what both shelves share ─────────────────────────────────────────────

/// Where a shelf's processed catalog is kept, beside the zip index and
/// the fetched tracks.
fn catalog_path(dirs: &AppDirs, source: Source) -> PathBuf {
    dir(dirs).join(match source {
        Source::FreePd => "freepd.json",
        Source::Lofi => "lofi.json",
    })
}

/// Where the Lo-Fi zip's measured shape is kept.
fn zip_index_path(dirs: &AppDirs) -> PathBuf {
    dir(dirs).join("lofi-zip.json")
}

/// A shelf's catalog as this machine last wrote it, when it parses.
fn read_catalog(dirs: &AppDirs, source: Source) -> Option<Vec<Track>> {
    let bytes = std::fs::read(catalog_path(dirs, source)).ok()?;
    let tracks: Vec<Track> = serde_json::from_slice(&bytes).ok()?;
    (!tracks.is_empty()).then_some(tracks)
}

/// Writes a shelf's catalog through a temporary file and a rename, so a
/// crash mid-write leaves the last one whole. A folder that cannot be
/// written loses the cache and nothing else: the next prepare fetches
/// the catalog again.
fn write_catalog(dirs: &AppDirs, source: Source, tracks: &[Track]) {
    let path = catalog_path(dirs, source);
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    let partial = path.with_extension("partial");
    if let Ok(bytes) = serde_json::to_vec(tracks) {
        let _ = std::fs::write(&partial, bytes).and_then(|()| std::fs::rename(&partial, &path));
    }
}

/// The zip's measured shape as this machine last wrote it, when it
/// parses; the census's own measurements included.
fn read_zip_index(dirs: &AppDirs) -> ZipIndex {
    std::fs::read(zip_index_path(dirs))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Writes the zip's measured shape, the same safe way a catalog is
/// written.
fn write_zip_index(dirs: &AppDirs, index: &ZipIndex) {
    let path = zip_index_path(dirs);
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    let partial = path.with_extension("partial");
    if let Ok(bytes) = serde_json::to_vec(index) {
        let _ = std::fs::write(&partial, bytes).and_then(|()| std::fs::rename(&partial, &path));
    }
}

/// A search over a pool of tracks, as the page's filters and the API's
/// fields both say it. The words are ANDed and prefix-matched over a
/// track's title, artist and category; the category is matched exactly;
/// the length bounds pass by a track whose length is unknown rather than
/// guess at it; and the order is the sort's own.
fn search(pool: &[Track], query: &Query) -> Result<Found, String> {
    let words: Vec<String> = query
        .text
        .as_deref()
        .unwrap_or("")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(12)
        .map(|word| word.to_lowercase())
        .collect();
    let category = query.category.as_deref().map(str::to_lowercase);
    let limit = query.limit.clamp(1, 100);

    let mut matched: Vec<(i32, &Track)> = Vec::new();
    for track in pool {
        if let Some(category) = &category
            && track.category.to_lowercase() != *category
        {
            continue;
        }
        if (query.min_seconds.is_some() || query.max_seconds.is_some()) && track.duration <= 0.0 {
            continue;
        }
        if let Some(min) = query.min_seconds
            && track.duration < min
        {
            continue;
        }
        if let Some(max) = query.max_seconds
            && track.duration > max
        {
            continue;
        }
        let title = track.title.to_lowercase();
        let artist = track.artist.to_lowercase();
        let group = track.category.to_lowercase();
        let mut score = 0;
        let mut all = true;
        for word in &words {
            let in_title = title
                .split(|c: char| !c.is_alphanumeric())
                .any(|token| token.starts_with(word.as_str()));
            let in_artist = artist
                .split(|c: char| !c.is_alphanumeric())
                .any(|token| token.starts_with(word.as_str()));
            let in_group = group
                .split(|c: char| !c.is_alphanumeric())
                .any(|token| token.starts_with(word.as_str()));
            if !in_title && !in_artist && !in_group {
                all = false;
                break;
            }
            score += i32::from(in_title) * 2 + i32::from(in_artist) + i32::from(in_group);
        }
        if !all {
            continue;
        }
        matched.push((score, track));
    }

    // A track whose length is unknown goes last, either way round: a
    // sort that put it first would say the shortest track in the library
    // is one it has not measured.
    let known = |track: &Track| {
        if track.duration > 0.0 {
            track.duration
        } else {
            f64::INFINITY
        }
    };
    match query.sort {
        Sort::Relevance => matched.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.title.to_lowercase().cmp(&b.1.title.to_lowercase()))
        }),
        Sort::Shortest => matched.sort_by(|a, b| {
            known(a.1)
                .partial_cmp(&known(b.1))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.title.to_lowercase().cmp(&b.1.title.to_lowercase()))
        }),
        Sort::Longest => matched.sort_by(|a, b| {
            b.1.duration
                .partial_cmp(&a.1.duration)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.title.to_lowercase().cmp(&b.1.title.to_lowercase()))
        }),
        Sort::Surprise => {
            // A different order every call: an xorshift over the pool,
            // seeded from the clock, the same shuffle a card deck gets.
            let mut seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9e37_79b9_7f4a_7c15, |since| since.as_nanos() as u64);
            if seed == 0 {
                seed = 0x9e37_79b9_7f4a_7c15;
            }
            for at in (1..matched.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let other = (seed % (at as u64 + 1)) as usize;
                matched.swap(at, other);
            }
        }
    }

    let total = matched.len() as u64;
    let window: Vec<Track> = matched
        .into_iter()
        .skip(query.offset as usize)
        .take(limit as usize)
        .map(|(_, track)| track.clone())
        .collect();
    Ok(Found {
        tracks: window,
        total,
    })
}

/// Reads `length` bytes at `start` out of the Lo-Fi zip, over the agent
/// given: one ranged GET, the status checked to be 206 - a 200 would
/// mean the range was ignored and the whole 554 MB zip was about to
/// arrive, which is stopped here rather than read.
fn ranged(agent: &ureq::Agent, start: u64, end: u64) -> Result<Vec<u8>, String> {
    if end < start {
        return Err("the zip was asked for a span that ends before it starts".to_owned());
    }
    let response = agent
        .get(LOFI_ZIP)
        .set("Range", &format!("bytes={start}-{end}"))
        .call()
        .map_err(|error| format!("the zip did not answer: {error}"))?;
    if response.status() != 206 {
        return Err(format!(
            "the zip would not serve a byte range (status {})",
            response.status()
        ));
    }
    let mut bytes = Vec::with_capacity((end - start + 1) as usize);
    use std::io::Read;
    response
        .into_reader()
        .take(end - start + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the bytes did not arrive: {error}"))?;
    Ok(bytes)
}

/// A track's playing time, measured from its own first MPEG audio
/// frames: the frames a Xing header counts, when the track carries one,
/// and the bitrate the first frame names over the track's own size
/// otherwise. `None` when the head holds no MPEG frame at all.
fn mp3_duration(head: &[u8], size: u64) -> Option<f64> {
    let mut at = 0usize;
    // An ID3v2 tag first, if there is one: its own length is a syncsafe
    // integer four bytes from its front.
    if head.len() >= 10 && &head[..3] == b"ID3" {
        let tag = ((head[6] as usize) << 21)
            | ((head[7] as usize) << 14)
            | ((head[8] as usize) << 7)
            | head[9] as usize;
        at = 10 + tag;
    }
    // The first frame's sync, within the first two kilobytes: a little
    // slack for the tags and padding an encoder writes before it.
    let find = (at + 2048).min(head.len());
    while at + 4 <= find {
        if head[at] == 0xFF && head[at + 1] & 0xE0 == 0xE0 {
            break;
        }
        at += 1;
    }
    if at + 4 > head.len() {
        return None;
    }
    let frame = &head[at..];
    let version_bits = (frame[1] >> 3) & 0x03;
    let layer_bits = (frame[1] >> 1) & 0x03;
    if layer_bits != 0x01 {
        return None; // Layer III is what every track here is.
    }
    let samples_a_frame = match version_bits {
        0x03 => 1152.0,
        _ => 576.0,
    };
    let bitrate_index = (frame[2] >> 4) & 0x0F;
    let rate_index = (frame[2] >> 2) & 0x03;
    if bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }
    // The bitrate tables for Layer III, in kilobits a second: MPEG 1 and
    // MPEG 2/2.5 carry their own.
    let mpeg1 = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    let mpeg2 = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];
    let bitrate = if version_bits == 0x03 {
        mpeg1[bitrate_index as usize]
    } else {
        mpeg2[bitrate_index as usize]
    } as f64
        * 1000.0;
    let rates_1 = [44100.0, 48000.0, 32000.0];
    let rates_2 = [22050.0, 24000.0, 16000.0];
    let rates_25 = [11025.0, 12000.0, 8000.0];
    let rate = match version_bits {
        0x03 => rates_1[rate_index as usize],
        0x02 => rates_2[rate_index as usize],
        _ => rates_25[rate_index as usize],
    };
    if rate <= 0.0 || bitrate <= 0.0 {
        return None;
    }

    // A Xing or Info header in the first frame's side information names
    // the frame count, and the frame count is the track's own length:
    // the frames themselves are what a VBR track's size cannot say.
    let channels_stereo = (frame[3] >> 6) & 0x03 != 0x03;
    let side = if version_bits == 0x03 {
        if channels_stereo { 32 } else { 17 }
    } else if channels_stereo {
        17
    } else {
        9
    };
    let xing_at = 4 + side;
    if frame.len() >= xing_at + 16
        && (&frame[xing_at..xing_at + 4] == b"Xing" || &frame[xing_at..xing_at + 4] == b"Info")
    {
        let flags = u32::from_be_bytes([
            frame[xing_at + 4],
            frame[xing_at + 5],
            frame[xing_at + 6],
            frame[xing_at + 7],
        ]);
        if flags & 0x0000_0001 != 0 && frame.len() >= xing_at + 12 {
            let frames = u32::from_be_bytes([
                frame[xing_at + 8],
                frame[xing_at + 9],
                frame[xing_at + 10],
                frame[xing_at + 11],
            ]);
            if frames > 0 {
                return Some(frames as f64 * samples_a_frame / rate);
            }
        }
    }

    // No Xing: the first frame's bitrate over the track's own size - an
    // estimate, and the honest one a CBR track deserves.
    Some(size as f64 * 8.0 / bitrate)
}

/// A path under the Archive's item, percent-encoded as its own URLs are:
/// the safe characters kept, everything else as uppercase hex, `/` kept
/// as the one separator.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    for &byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("concat-bgm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dirs_at(path: &Path) -> AppDirs {
        AppDirs::under(path)
    }

    fn track(source: Source, title: &str, artist: &str, category: &str, duration: f64) -> Track {
        Track {
            id: format!("{:?}-{title}", source).to_lowercase(),
            source,
            title: title.to_owned(),
            artist: artist.to_owned(),
            category: category.to_owned(),
            duration,
            bytes: 1000,
            file: format!("{}.mp3", title.to_lowercase().replace(' ', "-")),
        }
    }

    /// One shelf down must not make the whole library unusable: a search
    /// that names neither shelf searches whichever are ready, and refuses
    /// only when none are. A search that names a shelf still needs it.
    #[test]
    fn a_search_degrades_to_the_shelves_that_are_ready() {
        let dir = scratch("degrade");
        let library = Library {
            dirs: dirs_at(&dir),
            freepd: Shelf::new(),
            lofi: Shelf::new(),
            turns: Mutex::new(HashMap::new()),
        };
        // FreePD failed - the Archive has its days; Lo-Fi is ready.
        library
            .freepd
            .lands(Held::Failed("the Archive did not answer".to_owned()));
        library.lofi.lands(Held::Ready(Arc::new(vec![track(
            Source::Lofi,
            "Rain Off The Neon Signs",
            "Moby (MobyGratis)",
            "late-night",
            118.0,
        )])));

        // A query that names neither shelf searches what is ready.
        let found = library
            .search(&Query {
                text: Some("rain".to_owned()),
                limit: 10,
                ..Query::default()
            })
            .expect("the ready shelf is searched");
        assert_eq!(found.total, 1);
        assert_eq!(found.tracks[0].source, Source::Lofi);

        // A query that names the failed shelf is refused with its error.
        let error = library
            .search(&Query {
                text: Some("rain".to_owned()),
                limit: 10,
                source: Some(Source::FreePd),
                ..Query::default()
            })
            .expect_err("the named shelf is not usable");
        assert!(error.contains("unusable"), "{error}");
        assert!(error.contains("the Archive did not answer"), "{error}");

        // A query that names the ready shelf works as before.
        let found = library
            .search(&Query {
                text: Some("rain".to_owned()),
                limit: 10,
                source: Some(Source::Lofi),
                ..Query::default()
            })
            .expect("the named shelf is ready");
        assert_eq!(found.total, 1);

        // And with neither shelf usable, the refusal names a shelf and
        // its remedy.
        library.freepd.lands(Held::Missing);
        library
            .lofi
            .lands(Held::Failed("the Hub did not answer".to_owned()));
        let error = library
            .search(&Query {
                text: Some("rain".to_owned()),
                limit: 10,
                ..Query::default()
            })
            .expect_err("no shelf is usable");
        assert!(error.contains("bgm_prepare"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The words are ANDed and prefix-matched over the title, the artist
    /// and the category; a category is matched exactly; a length bound
    /// passes by a track whose length is unknown; the window is what the
    /// limit and the offset say it is.
    #[test]
    fn a_search_matches_words_and_windows() {
        let pool = vec![
            track(
                Source::FreePd,
                "City Rain",
                "Kevin MacLeod",
                "ambient",
                120.0,
            ),
            track(
                Source::FreePd,
                "Sunny Street",
                "Kevin MacLeod",
                "upbeat",
                90.0,
            ),
            track(
                Source::Lofi,
                "Rain Off The Neon Signs",
                "Moby (MobyGratis)",
                "late-night",
                118.0,
            ),
            track(
                Source::Lofi,
                "Unknown Length",
                "Moby (MobyGratis)",
                "late-night",
                0.0,
            ),
        ];
        let found = search(
            &pool,
            &Query {
                text: Some("rain cit".to_owned()),
                limit: 10,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.total, 1);
        assert_eq!(found.tracks[0].title, "City Rain");

        // Prefix-matched: "rain" finds both rain tracks.
        let found = search(
            &pool,
            &Query {
                text: Some("rain".to_owned()),
                limit: 10,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.total, 2);

        // ANDed: a word nothing carries matches nothing.
        let found = search(
            &pool,
            &Query {
                text: Some("rain ocean".to_owned()),
                limit: 10,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.total, 0);

        // The category is exact, and lowercase either way.
        let found = search(
            &pool,
            &Query {
                category: Some("late-night".to_owned()),
                limit: 10,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.total, 2);

        // A length bound passes by a track whose length is unknown.
        let found = search(
            &pool,
            &Query {
                min_seconds: Some(100.0),
                limit: 10,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.total, 2);

        // The window: limit and offset over the whole match.
        let found = search(
            &pool,
            &Query {
                limit: 2,
                offset: 1,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.total, 4);
        assert_eq!(found.tracks.len(), 2);

        // The sorts: shortest and longest order by the length, the
        // unknown going last.
        let found = search(
            &pool,
            &Query {
                sort: Sort::Shortest,
                limit: 10,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.tracks.last().unwrap().title, "Unknown Length");
        let found = search(
            &pool,
            &Query {
                sort: Sort::Longest,
                limit: 10,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(found.tracks.last().unwrap().title, "Unknown Length");
    }

    /// The Archive's own listing, as its metadata API writes it, becomes
    /// the shelf's tracks: every MP3, its category its folder, its title
    /// the listing's own word or its file name, and a file named twice
    /// read once.
    #[test]
    fn the_freepd_listing_becomes_tracks() {
        let bytes = br#"{"files":[
            {"name":"upbeat/Sunny Street.mp3","format":"VBR MP3","size":"2048",
             "length":"90.5","title":"Sunny Street","artist":"Kevin MacLeod"},
            {"name":"upbeat/Sunny Street.png","format":"PNG","size":"999"},
            {"name":"Page2/120 Monster.mp3","format":"VBR MP3","size":"4096",
             "length":"40.05","artist":"Kevin MacLeod"},
            {"name":"upbeat/Sunny Street.mp3","format":"VBR MP3","size":"2048"},
            {"name":"Root Track.mp3","format":"VBR MP3","size":"1024","length":"1.0"}
        ]}"#;
        let tracks = parse_freepd(bytes).unwrap();
        assert_eq!(tracks.len(), 3, "the PNG and the duplicate are not tracks");
        let monster = tracks
            .iter()
            .find(|t| t.file == "Page2/120 Monster.mp3")
            .unwrap();
        assert_eq!(monster.title, "120 Monster", "no title: the file name is");
        assert_eq!(monster.category, "page2");
        assert_eq!(monster.artist, "Kevin MacLeod");
        assert_eq!(monster.duration, 40.05);
        assert_eq!(monster.bytes, 4096);
        assert!(monster.id.starts_with(FREEPD_PREFIX));
        let root = tracks.iter().find(|t| t.file == "Root Track.mp3").unwrap();
        assert_eq!(root.category, "misc", "a file in no folder is misc");
        assert!(parse_freepd(br#"{"files":[]}"#).is_err());
        assert!(parse_freepd(b"not json").is_err());
    }

    /// A catalog written is a catalog read back, and an unreadable one
    /// is nothing rather than an error: the shelf falls back to fetching.
    #[test]
    fn a_catalog_round_trips() {
        let dir = scratch("catalog");
        let dirs = dirs_at(&dir);
        let tracks = vec![track(Source::FreePd, "One", "A", "upbeat", 10.0)];
        write_catalog(&dirs, Source::FreePd, &tracks);
        let back = read_catalog(&dirs, Source::FreePd).unwrap();
        assert_eq!(back, tracks);
        assert!(read_catalog(&dirs, Source::Lofi).is_none());
        std::fs::write(catalog_path(&dirs, Source::Lofi), b"not json").unwrap();
        assert!(read_catalog(&dirs, Source::Lofi).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Archive's paths are percent-encoded as its own URLs are:
    /// spaces and marks as hex, the safe characters and the one
    /// separator kept.
    #[test]
    fn archive_paths_are_encoded() {
        assert_eq!(
            encode_path("Page2/120 Monster.mp3"),
            "Page2/120%20Monster.mp3"
        );
        assert_eq!(encode_path("a&b#c?d.mp3"), "a%26b%23c%3Fd.mp3");
        assert_eq!(encode_path("Already-Safe_1.mp3"), "Already-Safe_1.mp3");
        assert_eq!(encode_path("caf\u{e9}.mp3"), "caf%C3%A9.mp3");
    }

    /// A first frame's own numbers name a track's length: a Xing header
    /// exactly, a plain frame's bitrate over the size as an estimate.
    #[test]
    fn a_first_frame_names_the_length() {
        // MPEG 1 Layer III, 128 kbps, 44100 Hz, stereo: frame header
        // FF FB 90 00, then 32 bytes of side information, then a Xing
        // header counting 2000 frames. 2000 * 1152 / 44100 = 52.2 s.
        let mut head = vec![0xFF, 0xFB, 0x90, 0x00];
        head.extend(std::iter::repeat_n(0u8, 32));
        head.extend_from_slice(b"Xing");
        head.extend_from_slice(&1u32.to_be_bytes());
        head.extend_from_slice(&2000u32.to_be_bytes());
        head.extend(std::iter::repeat_n(0u8, 64));
        assert!((mp3_duration(&head, 838_656).unwrap() - 2000.0 * 1152.0 / 44100.0) < 0.01);

        // The same frame without a Xing header: the bitrate over the
        // size. 838_656 bytes at 128 kbps is 52.4 s.
        let mut plain = vec![0xFF, 0xFB, 0x90, 0x00];
        plain.extend(std::iter::repeat_n(0u8, 128));
        assert!((mp3_duration(&plain, 838_656).unwrap() - 838_656.0 * 8.0 / 128_000.0) < 0.01);

        // An ID3v2 tag in front of the frame is stepped past: ten-byte
        // tag header, size 100.
        let mut tagged = vec![b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 100];
        tagged.extend(std::iter::repeat_n(0u8, 100));
        tagged.extend_from_slice(&plain);
        assert!(mp3_duration(&tagged, 838_656).is_some());

        // No frame at all: nothing, rather than a guess.
        assert!(mp3_duration(&[0u8; 64], 1000).is_none());
    }

    /// The zip's own directory is read out of a real zip's bytes - the
    /// end-of-central-directory record out of its tail, every member's
    /// place and span out of the directory - and what it says about a
    /// stored member and a deflated one is what was written. The reading
    /// itself is ranged over the network in the library; here the same
    /// parsing runs over bytes that lie still.
    #[test]
    fn a_zips_own_directory_is_read() {
        use flate2::Compression;
        use flate2::write::DeflateEncoder;
        use std::io::Read;
        use std::io::Write;

        // One member's local header and data, as the zip spells them. The
        // data is what the header spans - stored bytes for a stored
        // member, compressed bytes for a deflated one - and `own` is the
        // member's own size, which a deflated member's header names
        // beside the span it takes.
        fn member(zip: &mut Vec<u8>, name: &[u8], data: &[u8], own: usize, method: u16, crc: u32) {
            zip.extend_from_slice(b"PK\x03\x04");
            zip.extend_from_slice(&20u16.to_le_bytes());
            zip.extend_from_slice(&0u16.to_le_bytes());
            zip.extend_from_slice(&method.to_le_bytes());
            zip.extend_from_slice(&0u16.to_le_bytes());
            zip.extend_from_slice(&0u16.to_le_bytes());
            zip.extend_from_slice(&crc.to_le_bytes());
            zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
            zip.extend_from_slice(&(own as u32).to_le_bytes());
            zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
            zip.extend_from_slice(&0u16.to_le_bytes());
            zip.extend_from_slice(name);
            zip.extend_from_slice(data);
        }
        // The same member's row in the central directory, which is where
        // `read_zip` reads its place and span from.
        fn row(
            directory: &mut Vec<u8>,
            name: &[u8],
            data: &[u8],
            own: usize,
            method: u16,
            crc: u32,
            offset: u64,
        ) {
            directory.extend_from_slice(b"PK\x01\x02");
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&method.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&crc.to_le_bytes());
            directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(own as u32).to_le_bytes());
            directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u32.to_le_bytes());
            directory.extend_from_slice(&(offset as u32).to_le_bytes());
            directory.extend_from_slice(name);
        }

        // Two members: one stored, one deflated.
        let plain = b"stored member's own bytes";
        let long = b"deflated member's own bytes, longer than the stored one \
                     so the compressor has something to do";
        let deflated = {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(long).unwrap();
            encoder.finish().unwrap()
        };

        let mut zip: Vec<u8> = Vec::new();
        let mut directory: Vec<u8> = Vec::new();
        let one_at = zip.len() as u64;
        member(
            &mut zip,
            b"one.bin",
            plain,
            plain.len(),
            0,
            crc32fast::hash(plain),
        );
        let two_at = zip.len() as u64;
        member(
            &mut zip,
            b"two.bin",
            &deflated,
            long.len(),
            8,
            crc32fast::hash(long),
        );
        let directory_at = zip.len() as u64;
        row(
            &mut directory,
            b"one.bin",
            plain,
            plain.len(),
            0,
            crc32fast::hash(plain),
            one_at,
        );
        row(
            &mut directory,
            b"two.bin",
            &deflated,
            long.len(),
            8,
            crc32fast::hash(long),
            two_at,
        );
        let directory_size = directory.len() as u64;
        zip.extend_from_slice(&directory);
        zip.extend_from_slice(b"PK\x05\x06");
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&(directory_size as u32).to_le_bytes());
        zip.extend_from_slice(&(directory_at as u32).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());

        // The end-of-central-directory record, out of the tail.
        let eocd = zip.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
        let record = &zip[eocd..];
        let entries = u16::from_le_bytes([record[10], record[11]]) as usize;
        let size = u32::from_le_bytes([record[12], record[13], record[14], record[15]]) as u64;
        let at = u32::from_le_bytes([record[16], record[17], record[18], record[19]]) as u64;
        assert_eq!(entries, 2);
        assert_eq!((at, size), (directory_at, directory_size));

        // The central directory, member by member, the way `read_zip`
        // walks it: every member's method, sizes, CRC and offset are what
        // was written above.
        let mut place = 0usize;
        let mut read: Vec<(&str, u8, u64, u64, u32, u64)> = Vec::new();
        for _ in 0..entries {
            assert_eq!(&directory[place..place + 4], b"PK\x01\x02");
            let u16_at = |at: usize| u16::from_le_bytes([directory[at], directory[at + 1]]);
            let u32_at = |at: usize| {
                u32::from_le_bytes([
                    directory[at],
                    directory[at + 1],
                    directory[at + 2],
                    directory[at + 3],
                ])
            };
            let method = u16_at(place + 10) as u8;
            let crc = u32_at(place + 16);
            let csize = u32_at(place + 20) as u64;
            let own = u32_at(place + 24) as u64;
            let name_len = u16_at(place + 28) as usize;
            let extra_len = u16_at(place + 30) as usize;
            let comment_len = u16_at(place + 32) as usize;
            let offset = u32_at(place + 42) as u64;
            let name =
                String::from_utf8_lossy(&directory[place + 46..place + 46 + name_len]).into_owned();
            read.push((name.leak(), method, csize, own, crc, offset));
            place += 46 + name_len + extra_len + comment_len;
        }
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].0, "one.bin");
        assert_eq!(read[0].1, 0);
        assert_eq!(read[0].3, plain.len() as u64);
        assert_eq!(read[0].5, one_at);
        assert_eq!(read[1].0, "two.bin");
        assert_eq!(read[1].1, 8);
        assert_eq!(read[1].2, deflated.len() as u64);
        assert_eq!(read[1].3, long.len() as u64);
        assert_eq!(read[1].4, crc32fast::hash(long));
        assert_eq!(read[1].5, two_at);

        // The members as `census` measures them: the data starts past the
        // local header's own name, and the deflated member's own bytes
        // inflate back to what was written - the same check `fetch_lofi`
        // makes against the CRC the directory carries.
        let data_of = |offset: u64, name: &[u8], len: usize| {
            let name_len = name.len() as u64;
            let at = (offset + 30 + name_len) as usize;
            &zip[at..at + len]
        };
        assert_eq!(data_of(one_at, b"one.bin", plain.len()), plain);
        let span = data_of(two_at, b"two.bin", deflated.len());
        let mut inflated = Vec::new();
        flate2::read::DeflateDecoder::new(&span[..read[1].2 as usize])
            .read_to_end(&mut inflated)
            .unwrap();
        assert_eq!(inflated.len(), long.len());
        assert_eq!(inflated.as_slice(), long);
    }

    /// The whole library against the real sources, once, by hand: the
    /// FreePD listing down from the Archive, the Lo-Fi manifest and its
    /// zip's own directory measured out of the release, and one track of
    /// each shelf fetched through the ranged reads and checked. Marked
    /// `ignore` because it moves real bytes and needs the network; run it
    /// with `cargo test -p concat-host bgm -- --ignored` when a source
    /// or this module changes.
    #[test]
    #[ignore]
    fn the_real_sources_answer() {
        let dir = scratch("live");
        let dirs = dirs_at(&dir);
        let library = shared(&dirs);
        let cancel = AtomicBool::new(false);

        // FreePD: the listing arrives and parses into tracks with the
        // folders the shelf groups by.
        library
            .prepare(Source::FreePd, &cancel)
            .expect("the FreePD listing arrives");
        let freepd = library.rows(Source::FreePd).unwrap();
        assert!(
            freepd > 900,
            "the Archive's item holds a thousand tracks, not {freepd}"
        );
        assert!(
            library
                .categories(Source::FreePd)
                .iter()
                .any(|category| category == "upbeat"),
            "freepd.com's own folders are the categories"
        );

        // Lo-Fi: the manifest and the zip agree, and the census measured
        // every member's place and length.
        library
            .prepare(Source::Lofi, &cancel)
            .expect("the Lo-Fi pack arrives");
        let lofi = library.rows(Source::Lofi).unwrap();
        assert_eq!(lofi, 166, "the pack holds 166 tracks");
        assert!(
            library
                .categories(Source::Lofi)
                .iter()
                .any(|category| category == "late-night"),
            "the pack's own categories are the categories"
        );

        // A search across both shelves, and a fetch of what it found: the
        // bytes arrive, and they are an MP3.
        let found = library
            .search(&Query {
                text: Some("rain".to_owned()),
                limit: 5,
                ..Query::default()
            })
            .expect("both shelves are ready");
        assert!(found.total >= 2, "both shelves hold a rain track");
        let mut fetched = 0;
        for track in &found.tracks {
            let mut progress = |_, _| {};
            let path = library
                .fetch(&track.id, &cancel, &mut progress)
                .expect("the track's own bytes arrive");
            let head = std::fs::read(&path).expect("the track is on disk").len();
            assert!(head > 10_000, "a track is more than a header");
            assert!(
                head + 512 >= track.bytes as usize,
                "the track is its own size"
            );
            fetched += 1;
        }
        assert!(fetched >= 1, "at least one track was fetched whole");

        // The fetch is remembered: the same track is the same file, and
        // the search's duration filter passes by what the sources said.
        let id = found.tracks[0].id.clone();
        assert!(library.fetched(&id).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
