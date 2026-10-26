// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The OpenVid stock library: nine hundred thousand clips from OpenVid-1M,
//! searched in a local index, fetched one clip at a time.
//!
//! The dataset - [lance-format/openvid-lance] on Hugging Face - is 7.5 TB of
//! MP4 with a caption and quality signals beside every clip, published in
//! the Lance column format so a reader can take one column, or one row,
//! without touching the rest. Reading it that way from the Hub works, but
//! every query is a network round trip: a caption search runs 10-25 s from
//! a cold start, which is no way to browse.
//!
//! So the page's search runs against a **local index**: a single SQLite
//! file (~435 MB, six hundredths of one percent of the dataset) holding
//! every clip's caption, scores, camera motion, duration and - this is
//! what makes video fetching cheap - the byte range of each clip's MP4
//! inside the dataset's data files. The index is downloaded once, with a
//! progress bar and a checksum, and after that every search is a local
//! query: milliseconds, offline, no lag. Only the clips a person actually
//! picks cross the network, one HTTP range request each, straight from the
//! Hub's CDN.
//!
//! An orientation filter works the same honest way. The dataset's metadata
//! carries no frame size, so the first time a clip is shown, the end its
//! shelf puts the MP4's `moov` box on is fetched - the tail, except that
//! the WebVid shelf's files are faststart and carry it at the front - and
//! the real width and height read out of its `tkhd` box, into a side
//! table. A shelf that guesses wrong costs one more small read, once.
//! What the measurements say: the Pixabay, Pexels and CelebV shelves are
//! 512x512 squares (cut for text-to-video training) and the WebVid shelf
//! is 720p and 1080p landscape, so Square finds the one and Horizontal
//! the other - and each says so only because the clips' own headers do.
//! The first shape search also takes a census - one small sample of every
//! shelf, measured the same way and remembered - whose unanimous verdict
//! widens the filter to the shelf's unmeasured clips, so a person clicking
//! Horizontal sees the WebVid shelf at once rather than a clip at a time.
//! The census never overrules a measurement: a clip the `dims` table has
//! spoken for is exactly what the filter says it is, whatever its shelf
//! sampled as.
//!
//! Two doors lead here. The library pane's OpenVid page is the person's:
//! a search field, the filters, cards with captions and scores, a click to
//! preview, a plus to lay a clip at the playhead. The API's `openvid.*`
//! methods are the agent's, and reach the same index with the same filters
//! so an agent can pick clips by their numbers - aesthetic score, motion,
//! temporal consistency, camera motion, duration - without a window, then
//! import and place them in one call.
//!
//! [lance-format/openvid-lance]: https://huggingface.co/datasets/lance-format/openvid-lance

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use rusqlite::Connection;

use crate::AppDirs;

/// The captions in the index are zstd-compressed against a dictionary the
/// index carries in its own `meta` table (trained on the captions
/// themselves, which is what turns 680 MB of prose into 164 MB). Kept here
/// once opened, beside the connection it belongs to; a page of forty cards
/// decompresses in well under a millisecond.
struct Index {
    db: Connection,
    dict: Vec<u8>,
}

/// Where the index is published: a release asset on this repository, so the
/// download needs no account and no token. The digest beside it is baked
/// into this build; a new index is a new release tag and a new constant,
/// and a file whose version disagrees is re-downloaded.
pub const INDEX_URL: &str = "https://github.com/coreselfmastery/FUNCUT/releases/download/openvid-index-v1/openvid-index.sqlite";

/// The index file's SHA-256, lowercase hex. A download that does not match
/// is deleted, not installed.
pub const INDEX_SHA256: &str = "9d3e83f190bcab9906910fa98b901d74ec9944674a891797d50a731d28682c09";

/// The index layout this build reads. The index's own `meta` table carries
/// the same number; a disagreement is a re-download, not a guess.
pub const INDEX_VERSION: i64 = 1;

/// The dataset every byte range in the index points into. Clips are read
/// with one ranged GET each: `…/data/train.lance/data/<file>`, `Range:
/// bytes=<pos>-<pos+size-1>`. The Hub answers 206 from its CDN, so a clip
/// costs its own size in bandwidth and nothing else.
pub const DATASET_URL: &str =
    "https://huggingface.co/datasets/lance-format/openvid-lance/resolve/main/data/train.lance/data";

/// How large a trailer to read for a clip's frame size, at most. The MP4s
/// in this dataset put their `moov` box - the one that names the width and
/// height - at one end of the file or the other, and the largest one seen
/// in a thousand clips was 26 KB; 64 KB covers it with room, and a clip
/// whose box still does not fit is simply one whose size stays unknown.
const TRAILER: u64 = 64 * 1024;

/// How many clips a shelf's census samples: enough that a unanimous sample
/// speaks for the shelf, few enough that the once-per-machine cost is a few
/// seconds. A sample can be unlucky - which is why the verdict only ever
/// widens a filter to the shelf's *unmeasured* clips, and a clip its own
/// header has spoken for always wins over its shelf's sample. A sample of
/// eight or fewer, or one that disagreed with itself, speaks for nothing.
const CENSUS: i64 = 16;

/// How many clips the census measures at once. Each read is one network
/// round trip, so six at a time is the difference between a few seconds
/// and a few tens of them; the connection each thread keeps warm is the
/// other difference.
const CENSUS_WIDTH: usize = 6;

/// Where the page's own files live: `<data>/openvid/`, the index beside the
/// clips. `data` is large state by the app's convention, which is right for
/// both.
pub fn dir(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("openvid")
}

/// Where the index is kept.
fn index_path(dirs: &AppDirs) -> PathBuf {
    dir(dirs).join("openvid-index.sqlite")
}

/// Where a clip is kept once fetched: `clips/<id>.mp4`. The id is the
/// dataset's row id, stable for the index's version, so a clip fetched
/// twice is fetched once.
fn clip_path(dirs: &AppDirs, id: i64) -> PathBuf {
    dir(dirs).join("clips").join(format!("{id}.mp4"))
}

/// One clip of the library, as a search result carries it and as the API
/// reports it. Everything here comes from the local index; the frame size
/// is Option because it is learned, not published.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    /// The clip's id in the dataset, and the id every verb takes back.
    pub id: i64,
    /// The file's name: `pixabay_auroras_videos_90877_000.mp4`'s stem,
    /// `pixabay_auroras_videos_90877_000`.
    pub name: String,
    /// What the clip shows, in the dataset's own words.
    pub caption: String,
    /// Visual quality, 0-6, higher better.
    pub aesthetic: f64,
    /// How much moves, higher more.
    pub motion: f64,
    /// Frame-to-frame stability, 0-1, higher steadier.
    pub temporal: f64,
    /// `static`, `zoom_in`, `pan_left+tilt_up`, … .
    pub camera: String,
    /// Frames a second.
    pub fps: f64,
    /// Seconds.
    pub seconds: f64,
    /// The MP4's size, as it will cost to fetch.
    pub bytes: u64,
    /// Which shelf the clip came from: Pixabay, Pexels or CelebV, as the
    /// file's name says.
    pub source: Source,
    /// Width and height once read from the clip's own header, and not
    /// before. `None` is "not looked yet", never "unknown forever".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dims: Option<(u32, u32)>,
}

/// The shelves the dataset was cut from, read out of each file's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// The Pixabay shelf.
    Pixabay,
    /// The Pexels shelf.
    Pexels,
    /// The CelebV shelf: people, talking heads, faces.
    CelebV,
    /// Everything else the file names do not say.
    Other,
}

impl Source {
    /// The shelf a file name puts a clip on.
    fn of(name: &str) -> Source {
        if name.starts_with("pixabay_") {
            Source::Pixabay
        } else if name.starts_with("pexels_") {
            Source::Pexels
        } else if name.starts_with("celebv_") {
            Source::CelebV
        } else {
            Source::Other
        }
    }

    /// The number the index stores for this shelf, so the Rust name and
    /// the SQL column cannot drift apart.
    fn number(self) -> i64 {
        match self {
            Source::Pixabay => 0,
            Source::Pexels => 1,
            Source::CelebV => 2,
            Source::Other => 3,
        }
    }

    /// The shelf a stored number is, as [`Source::number`] writes it. The
    /// index only ever holds the four, so anything else is the last of
    /// them rather than an error the reader cannot act on.
    fn of_number(number: i64) -> Source {
        match number {
            0 => Source::Pixabay,
            1 => Source::Pexels,
            2 => Source::CelebV,
            _ => Source::Other,
        }
    }
}

/// What a search asks for. Every field is optional; an all-`None` search is
/// "show me clips", the library as it comes. The same type serves the
/// window's page and the API's `openvid.search`, so the two cannot disagree
/// about what a filter means.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Words to find in captions. Terms are ANDed and match prefixes:
    /// `ocean sunset` finds captions holding both a word starting "ocean"
    /// and one starting "sunset", ranked by relevance.
    pub text: Option<String>,
    /// Aesthetic score at least this.
    pub min_aesthetic: Option<f64>,
    /// Motion score at least this.
    pub min_motion: Option<f64>,
    /// Motion score at most this.
    pub max_motion: Option<f64>,
    /// Temporal consistency at least this.
    pub min_temporal: Option<f64>,
    /// Seconds at least this.
    pub min_seconds: Option<f64>,
    /// Seconds at most this.
    pub max_seconds: Option<f64>,
    /// Frames a second at least this.
    pub min_fps: Option<f64>,
    /// Only this camera motion, exactly as the dataset names it: `static`,
    /// `zoom_in`, `pan_left+tilt_up`, … .
    pub camera: Option<String>,
    /// Only this shelf.
    pub source: Option<Source>,
    /// Only clips whose measured frame size is this shape. Clips not yet
    /// measured are left out when this is set - a filter cannot honour a
    /// measurement nobody has made - so the page says how many are measured
    /// beside it.
    pub orientation: Option<Orientation>,
    /// How to order what is found; [`Sort::Relevance`] needs `text`.
    pub sort: Sort,
    /// At most this many, from the front of the order.
    pub limit: u32,
    /// Skip this many from the front, for the next page.
    pub offset: u32,
}

/// A frame's shape, as the orientation filter asks for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    /// Taller than wide, by five per cent or more.
    Vertical,
    /// Wider than tall, by five per cent or more.
    Horizontal,
    /// Neither: within five per cent either way.
    Square,
}

impl Orientation {
    /// The shape a frame size is. Five per cent either way is the slack
    /// that lets 512x512 count as square while 544x544 with a stray crop
    /// still does. The search asks the same question of the measured
    /// sizes in SQL - [`Orientation::sql`], which spells the slack out
    /// beside it - and the census asks it of every sample it measures, so
    /// the one definition serves the badge, the filter and the census.
    fn of((w, h): (u32, u32)) -> Orientation {
        if h as f64 > w as f64 * 1.05 {
            Orientation::Vertical
        } else if w as f64 > h as f64 * 1.05 {
            Orientation::Horizontal
        } else {
            Orientation::Square
        }
    }

    /// The shape a search asks of the measured sizes in `dims`, in SQL:
    /// the same five-per-cent slack [`Orientation::of`] gives, spelled
    /// where the query can use it. Kept beside `of` so the two cannot
    /// drift apart.
    fn sql(self) -> &'static str {
        match self {
            Orientation::Vertical => "dims.h > dims.w * 1.05",
            Orientation::Horizontal => "dims.w > dims.h * 1.05",
            Orientation::Square => "dims.h <= dims.w * 1.05 AND dims.w <= dims.h * 1.05",
        }
    }

    /// The word the census table stores a shelf's verdict as.
    fn word(self) -> &'static str {
        match self {
            Orientation::Vertical => "vertical",
            Orientation::Horizontal => "horizontal",
            Orientation::Square => "square",
        }
    }
}

/// How a search's finds are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// By how well the caption matches, best first. The default when there
    /// are words to match.
    #[default]
    Relevance,
    /// Highest aesthetic score first.
    Aesthetic,
    /// Most motion first.
    Motion,
    /// Steadiest first.
    Temporal,
    /// Shortest first.
    Shortest,
    /// Longest first.
    Longest,
    /// A different order every time, for when nothing is being looked for
    /// in particular.
    Surprise,
}

/// What a search found: the clips, in the order asked for, and how many
/// there are in all - the count so the page can say "1,234 clips" without
/// fetching a thousand cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The clips, `limit` of them from `offset` in the order.
    pub clips: Vec<Clip>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// The whole library in one place: its state, its index connection and the
/// clips on disk. One per process, reached through [`shared`], because the
/// window and its embedded API server are one process and a download
/// started twice would race itself; a *separate* process - the headless MCP
/// server - gets its own instance, and the two are safe together because
/// the index is only ever replaced by an atomic rename and every write goes
/// through SQLite's own locking.
pub struct Library {
    dirs: AppDirs,
    /// The index connection and its caption dictionary, once the index is
    /// on disk. Opened once and kept: opening a connection per search would
    /// re-read the schema and the FTS dictionaries every keystroke.
    db: OnceLock<Mutex<Index>>,
    /// Where the index download stands, as a per-cent 0-100. An atomic
    /// rather than the state mutex, so the download's worker can tick it
    /// without taking a lock the page is reading.
    progress: AtomicU32,
    /// Set while a download is running in *this* process, so a second
    /// `prepare` - from the pane and the window's API server at once,
    /// say - cannot start a second one. Across processes, the partial
    /// file's name carries the pid, so two downloaders never write the
    /// same file either.
    running: AtomicBool,
}

impl Library {
    /// The library's state as the page and the API report it. Cheap: a
    /// stat and a lock check, no I/O to speak of, so the page can poll it
    /// every second while a download runs.
    pub fn status(&self) -> Status {
        if self.running.load(Ordering::Relaxed) {
            return Status::Downloading(self.progress.load(Ordering::Relaxed));
        }
        if self.db.get().is_some() {
            return Status::Ready;
        }
        if !index_path(&self.dirs).is_file() {
            return Status::Missing;
        }
        // On disk but not open yet: open now, and report what that said. A
        // file that will not open is a real condition - a truncated
        // download wearing the finished file's name - not one to quietly
        // call "missing".
        match self.open_index() {
            Ok(()) => Status::Ready,
            Err(error) => Status::Failed(error),
        }
    }

    /// How many clips the index holds, when it is ready. The count is in
    /// the index's own `meta` table, written when it was built, so the page
    /// does not `COUNT(*)` nine hundred thousand rows to say "938k clips"
    /// beside the search field.
    pub fn rows(&self) -> Option<u64> {
        let index = self.db.get()?.lock().ok()?;
        index
            .db
            .query_row("SELECT value FROM meta WHERE key = 'rows'", [], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .ok()
            .and_then(|value| String::from_utf8(value).ok()?.parse().ok())
    }

    /// How many clips the page has measured the frame size of so far, for
    /// the honest number beside the orientation filter.
    pub fn measured(&self) -> u64 {
        let Some(index) = self.db.get() else { return 0 };
        let Ok(index) = index.lock() else { return 0 };
        index
            .db
            .query_row("SELECT count(*) FROM dims", [], |row| {
                row.get::<_, i64>(0).map(|count| count.max(0) as u64)
            })
            .unwrap_or(0)
    }

    /// Brings the index in: downloaded from the release, checked against
    /// [`INDEX_SHA256`], and renamed into place only when the digest
    /// matches. Blocks - run it on a worker - and reports its per-cent
    /// through the status the page is already polling. A download already
    /// running in this process is not an error, just nothing to do.
    pub fn prepare(&self, cancel: &AtomicBool) -> Result<(), String> {
        if index_path(&self.dirs).is_file() {
            // Present, but perhaps not openable. A version this build
            // cannot read is re-downloaded rather than argued with -
            // the module's own promise - so an index left behind by an
            // older build heals itself instead of failing forever over
            // a file nothing will ever open. Anything else that will
            // not open is the caller's status to say.
            match self.open_index() {
                Ok(()) => return Ok(()),
                Err(why) if why.contains("this build reads") => {
                    let _ = std::fs::remove_file(index_path(&self.dirs));
                }
                Err(why) => return Err(why),
            }
        }
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(()); // One is already running; its progress is the
            // one to watch.
        }
        let outcome = self.download_index(cancel);
        self.running.store(false, Ordering::SeqCst);
        self.progress.store(0, Ordering::Relaxed);
        outcome
    }

    /// The download itself, in the shape [`crate::models::download`] gives:
    /// a partial file resumed by range, a read timeout, and a cancel flag a
    /// closed window can set.
    fn download_index(&self, cancel: &AtomicBool) -> Result<(), String> {
        let target = index_path(&self.dirs);
        let folder = target.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
        // The partial file carries this process's pid, so the window and a
        // headless agent fetching at the same moment write two files and
        // the first to finish renames over the second, rather than both
        // appending to one corrupt thing.
        let name = target.file_name().map_or_else(
            || "openvid-index.sqlite".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let partial = folder.join(format!("{name}.partial-{}", std::process::id()));
        let progress = &self.progress;
        let mut report = move |done: u64, total: u64| {
            let percent = if total > 0 {
                ((done as f64 / total as f64) * 100.0).round() as u32
            } else {
                0
            };
            progress.store(percent.min(100), Ordering::Relaxed);
        };
        // A cancelled or interrupted download leaves its partial in place:
        // the next attempt in this process picks up from it by range, the
        // way `models::download` is built to. Only bytes that fail the
        // digest - deleted inside - are not worth resuming.
        (|| {
            crate::models::download(
                INDEX_URL,
                &partial,
                470_000_000,
                cancel,
                "cancelled",
                &mut report,
            )
            .map_err(|error| format!("the OpenVid index could not be fetched: {error}"))?;
            match crate::models::verify(&partial, INDEX_SHA256) {
                Ok(()) => {}
                Err(error) => {
                    // Bytes that fail the digest are not a partial worth
                    // resuming: they are deleted, not kept for a retry
                    // that would only fail the same way.
                    let _ = std::fs::remove_file(&partial);
                    return Err(format!("the OpenVid index did not verify: {error}"));
                }
            }
            std::fs::rename(&partial, &target)
                .map_err(|error| format!("could not keep {}: {error}", target.display()))
        })()
    }

    /// Opens the index, or says why not. The connection is kept in WAL mode
    /// so the frame sizes the page learns as it browses can be written
    /// without ever blocking a search, and the caption dictionary is read
    /// out of the index's own `meta` table so the two cannot be parted.
    fn open_index(&self) -> Result<(), String> {
        if self.db.get().is_some() {
            return Ok(());
        }
        let open = |what: &str| format!("the OpenVid index would not open: {what}");
        let connection =
            Connection::open(index_path(&self.dirs)).map_err(|error| open(&error.to_string()))?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| open(&error.to_string()))?;
        let meta = |key: &str| -> Result<Vec<u8>, String> {
            connection
                .query_row("SELECT value FROM meta WHERE key = ?", [key], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .map_err(|error| open(&error.to_string()))
        };
        let version = String::from_utf8_lossy(&meta("version")?).to_string();
        if version.parse::<i64>().ok() != Some(INDEX_VERSION) {
            return Err(open(&format!(
                "it is version {version}; this build reads {INDEX_VERSION}"
            )));
        }
        let dict = meta("captions-dict")?;
        // The side table of frame sizes, written by this app as it browses
        // rather than shipped in the index: it is learned, not published.
        // The census table beside it is learned too: one sample of each
        // shelf, taken the first time a shape filter is asked for, whose
        // unanimous verdict widens that filter to the shelf's unmeasured
        // clips - never past a clip the `dims` table has spoken for.
        connection
            .execute(
                "CREATE TABLE IF NOT EXISTS dims (
                     id INTEGER PRIMARY KEY,
                     w INTEGER NOT NULL,
                     h INTEGER NOT NULL
                 )",
                [],
            )
            .map_err(|error| open(&error.to_string()))?;
        connection
            .execute(
                "CREATE TABLE IF NOT EXISTS census (
                     source INTEGER PRIMARY KEY,
                     shape TEXT NOT NULL,
                     agreed INTEGER NOT NULL,
                     sample INTEGER NOT NULL
                 )",
                [],
            )
            .map_err(|error| open(&error.to_string()))?;
        let _ = self.db.set(Mutex::new(Index {
            db: connection,
            dict,
        }));
        Ok(())
    }

    /// Runs a query against the index. Every filter is optional and every
    /// bound is a parameter, never interpolated: the words a person types
    /// are data to the search, not SQL.
    pub fn search(&self, query: &Query) -> Result<Found, String> {
        self.open_index()?;

        // A shape filter asks something the index does not publish - a
        // clip's frame size - so the first one takes the census: one
        // sample of each shelf, measured and remembered, after which the
        // shelf's unanimous verdict widens the filter to its unmeasured
        // clips and the search answers from then on without the network.
        // The census never fails a search: a shelf that would not be
        // measured is a shelf the filter simply does not speak for.
        if query.orientation.is_some() {
            self.take_census();
        }
        // The shelves a shape filter may speak for, read before the
        // search's own lock on the index: `censused` takes the same
        // mutex, and a search that waited on itself would wait forever.
        let censused: Vec<i64> = query
            .orientation
            .map_or_else(Vec::new, |shape| self.censused(shape));

        let held = self
            .db
            .get()
            .and_then(|db| db.lock().ok())
            .ok_or_else(|| "the OpenVid index is busy".to_owned())?;
        let index = &*held;
        let db = &index.db;

        let words = query
            .text
            .as_deref()
            .map(fts_words)
            .filter(|words| !words.is_empty());
        let mut where_parts: Vec<String> = Vec::new();
        let mut binds: Vec<Bind> = Vec::new();
        // The caption search, when there is one, is the first part and the
        // first bind: the FTS table is what the count and the page both
        // start from, and its `rank` is what orders a relevance sort.
        let from = if let Some(words) = words.as_deref() {
            where_parts.push("clips_fts MATCH ?".to_owned());
            binds.push(Bind::Text(words.to_owned()));
            "clips_fts JOIN clips ON clips.id = clips_fts.rowid"
        } else {
            "clips"
        };
        // A shape filter reaches past the measured clips to the censused
        // shelves' unmeasured ones, so it is a LEFT JOIN in the queries
        // and the shape question itself lives in the WHERE below, asked
        // of both at once. Without a shape, the join is the cards'
        // business alone.
        let from_shown = format!("{from} LEFT JOIN dims ON dims.id = clips.id");
        let from_filtered = if query.orientation.is_some() {
            from_shown.clone()
        } else {
            from.to_owned()
        };
        // Every scalar filter, in a fixed order, each a bound parameter.
        if let Some(value) = query.min_aesthetic {
            where_parts.push("clips.aesthetic >= ?".to_owned());
            binds.push(Bind::Real(value));
        }
        if let Some(value) = query.min_motion {
            where_parts.push("clips.motion >= ?".to_owned());
            binds.push(Bind::Real(value));
        }
        if let Some(value) = query.max_motion {
            where_parts.push("clips.motion <= ?".to_owned());
            binds.push(Bind::Real(value));
        }
        if let Some(value) = query.min_temporal {
            where_parts.push("clips.temporal >= ?".to_owned());
            binds.push(Bind::Real(value));
        }
        if let Some(value) = query.min_seconds {
            where_parts.push("clips.seconds >= ?".to_owned());
            binds.push(Bind::Real(value));
        }
        if let Some(value) = query.max_seconds {
            where_parts.push("clips.seconds <= ?".to_owned());
            binds.push(Bind::Real(value));
        }
        if let Some(value) = query.min_fps {
            where_parts.push("clips.fps >= ?".to_owned());
            binds.push(Bind::Real(value));
        }
        if let Some(value) = query.camera.as_deref() {
            where_parts.push("clips.camera = ?".to_owned());
            binds.push(Bind::Text(value.to_owned()));
        }
        if let Some(source) = query.source {
            where_parts.push("clips.source = ?".to_owned());
            binds.push(Bind::Integer(source.number()));
        }
        if let Some(orientation) = query.orientation {
            let (predicate, shelf_binds) = orientation_predicate(orientation, &censused);
            where_parts.push(predicate);
            binds.extend(shelf_binds);
        }

        let where_sql = if where_parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", where_parts.join(" AND "))
        };
        // The count first: same joins, same WHERE, no clip rows read, so
        // "1,234 clips" costs one index pass.
        let count_sql = format!("SELECT count(*) FROM {from_filtered}{where_sql}");
        let total: u64 = db
            .query_row(
                &count_sql,
                rusqlite::params_from_iter(binds.iter().map(Bind::value)),
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| format!("the OpenVid index would not search: {error}"))?
            as u64;

        let order = match (query.sort, words.is_some()) {
            (Sort::Relevance, true) => " ORDER BY clips_fts.rank",
            (Sort::Relevance, false) => " ORDER BY clips.id",
            (Sort::Aesthetic, _) => " ORDER BY clips.aesthetic DESC, clips.id",
            (Sort::Motion, _) => " ORDER BY clips.motion DESC, clips.id",
            (Sort::Temporal, _) => " ORDER BY clips.temporal DESC, clips.id",
            (Sort::Shortest, _) => " ORDER BY clips.seconds, clips.id",
            (Sort::Longest, _) => " ORDER BY clips.seconds DESC, clips.id",
            (Sort::Surprise, _) => " ORDER BY random()",
        };
        let page_sql = format!(
            "SELECT clips.id, clips.path, clips.caption, clips.aesthetic, clips.motion, \
             clips.temporal, clips.camera, clips.fps, clips.seconds, clips.size, \
             dims.w, dims.h \
             FROM {from_shown}{where_sql}{order} \
             LIMIT ? OFFSET ?"
        );
        let mut all_binds: Vec<Bind> = binds;
        all_binds.push(Bind::Integer(query.limit as i64));
        all_binds.push(Bind::Integer(query.offset as i64));
        let mut statement = db
            .prepare(&page_sql)
            .map_err(|error| format!("the OpenVid index would not search: {error}"))?;
        let clips = statement
            .query_map(
                rusqlite::params_from_iter(all_binds.iter().map(Bind::value)),
                |row| clip_of_row(row, &index.dict),
            )
            .map_err(|error| format!("the OpenVid index would not search: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("the OpenVid index would not search: {error}"))?;
        Ok(Found { clips, total })
    }

    /// One clip by id, with its frame size if it is already measured.
    /// The search results' Clip, again, for a card the page is re-drawing
    /// or an agent following a hunch.
    pub fn clip(&self, id: i64) -> Result<Option<Clip>, String> {
        self.open_index()?;
        let held = self
            .db
            .get()
            .and_then(|db| db.lock().ok())
            .ok_or_else(|| "the OpenVid index is busy".to_owned())?;
        let index = &*held;
        let mut statement = index
            .db
            .prepare(
                "SELECT clips.id, clips.path, clips.caption, clips.aesthetic, clips.motion, \
                 clips.temporal, clips.camera, clips.fps, clips.seconds, clips.size, \
                 dims.w, dims.h \
                 FROM clips LEFT JOIN dims ON dims.id = clips.id WHERE clips.id = ?",
            )
            .map_err(|error| format!("the OpenVid index would not open: {error}"))?;
        let mut rows = statement
            .query([id])
            .map_err(|error| format!("the OpenVid index would not open: {error}"))?;
        let Some(row) = rows
            .next()
            .map_err(|error| format!("the OpenVid index would not open: {error}"))?
        else {
            return Ok(None);
        };
        Ok(Some(clip_of_row(row, &index.dict).map_err(|error| {
            format!("the OpenVid index would not open: {error}")
        })?))
    }

    /// A clip's frame size, fetching the MP4's trailer and reading it if
    /// this is the first ask. Blocks on one small network read - the page
    /// calls it for a page of cards at a time, off the window's thread -
    /// and remembers the answer in the index's `dims` side table, so every
    /// ask after the first is local. `Ok(None)` is "could not be read":
    /// a trailer too large or a header this parser does not know, neither
    /// worth an error to the page.
    pub fn dims(&self, id: i64) -> Result<Option<(u32, u32)>, String> {
        if let Some(known) = self.dims_known(id)? {
            return Ok(Some(known));
        }
        // A clip already on disk is read from the disk: the ends are
        // the same bytes the Hub would send, without the network - and
        // only the ends, not the whole clip.
        let read = match self.dims_of_fetched(id) {
            Some(dims) => Some(dims),
            None => {
                let range = self.clip_range(id)?;
                let Some((file, pos, size, source)) = range else {
                    return Ok(None);
                };
                let agent = crate::net::agent(
                    std::time::Duration::from_secs(20),
                    std::time::Duration::from_secs(60),
                );
                self.read_dims(&file, pos, size, source, &agent)?
            }
        };
        match read {
            Some(dims) => {
                self.remember_dims(id, dims)?;
                Ok(Some(dims))
            }
            None => Ok(None),
        }
    }

    /// A fetched clip's frame size, read from its own bytes on disk: the
    /// tail first, then the head when the tail says nothing, because a
    /// faststart file carries its `moov` at the front. `None` when the
    /// clip is not on disk or neither end says anything this parser
    /// knows. Reading a local file costs nothing, so both ends are read
    /// without asking the shelf which one to try first.
    fn dims_of_fetched(&self, id: i64) -> Option<(u32, u32)> {
        use std::io::Seek;
        let path = self.fetched(id)?;
        let mut file = std::fs::File::open(path).ok()?;
        let end = file.metadata().ok()?.len();
        let length = end.min(TRAILER);
        file.seek(std::io::SeekFrom::Start(end - length)).ok()?;
        let mut trailer = vec![0u8; length as usize];
        std::io::Read::read_exact(&mut file, &mut trailer).ok()?;
        if let Some(dims) = frame_size_of(&trailer) {
            return Some(dims);
        }
        file.seek(std::io::SeekFrom::Start(0)).ok()?;
        let mut head = vec![0u8; length as usize];
        std::io::Read::read_exact(&mut file, &mut head).ok()?;
        frame_size_of(&head)
    }

    /// The camera motions the library knows, most common first, for the
    /// page's filter list. Read once from the index and cheap to ask
    /// again: the column holds a handful of distinct values, and the
    /// answer is a few dozen rows however many clips there are.
    pub fn cameras(&self) -> Vec<String> {
        let Some(index) = self.db.get() else {
            return Vec::new();
        };
        let Ok(index) = index.lock() else {
            return Vec::new();
        };
        let Ok(mut statement) = index
            .db
            .prepare("SELECT camera, count(*) AS n FROM clips GROUP BY camera ORDER BY n DESC")
        else {
            return Vec::new();
        };
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    /// The frame size as the index already knows it, network aside.
    fn dims_known(&self, id: i64) -> Result<Option<(u32, u32)>, String> {
        let index = self
            .db
            .get()
            .and_then(|db| db.lock().ok())
            .ok_or_else(|| "the OpenVid index is not ready".to_owned())?;
        let dims = index
            .db
            .query_row("SELECT w, h FROM dims WHERE id = ?", [id], |row| {
                Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32))
            })
            .ok();
        Ok(dims)
    }

    /// Writes a learned frame size down. INSERT OR REPLACE, because two
    /// cards can ask at once and both answers are the same answer.
    fn remember_dims(&self, id: i64, (w, h): (u32, u32)) -> Result<(), String> {
        let index = self
            .db
            .get()
            .and_then(|db| db.lock().ok())
            .ok_or_else(|| "the OpenVid index is not ready".to_owned())?;
        index
            .db
            .execute(
                "INSERT OR REPLACE INTO dims (id, w, h) VALUES (?, ?, ?)",
                (id, w as i64, h as i64),
            )
            .map_err(|error| format!("the frame size could not be kept: {error}"))?;
        Ok(())
    }

    /// The shelves - as the numbers the index stores - whose census sample
    /// was unanimous for this shape. A shelf that was never censused, or
    /// whose sample disagreed with itself, or that was sampled too thinly,
    /// is not here: only clips their own headers have measured match the
    /// shape, which is the answer a shelf must earn the right to widen.
    fn censused(&self, shape: Orientation) -> Vec<i64> {
        let Some(index) = self.db.get() else {
            return Vec::new();
        };
        let Ok(index) = index.lock() else {
            return Vec::new();
        };
        index
            .db
            .prepare(
                "SELECT source FROM census WHERE shape = ? AND agreed = sample AND sample >= 8",
            )
            .and_then(|mut statement| {
                let shelves = statement
                    .query_map([shape.word()], |row| row.get::<_, i64>(0))?
                    .filter_map(Result::ok)
                    .collect::<Vec<i64>>();
                Ok(shelves)
            })
            .unwrap_or_default()
    }

    /// Takes the census: for every shelf without a verdict, samples
    /// [`CENSUS`] clips, measures their frame sizes - [`CENSUS_WIDTH`]
    /// reads at a time over one shared agent, each reader keeping its
    /// connection warm - and writes the verdict the sample earned, held
    /// to no more than what it agreed on. The measurements themselves
    /// land in `dims` like any other, so the census leaves real rows
    /// behind rather than guesses. A first read that fails says the
    /// network is not there and the whole census is abandoned: an
    /// unanswered shelf is retried by the next shape search, never
    /// silently guessed for. A shelf already censused is not asked
    /// again - a mixed verdict is as final as a unanimous one, and
    /// speaks for nothing.
    fn take_census(&self) {
        let lacking: Vec<i64> = match self.db.get().and_then(|db| db.lock().ok()) {
            Some(index) => index
                .db
                .prepare(
                    "SELECT source FROM clips \
                     WHERE source NOT IN (SELECT source FROM census) GROUP BY source",
                )
                .and_then(|mut statement| {
                    let shelves = statement
                        .query_map([], |row| row.get::<_, i64>(0))?
                        .filter_map(Result::ok)
                        .collect::<Vec<i64>>();
                    Ok(shelves)
                })
                .unwrap_or_default(),
            None => return,
        };
        if lacking.is_empty() {
            return;
        }
        // The samples: CENSUS clips of each shelf lacking a verdict.
        let mut sample: Vec<(i64, String, u64, u64, Source)> = Vec::new();
        {
            let Some(index) = self.db.get().and_then(|db| db.lock().ok()) else {
                return;
            };
            for shelf in &lacking {
                let rows = index
                    .db
                    .prepare(
                        "SELECT clips.id, files.name, clips.pos, clips.size, clips.source \
                         FROM clips JOIN files ON files.id = clips.file \
                         WHERE clips.source = ? ORDER BY random() LIMIT ?",
                    )
                    .and_then(|mut statement| {
                        let rows = statement
                            .query_map([*shelf, CENSUS], |row| {
                                Ok((
                                    row.get::<_, i64>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, i64>(2)?.max(0) as u64,
                                    row.get::<_, i64>(3)?.max(0) as u64,
                                    Source::of_number(row.get::<_, i64>(4)?),
                                ))
                            })?
                            .filter_map(Result::ok)
                            .collect::<Vec<(i64, String, u64, u64, Source)>>();
                        Ok(rows)
                    })
                    .unwrap_or_default();
                sample.extend(rows);
            }
        }
        let Some(first) = sample.first() else {
            return;
        };
        // One agent, shared by the readers: the census is one burst of
        // small reads, and a connection each is the difference between
        // seconds and tens of them.
        let agent = crate::net::agent(
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(60),
        );
        // The probe: a first read that fails says the network is not
        // there, and nothing else is worth trying. An error here is not
        // a verdict - no row is written, and the next search tries again.
        if self
            .read_dims(&first.1, first.2, first.3, first.4, &agent)
            .is_err()
        {
            return;
        }
        // The measurements, CENSUS_WIDTH at a time. Each reader pulls
        // the next row itself, so the reads spread themselves and no
        // row is read twice. The counter is declared outside the scope
        // because a spawned reader may live until the scope ends, and
        // must be able to borrow it that long.
        let next = std::sync::atomic::AtomicUsize::new(0);
        let measured: Vec<(i64, Source, (u32, u32))> = std::thread::scope(|scope| {
            let readers: Vec<_> = (0..CENSUS_WIDTH)
                .map(|_| {
                    scope.spawn(|| {
                        let mut found: Vec<(i64, Source, (u32, u32))> = Vec::new();
                        loop {
                            let at = next.fetch_add(1, Ordering::Relaxed);
                            if at >= sample.len() {
                                break;
                            }
                            let (id, file, pos, size, source) = &sample[at];
                            if let Ok(Some(dims)) =
                                self.read_dims(file, *pos, *size, *source, &agent)
                            {
                                found.push((*id, *source, dims));
                            }
                        }
                        found
                    })
                })
                .collect();
            readers
                .into_iter()
                .filter_map(|reader| reader.join().ok())
                .flatten()
                .collect()
        });
        for (id, _, dims) in &measured {
            let _ = self.remember_dims(*id, *dims);
        }
        // The verdicts: each lacking shelf's plurality shape, with how
        // many of its measured sample agreed. Unanimous and thick
        // enough, `censused` lets it widen the filter; mixed, it is
        // final and speaks for nothing.
        let count = |source: i64, shape: Orientation| {
            measured
                .iter()
                .filter(|(_, shelf, dims)| {
                    shelf.number() == source && Orientation::of(*dims) == shape
                })
                .count()
        };
        if let Some(index) = self.db.get().and_then(|db| db.lock().ok()) {
            for shelf in lacking {
                let sample_of = measured
                    .iter()
                    .filter(|(_, source, _)| source.number() == shelf)
                    .count();
                if sample_of == 0 {
                    continue; // Nothing measured: no verdict, retried later.
                }
                let verdict = [
                    Orientation::Vertical,
                    Orientation::Horizontal,
                    Orientation::Square,
                ]
                .into_iter()
                .max_by_key(|shape| count(shelf, *shape))
                .unwrap_or(Orientation::Square);
                let agreed = count(shelf, verdict);
                let _ = index.db.execute(
                    "INSERT OR REPLACE INTO census (source, shape, agreed, sample) VALUES (?, ?, ?, ?)",
                    (shelf, verdict.word(), agreed as i64, sample_of as i64),
                );
            }
        }
    }

    /// Where a clip's MP4 lives: the data file it sits in, the byte range
    /// inside that file, and the shelf it came from - the shelf because it
    /// is what says which end of the file the `moov` box is on, for
    /// [`Library::read_dims`]. This is the whole trick of fetching one clip
    /// out of a 7.5 TB dataset - the index knows the range, the Hub honours
    /// a Range header, and nothing else moves.
    fn clip_range(&self, id: i64) -> Result<Option<(String, u64, u64, Source)>, String> {
        let index = self
            .db
            .get()
            .and_then(|db| db.lock().ok())
            .ok_or_else(|| "the OpenVid index is not ready".to_owned())?;
        index
            .db
            .query_row(
                "SELECT files.name, clips.pos, clips.size, clips.source \
                 FROM clips JOIN files ON files.id = clips.file WHERE clips.id = ?",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?.max(0) as u64,
                        row.get::<_, i64>(2)?.max(0) as u64,
                        Source::of_number(row.get::<_, i64>(3)?),
                    ))
                },
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(format!("the OpenVid index would not open: {other}")),
            })
    }

    /// Reads a clip's frame size from the Hub: one end of the file first -
    /// which end is the shelf's to say, the WebVid shelf's files being
    /// faststart and everyone else's carrying their `moov` at the back -
    /// and the other end when the first says nothing, so a clip the shelf
    /// misjudged costs one more small read, once, and is measured either
    /// way. Each read is a ranged GET of at most [`TRAILER`] bytes over
    /// the agent given - one agent for a single ask, the census's shared
    /// one for its many - and the parse is [`frame_size_of`], tested
    /// against a real clip of the dataset's own making.
    fn read_dims(
        &self,
        file: &str,
        pos: u64,
        size: u64,
        source: Source,
        agent: &ureq::Agent,
    ) -> Result<Option<(u32, u32)>, String> {
        let read = |start: u64| -> Result<Option<(u32, u32)>, String> {
            let length = size.min(TRAILER);
            let bytes: Vec<u8> = ranged_with(agent, file, start, length)?;
            Ok(frame_size_of(&bytes))
        };
        // A clip smaller than the trailer window is one read from either
        // end: the head and the tail are the same bytes, so the second
        // read is skipped by the starts being equal.
        let head = pos;
        let tail = pos + size - size.min(TRAILER);
        let (first, second) = if source == Source::Other {
            (head, tail)
        } else {
            (tail, head)
        };
        if let Some(dims) = read(first)? {
            return Ok(Some(dims));
        }
        if second != first {
            return read(second);
        }
        Ok(None)
    }

    /// Fetches a clip whole, into the page's own cache, and says where it
    /// landed. A clip already on disk is returned as it stands - clips are
    /// immutable, so there is nothing to refresh. Blocks; run it on a
    /// worker, with a cancel flag the window owns.
    pub fn fetch(&self, id: i64, cancel: &AtomicBool) -> Result<PathBuf, String> {
        if let Some(path) = self.fetched(id) {
            return Ok(path);
        }
        let Some((file, pos, size, _)) = self.clip_range(id)? else {
            return Err(format!("there is no clip {id} in the OpenVid library"));
        };
        let target = clip_path(&self.dirs, id);
        let folder = target.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
        let url = format!("{DATASET_URL}/{file}");
        // The clips are small - the largest is tens of megabytes - so one
        // ranged GET is the whole fetch; the models downloader's resume
        // machinery is for the index, not for these.
        let agent = crate::net::agent(
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(60),
        );
        let response = agent
            .get(&url)
            .set("Range", &format!("bytes={pos}-{}", pos + size - 1))
            .call()
            .map_err(|error| format!("the Hub did not answer: {error}"))?;
        if response.status() != 206 {
            return Err(format!(
                "the Hub would not serve a byte range (status {})",
                response.status()
            ));
        }
        let partial = target.with_extension("partial");
        {
            use std::io::{Read, Write};
            let mut body = Vec::with_capacity(size as usize);
            response
                .into_reader()
                .take(size)
                .read_to_end(&mut body)
                .map_err(|error| format!("the clip did not arrive: {error}"))?;
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_owned());
            }
            std::fs::File::create(&partial)
                .and_then(|mut file| file.write_all(&body))
                .map_err(|error| format!("could not write {}: {error}", partial.display()))?;
        }
        if std::fs::metadata(&partial).map_or(true, |meta| meta.len() != size) {
            let _ = std::fs::remove_file(&partial);
            return Err("the clip arrived short".to_owned());
        }
        std::fs::rename(&partial, &target)
            .map_err(|error| format!("could not keep {}: {error}", target.display()))?;
        Ok(target)
    }

    /// Where a fetched clip is, if it has been fetched.
    pub fn fetched(&self, id: i64) -> Option<PathBuf> {
        let path = clip_path(&self.dirs, id);
        path.is_file().then_some(path)
    }

    /// Every fetched clip, oldest first by name, for the day the page wants
    /// to say what the cache is holding.
    pub fn fetched_clips(&self) -> Vec<PathBuf> {
        let Ok(listing) = std::fs::read_dir(dir(&self.dirs).join("clips")) else {
            return Vec::new();
        };
        let mut clips: Vec<PathBuf> = listing
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "mp4"))
            .collect();
        clips.sort();
        clips
    }
}

/// A row of a clip query, as the Clip every asker carries. The columns
/// are the search's and the one-clip lookup's both, in the same order; the
/// caption column is the row's compressed bytes, decompressed here with
/// the dictionary the index was built with.
fn clip_of_row(row: &rusqlite::Row<'_>, dict: &[u8]) -> rusqlite::Result<Clip> {
    let path: String = row.get(1)?;
    let compressed: Vec<u8> = row.get(2)?;
    let w = row.get::<_, Option<i64>>(10)?;
    let h = row.get::<_, Option<i64>>(11)?;
    Ok(Clip {
        id: row.get(0)?,
        name: Path::new(&path)
            .file_stem()
            .map_or(path.clone(), |stem| stem.to_string_lossy().into_owned()),
        caption: caption_of(&compressed, dict),
        aesthetic: row.get(3)?,
        motion: row.get(4)?,
        temporal: row.get(5)?,
        camera: row.get(6)?,
        fps: row.get(7)?,
        seconds: row.get(8)?,
        bytes: row.get::<_, i64>(9)?.max(0) as u64,
        source: Source::of(&path),
        dims: match (w, h) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as u32, h as u32)),
            _ => None,
        },
    })
}

/// One compressed caption, as words again. The index is digest-checked on
/// the way in, so a caption that will not decompress is a should-never:
/// said to the log and shown as an empty caption rather than taking the
/// whole page of results down with it.
fn caption_of(compressed: &[u8], dict: &[u8]) -> String {
    let mut text = String::new();
    let read = zstd::stream::read::Decoder::with_dictionary(compressed, dict)
        .and_then(|mut decoder| decoder.read_to_string(&mut text));
    if read.is_ok() {
        text
    } else {
        let error = read
            .err()
            .map_or("no bytes".to_owned(), |error| error.to_string());
        log::warn!("openvid: a caption did not decompress: {error}");
        String::new()
    }
}

/// The library as the page and the API tell it where it stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// No index on disk. The page offers the download.
    Missing,
    /// The index is coming down, this many per-cent along.
    Downloading(u32),
    /// Searched locally from here on.
    Ready,
    /// The index is on disk but will not open, or the download failed; the
    /// words are the page's to show as they are.
    Failed(String),
}

/// A value bound into a search's SQL, so the WHERE builder can hold mixed
/// numbers and words without the query itself ever seeing them. Everything
/// a person types is bound, never spliced: their words are data, not SQL.
#[derive(Clone, Debug)]
enum Bind {
    Real(f64),
    Integer(i64),
    Text(String),
}

impl Bind {
    fn value(&self) -> rusqlite::types::Value {
        match self {
            Bind::Real(value) => rusqlite::types::Value::Real(*value),
            Bind::Integer(value) => rusqlite::types::Value::Integer(*value),
            Bind::Text(value) => rusqlite::types::Value::Text(value.clone()),
        }
    }
}

/// The one library per process. Built the first time anything asks, from
/// the app's own directories, because every asker - the pane, the API
/// server in the window, an MCP server in this same process - means the
/// same folder and the same index.
pub fn shared(dirs: &AppDirs) -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| Library {
        dirs: dirs.clone(),
        db: OnceLock::new(),
        progress: AtomicU32::new(0),
        running: AtomicBool::new(false),
    })
}

/// Reads `length` bytes at `start` out of one of the dataset's data files
/// on the Hub, over the agent given: one of its own for a single ask, the
/// census's shared one for its burst of them. The status must be 206 - a
/// 200 would mean the range was ignored and the whole 16 GB file was
/// about to arrive, which is stopped here rather than read.
fn ranged_with(
    agent: &ureq::Agent,
    file: &str,
    start: u64,
    length: u64,
) -> Result<Vec<u8>, String> {
    let url = format!("{DATASET_URL}/{file}");
    let response = agent
        .get(&url)
        .set("Range", &format!("bytes={start}-{}", start + length - 1))
        .call()
        .map_err(|error| format!("the Hub did not answer: {error}"))?;
    if response.status() != 206 {
        return Err(format!(
            "the Hub would not serve a byte range (status {})",
            response.status()
        ));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    response
        .into_reader()
        .take(length)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the bytes did not arrive: {error}"))?;
    Ok(bytes)
}

/// Turns a person's words into the query FTS5 is asked to match: each word
/// quoted and starred - so it matches itself and anything it begins, and
/// no punctuation of the person's is ever read as FTS5's own syntax - and
/// the words ANDed, because "ocean sunset" means both, ranked by how well
/// the caption carries them. Punctuation is a separator, as the tokenizer
/// itself reads it, so `City(night)!` is two words and never one strange
/// one. Empty in, empty out.
fn fts_words(raw: &str) -> String {
    let mut words = String::new();
    for token in raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .take(12)
    {
        let clean = token.to_lowercase();
        if !words.is_empty() {
            words.push(' ');
        }
        words.push('"');
        words.push_str(&clean);
        words.push_str("\"*");
    }
    words
}

/// The SQL a shape is asked of a search, and the shelves - as the numbers
/// the index stores - whose census said the same shape. Measured sizes
/// answer first, and a censused shelf's *unmeasured* clips stand beside
/// them: `dims.id IS NULL` is what keeps the sample in its place, because
/// a clip whose own header has spoken - either way - is never overruled
/// by its shelf's. A shape no shelf speaks for is asked of the measured
/// sizes alone, which is every clip there is an answer for.
fn orientation_predicate(shape: Orientation, shelves: &[i64]) -> (String, Vec<Bind>) {
    let dims = shape.sql();
    if shelves.is_empty() {
        return (dims.to_owned(), Vec::new());
    }
    let seats = vec!["?"; shelves.len()].join(", ");
    (
        format!("({dims} OR (clips.source IN ({seats}) AND dims.id IS NULL))"),
        shelves.iter().map(|n| Bind::Integer(*n)).collect(),
    )
}

/// The width and height an MP4 end names, from its first video track's
/// `tkhd` box. The dataset's MP4s put `moov` - the box holding the
/// tracks - at one end of the file or the other; whichever end's bytes
/// are in hand, the search for the box starts from the buffer's end and
/// works backwards, because a box's own bytes can contain the word
/// "moov" too, and the size field before a real one always fits the
/// bytes in hand.
fn frame_size_of(trailer: &[u8]) -> Option<(u32, u32)> {
    let mut at = trailer.len();
    while let Some(found) = trailer[..at].windows(4).rposition(|w| w == b"moov") {
        if found < 4 {
            return None;
        }
        let start = found - 4;
        let size = u32::from_be_bytes([
            trailer[start],
            trailer[start + 1],
            trailer[start + 2],
            trailer[start + 3],
        ]) as usize;
        if size >= 8
            && start + size <= trailer.len()
            && let Some(dims) = dims_of_moov(&trailer[start..start + size])
        {
            return Some(dims);
        }
        at = start;
    }
    None
}

/// The first video track's dimensions in a `moov` box's bytes. A track
/// counts as the video one when its `mdia`'s `hdlr` box names `vide`; a
/// sound track's `tkhd` is skipped, because its width and height are zero
/// and would answer the question with a lie.
fn dims_of_moov(moov: &[u8]) -> Option<(u32, u32)> {
    let mut at = 8;
    while at + 8 <= moov.len() {
        let size = box_size(moov, at)?;
        if size < 8 || at + size > moov.len() {
            return None;
        }
        let kind = &moov[at + 4..at + 8];
        if kind == b"trak"
            && let Some(dims) = dims_of_trak(&moov[at + 8..at + size])
        {
            return Some(dims);
        }
        at += size;
    }
    None
}

/// One `trak` box's dimensions, when it is the video track.
fn dims_of_trak(trak: &[u8]) -> Option<(u32, u32)> {
    let mut tkhd = None;
    let mut video = false;
    let mut at = 0;
    while at + 8 <= trak.len() {
        let size = box_size(trak, at)?;
        if size < 8 || at + size > trak.len() {
            return None;
        }
        let kind = &trak[at + 4..at + 8];
        if kind == b"tkhd" {
            tkhd = Some(&trak[at + 8..at + size]);
        } else if kind == b"mdia" {
            let mdia = &trak[at + 8..at + size];
            let mut inner = 0;
            while inner + 8 <= mdia.len() {
                let inner_size = box_size(mdia, inner)?;
                if inner_size < 8 || inner + inner_size > mdia.len() {
                    break;
                }
                if &mdia[inner + 4..inner + 8] == b"hdlr"
                    && inner + 8 + 16 <= inner + inner_size
                    && &mdia[inner + 16..inner + 20] == b"vide"
                {
                    video = true;
                }
                inner += inner_size;
            }
        }
        at += size;
    }
    let tkhd = tkhd?;
    if !video || tkhd.len() < 84 {
        return None;
    }
    // `tkhd`'s version byte says whether its times are 32- or 64-bit; the
    // width and height are the last eight bytes either way, as 16.16
    // fixed-point.
    let (w, h) = if tkhd[0] == 1 {
        if tkhd.len() < 96 {
            return None;
        }
        (fixed(&tkhd[88..92])?, fixed(&tkhd[92..96])?)
    } else {
        (fixed(&tkhd[76..80])?, fixed(&tkhd[80..84])?)
    };
    (w > 0 && h > 0).then_some((w, h))
}

/// An MP4 box's size at `at`, four big-endian bytes. A size of zero means
/// "to the end of the file", which cannot be honoured inside a trailer,
/// and a size of one means a 64-bit size follows - one this dataset's
/// files do not use, and not one to guess at.
fn box_size(bytes: &[u8], at: usize) -> Option<usize> {
    let end = at + 4;
    if end > bytes.len() {
        return None;
    }
    let size = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    (size >= 8).then_some(size as usize)
}

/// A 16.16 fixed-point number, as the whole number it is here.
fn fixed(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `tkhd` box, version 0, with the width and height given. The
    /// layout is the one the MP4 specification writes: creation and
    /// modification times, track id and reserved, duration, reserved,
    /// layer, alternate group, volume and reserved, the matrix, then the
    /// width and height as 16.16.
    fn tkhd(w: u32, h: u32) -> Vec<u8> {
        let mut body = vec![0u8; 84];
        body[0] = 0;
        let fixed_w = (w << 16).to_be_bytes().to_vec();
        let fixed_h = (h << 16).to_be_bytes().to_vec();
        body[76..80].copy_from_slice(&fixed_w);
        body[80..84].copy_from_slice(&fixed_h);
        let mut box_bytes = b"tkhd".to_vec();
        let mut size = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        let mut out = Vec::new();
        out.append(&mut size);
        out.append(&mut box_bytes);
        out.append(&mut body);
        out
    }

    /// Wraps `inner` in a box of `kind`.
    fn box_of(kind: &[u8; 4], inner: &[u8]) -> Vec<u8> {
        let mut out = ((inner.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(inner);
        out
    }

    /// A `moov` holding one video track and one sound track, as the
    /// dataset's MP4s do - video first, but the sound track is there to be
    /// skipped, and its zero-sized `tkhd` is the trap.
    fn moov(w: u32, h: u32) -> Vec<u8> {
        // hdlr with handler type "vide".
        let mut hdlr_body = vec![0u8; 24];
        hdlr_body[8..12].copy_from_slice(b"vide");
        let hdlr = box_of(b"hdlr", &hdlr_body);
        let mdia = box_of(b"mdia", &hdlr);
        let video_trak = box_of(b"trak", &[tkhd(w, h), mdia].concat());
        // The sound track: an hdlr naming "soun" and a zero-sized tkhd.
        let mut soun = vec![0u8; 24];
        soun[8..12].copy_from_slice(b"soun");
        let sound_mdia = box_of(b"mdia", &box_of(b"hdlr", &soun));
        let sound_trak = box_of(b"trak", &[tkhd(0, 0), sound_mdia].concat());
        box_of(b"moov", &[video_trak, sound_trak].concat())
    }

    #[test]
    fn a_trailer_names_the_video_track_size() {
        let moov = moov(1920, 1080);
        // The trailer holds junk that happens to contain the word "moov"
        // before the real one, with a nonsense size.
        let mut trailer = b"junk moov \0\0\0\0 more junk".to_vec();
        trailer.extend_from_slice(&moov);
        assert_eq!(frame_size_of(&trailer), Some((1920, 1080)));
    }

    #[test]
    fn a_square_clip_is_square_and_a_tall_one_is_vertical() {
        assert_eq!(Orientation::of((512, 512)), Orientation::Square);
        assert_eq!(Orientation::of((1080, 1920)), Orientation::Vertical);
        assert_eq!(Orientation::of((1920, 1080)), Orientation::Horizontal);
        // Five per cent of slack either way.
        assert_eq!(Orientation::of((520, 512)), Orientation::Square);
        assert_eq!(Orientation::of((600, 512)), Orientation::Horizontal);
    }

    #[test]
    fn the_search_words_are_quoted_and_starred() {
        assert_eq!(fts_words("ocean sunset"), "\"ocean\"* \"sunset\"*");
        assert_eq!(fts_words("  City(night)!  "), "\"city\"* \"night\"*");
        assert_eq!(fts_words("---"), "");
        assert_eq!(fts_words(""), "");
    }

    /// The whole library against the real index and the real Hub: the
    /// search's words through FTS5 and the dictionary-compressed captions
    /// back out, a clip's trailer for its frame size, and the clip itself
    /// down as one ranged read. Ignored by default - it wants the network
    /// and a 435 MB index - and run by hand when either changes:
    /// `cargo test -p concat-host --lib openvid -- --ignored` with
    /// OPENVID_INDEX pointing at an index file.
    #[test]
    #[ignore]
    fn the_library_end_to_end() {
        let index = std::path::PathBuf::from(
            std::env::var("OPENVID_INDEX").expect("OPENVID_INDEX names the index file"),
        );
        let data = std::path::PathBuf::from("/tmp/openvid-live");
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(data.join("openvid")).expect("the scratch folder");
        std::fs::copy(&index, data.join("openvid/openvid-index.sqlite")).expect("the index copied");
        let dirs = AppDirs {
            config: data.clone(),
            data,
        };
        let library = shared(&dirs);
        assert_eq!(library.status(), Status::Ready, "the index opens");
        assert_eq!(library.rows(), Some(937_957), "every clip is in it");

        let found = library
            .search(&Query {
                text: Some("sunset ocean".to_owned()),
                min_aesthetic: Some(5.0),
                sort: Sort::Relevance,
                limit: 5,
                ..Query::default()
            })
            .expect("the search runs");
        assert!(!found.clips.is_empty(), "the words find clips");
        assert!(found.clips[0].aesthetic >= 5.0);
        assert!(
            found
                .clips
                .iter()
                .any(|clip| clip.caption.contains("ocean") || clip.caption.contains("sunset")),
            "the captions came back as words, not compressed bytes: {:?}",
            found.clips[0].caption
        );

        let id = found.clips[0].id;
        let dims = library.dims(id).expect("the trailer is read");
        assert_eq!(dims, Some((512, 512)), "the dataset's own shape");

        // The census, as the first shape search takes it: every shelf
        // sampled and measured, once, and the unanimous verdicts widening
        // the shape filters from then on. The WebVid shelf is 720p and
        // 1080p landscape, so Horizontal finds it whole; Vertical, with no
        // shelf speaking for it and nothing measured tall, finds nothing -
        // an honest nothing, which is the point.
        let horizontal = library
            .search(&Query {
                orientation: Some(Orientation::Horizontal),
                sort: Sort::Aesthetic,
                limit: 5,
                ..Query::default()
            })
            .expect("the horizontal search runs");
        assert_eq!(horizontal.total, 553_789, "the WebVid shelf, whole");
        assert!(
            horizontal
                .clips
                .iter()
                .all(|clip| clip.source == Source::Other),
            "the censused shelf's clips"
        );
        let square = library
            .search(&Query {
                orientation: Some(Orientation::Square),
                limit: 5,
                ..Query::default()
            })
            .expect("the square search runs");
        assert_eq!(square.total, 384_168, "the three square shelves, whole");
        let vertical = library
            .search(&Query {
                orientation: Some(Orientation::Vertical),
                limit: 5,
                ..Query::default()
            })
            .expect("the vertical search runs");
        assert_eq!(vertical.total, 0, "nothing in this library is tall");

        let cancel = AtomicBool::new(false);
        let path = library.fetch(id, &cancel).expect("the clip is fetched");
        let size = std::fs::metadata(&path).expect("the clip is on disk").len();
        assert_eq!(size, found.clips[0].bytes, "exactly the clip's own bytes");
        assert!(library.fetched(id).is_some(), "and it is remembered");
    }

    #[test]
    fn a_shelf_is_read_out_of_the_name() {
        assert_eq!(
            Source::of("pixabay_auroras_videos_90877_000.mp4"),
            Source::Pixabay
        );
        assert_eq!(
            Source::of("pexels_landscape_5669996_005.mp4"),
            Source::Pexels
        );
        assert_eq!(Source::of("celebv_IMU00UwepuQ_5.mp4"), Source::CelebV);
        assert_eq!(Source::of("o2LQGDTAF0A_34_0to118.mp4"), Source::Other);
    }

    /// The shape predicate's two shapes: asked alone when no shelf speaks
    /// for the shape, and asked beside the censused shelves' unmeasured
    /// clips when one does. A shelf's number is a bound parameter, never
    /// spliced into the SQL, exactly as every other filter is.
    #[test]
    fn the_shape_predicate_speaks_for_the_censused_shelves() {
        let (alone, binds) = orientation_predicate(Orientation::Vertical, &[]);
        assert_eq!(alone, "dims.h > dims.w * 1.05");
        assert!(binds.is_empty());

        let (widened, binds) = orientation_predicate(Orientation::Horizontal, &[3]);
        assert_eq!(
            widened,
            "(dims.w > dims.h * 1.05 OR (clips.source IN (?) AND dims.id IS NULL))"
        );
        assert_eq!(binds.len(), 1);

        let (square, binds) = orientation_predicate(Orientation::Square, &[0, 1, 2]);
        assert_eq!(
            square,
            "(dims.h <= dims.w * 1.05 AND dims.w <= dims.h * 1.05 OR \
             (clips.source IN (?, ?, ?) AND dims.id IS NULL))"
        );
        assert_eq!(binds.len(), 3);
    }
}
