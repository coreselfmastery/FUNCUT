// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The sticker library: two shelves of transparent PNGs, none of them in
//! the repository or the app, fetched one picture at a time.
//!
//! *MAGICK* is OneOverZero's MAGICK (CVPR 2024): 148,193 captioned
//! single-subject 1024x1024 RGBA images, chroma-keyed out of their
//! backgrounds - people, animals, objects, gestures - each one a caption
//! a search reads and a URL the Hugging Face Hub serves as one plain
//! HTTPS GET. What the page and the agent need once is the dataset's own
//! TSV index, ~16 MB of `page_id` and caption: it is published as a
//! release asset of this repository, checked against a digest baked into
//! the build, and parsed into memory once. The 3,000 rows the index
//! itself marks not-safe-for-work are left out at parse; the shelf that
//! remains is 145,193 pictures.
//!
//! *OpenMoji* is the OpenMoji project's colour set (hfg-gmuend, CC-BY
//! 4.0): 4,565 emoji at 618x618, each a palette PNG whose `tRNS` carries
//! the transparency. Its index is the project's own `openmoji.json`
//! (~2 MB, annotation and tags per entry), read once from the project's
//! repository and cached; a picture is one plain GET of its own PNG. A
//! living upstream rather than a frozen dataset, so the index is cached
//! rather than digest-pinned - the way the sound library's manifest is.
//!
//! The search is one engine over both shelves: the query's words are
//! lowercased and cut at anything that is not a letter or a digit, the
//! glue that only holds a sentence together ("a", "the", "for", ...) is
//! dropped, and the words are ANDed - every word must match somewhere in
//! a caption, as a whole word or the front of one. A caption scores by
//! where its matches landed and how rare the matched words are: the
//! query's whole phrase said as one run of words counts most (+100),
//! then each word (+20 whole, +12 as a prefix) scaled by how few
//! captions share it, less a small penalty for rambling captions, so a
//! terse caption that says the thing outranks a sprawling one that says
//! it among other things. The page's field and the API's `magick.search`
//! ask the same engine, so a person and an agent are answered by the
//! same list.
//!
//! Nothing here touches a project. A picture put on the timeline is
//! copied into that project's own folder by the window, as a sound or a
//! stock clip is, and fetched into `<data>/magick/images/` first - once;
//! a picture fetched twice is fetched once. Beside the pictures,
//! `favourites.json` keeps the ones the heart was pressed on, ids of
//! either shelf, so a favourite survives the cache being cleared.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

use crate::AppDirs;

/// Where the MAGICK index is published: a release asset on this
/// repository, so the download needs no account and no token. The digest
/// beside it is baked into this build; a new index is a new release tag
/// and a new constant.
pub const INDEX_URL: &str =
    "https://github.com/coreselfmastery/FUNCUT/releases/download/magick-index-v1/magick_index.tsv";

/// The MAGICK index file's SHA-256, lowercase hex. A download that does
/// not match is deleted, not installed.
pub const INDEX_SHA256: &str = "b9b3be5c72bb14faf85d6ef1d0f05db2fc4476c9cd1eab88846ce36bf34c4417";

/// The same index, straight from the Hub, for when GitHub's downloads do
/// not reach this machine; and hf-mirror.com carries what the Hub does,
/// which is how the shelf reaches mainland China.
pub const INDEX_URL_HF: &str =
    "https://huggingface.co/datasets/OneOverZero/MAGICK/resolve/main/magick_index.tsv";

/// The dataset the MAGICK shelf's pictures live in. A picture is one
/// plain GET of `…/images/{first two of the id}/{id}.png`.
pub const DATASET_URL: &str =
    "https://huggingface.co/datasets/OneOverZero/MAGICK/resolve/main/images";

/// The OpenMoji index, as the project's repository serves it: every
/// entry's `hexcode`, `annotation` and `tags`. Read once, cached beside
/// the pictures; a living upstream, so no digest is pinned.
pub const OPENMOJI_INDEX: &str =
    "https://raw.githubusercontent.com/hfg-gmuend/openmoji/master/data/openmoji.json";

/// The OpenMoji colour PNGs, at the size meant for use: one plain GET of
/// `…/color/618x618/{hexcode}.png`.
pub const OPENMOJI_PNG: &str =
    "https://raw.githubusercontent.com/hfg-gmuend/openmoji/master/color/618x618";

/// How big the MAGICK index is, for the download's progress bar: ~16 MB.
const INDEX_BYTES: u64 = 17_000_000;

/// How big the OpenMoji index is, for the same reason: ~2 MB.
const OPENMOJI_BYTES: u64 = 3_000_000;

/// Where the page's own files live: `<data>/magick/`, the indexes beside
/// the pictures and the favourites. `data` is large state by the app's
/// convention, which is right for both.
pub fn dir(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("magick")
}

/// How many words a search will read, at most. A sentence of glue and
/// twelve words is a sentence the search can answer; the thirteenth
/// word has stopped being the ask.
const MOST_WORDS: usize = 12;

/// The two shelves, as a filter names them and a result carries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// OneOverZero's MAGICK: 145,193 captioned AI-generated pictures,
    /// chroma-keyed to transparency.
    Magick,
    /// The OpenMoji colour set: 4,565 emoji, CC-BY 4.0.
    Openmoji,
}

impl Source {
    /// The shelf's own name, as the API's words and the pane's rows say
    /// it.
    pub fn name(self) -> &'static str {
        match self {
            Source::Magick => "magick",
            Source::Openmoji => "openmoji",
        }
    }

    /// The shelf a name means; `None` for one that is no shelf's.
    pub fn parse(name: &str) -> Option<Source> {
        match name.to_ascii_lowercase().as_str() {
            "magick" => Some(Source::Magick),
            "openmoji" | "emoji" => Some(Source::Openmoji),
            _ => None,
        }
    }

    /// The shelf's seat among the library's per-shelf arrays: a
    /// download, and its progress, are a shelf's own, not the whole
    /// library's - the pane loads both shelves at once, and one flag
    /// shared between them would have the loser's `prepare` say
    /// "nothing to do" and return a success it never earned.
    fn seat(self) -> usize {
        match self {
            Source::Magick => 0,
            Source::Openmoji => 1,
        }
    }

    /// Both shelves, in the order the page lists them.
    pub const ALL: [Source; 2] = [Source::Magick, Source::Openmoji];
}

/// One picture of the library, as a search result carries it and as the
/// API reports it. The caption is what the search read; the bytes
/// themselves arrive only when the picture is picked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sticker {
    /// The picture's id in its dataset - MAGICK's `page_id`, OpenMoji's
    /// `hexcode` - and the id every verb takes back.
    pub id: String,
    /// Which shelf it is on.
    pub source: Source,
    /// What the picture shows, in the dataset's own words: the caption
    /// the search read. Short for an emoji ("grinning face"), a sentence
    /// for a MAGICK picture.
    pub subject: String,
}

/// What a search asks for. Every field is optional; an all-`None` search
/// is "show me stickers", the shelves as they come. The same type serves
/// the window's page and the API's `magick.search`, so the two cannot
/// disagree about what a filter means.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Words to find in captions. Terms are ANDed and match the front of
    /// a word: `red balloon` finds captions holding both a word starting
    /// "red" and one starting "balloon", ranked by relevance.
    pub text: Option<String>,
    /// Only this shelf.
    pub source: Option<Source>,
    /// Only the pictures the heart was pressed on.
    pub favourites: bool,
    /// How many, at most (1-100), from the front of the order.
    pub limit: u32,
    /// Skip this many from the front, for the next page.
    pub offset: u32,
}

/// What a search found: the stickers, best first, and how many match in
/// all - the count so the page can say "1,234 stickers" without fetching
/// a thousand cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The stickers, `limit` of them from `offset`.
    pub stickers: Vec<Sticker>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// Where a shelf stands, as the page and the API report it. Cheap to
/// ask: a file's presence and a lock, no I/O to speak of, so the page
/// can poll it while a download runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// The shelf's index is not on this machine yet; `prepare` brings
    /// it.
    Missing,
    /// The shelf's one-time download is running, this far along: 0-100.
    Downloading(u32),
    /// The index is parsed and the shelf can be searched.
    Ready,
    /// The last download or parse failed, with the words that say why.
    Failed(String),
}

impl Status {
    /// The per-cent a load has reached, when one is running; for the
    /// page's publish to read without matching on every shelf's word.
    pub fn percent_of_load(&self) -> Option<u32> {
        match self {
            Status::Downloading(percent) => Some(*percent),
            _ => None,
        }
    }
}

/// One row of a parsed index: the picture's id, its caption as a card
/// shows it, and the same caption lowercased - with the OpenMoji tags
/// folded in - which is the one string the search reads, precomputed
/// once so a search never lowercases per row.
#[derive(Clone, Debug)]
struct Row {
    id: String,
    subject: String,
    lowered: String,
}

/// Both shelves' parsed indexes, held by the one library per process.
#[derive(Default)]
struct Shelves {
    magick: Vec<Row>,
    openmoji: Vec<Row>,
}

impl Shelves {
    /// The rows of one shelf.
    fn of(&self, source: Source) -> &Vec<Row> {
        match source {
            Source::Magick => &self.magick,
            Source::Openmoji => &self.openmoji,
        }
    }
}

/// The whole library in one place: its indexes' state, their parsed rows
/// and the pictures on disk. One per process, reached through
/// [`shared`], because the window and its embedded API server are one
/// process and a download started twice would race itself; a *separate*
/// process - the headless MCP server - gets its own instance, and the
/// two are safe together because an index is only ever replaced by an
/// atomic rename.
pub struct Library {
    dirs: AppDirs,
    /// The parsed indexes, once opened. Opened once and kept: parsing
    /// 16 MB of TSV per keystroke would be absurd, and per search
    /// merely wasteful.
    shelves: OnceLock<Mutex<Shelves>>,
    /// Where each shelf's download stands, as a per-cent 0-100. An
    /// atomic rather than the shelves' mutex, so a download's worker
    /// can tick it without taking a lock a search is reading.
    progress: [AtomicU32; 2],
    /// Set while a download is running in *this* process, per shelf,
    /// so a second `prepare` of the same shelf - from the pane and the
    /// window's API server at once, say - cannot start a second one,
    /// while the other shelf's prepare runs alongside it. Across
    /// processes, the partial file's name carries the pid, so two
    /// downloaders never write the same file either.
    running: [AtomicBool; 2],
    /// The heart's list, loaded once and written through on every
    /// change.
    favourites: Mutex<Option<Vec<String>>>,
}

/// The one library per process; see [`Library`].
pub fn shared(dirs: &AppDirs) -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| Library {
        dirs: dirs.clone(),
        shelves: OnceLock::new(),
        progress: [AtomicU32::new(0), AtomicU32::new(0)],
        running: [AtomicBool::new(false), AtomicBool::new(false)],
        favourites: Mutex::new(None),
    })
}

impl Library {
    /// The shelf's state: a download running, the index present but
    /// never opened (opened now, so the answer is honest), missing, or
    /// the last failure's words.
    pub fn status(&self, source: Source) -> Status {
        let seat = source.seat();
        if self.running[seat].load(Ordering::Relaxed) {
            return Status::Downloading(self.progress[seat].load(Ordering::Relaxed));
        }
        let opened = self.shelves.get().is_some_and(|shelves| {
            shelves
                .lock()
                .map(|held| !held.of(source).is_empty())
                .unwrap_or(false)
        });
        if opened {
            return Status::Ready;
        }
        if !index_path(&self.dirs, source).is_file() {
            return Status::Missing;
        }
        // On disk but not parsed yet: parse now, and report what that
        // said. A file that will not parse is a real condition - a
        // truncated download wearing the finished file's name - not one
        // to quietly call "missing".
        match self.open_index(source) {
            Ok(()) => Status::Ready,
            Err(error) => Status::Failed(error),
        }
    }

    /// How many pictures a shelf holds, once ready.
    pub fn rows(&self, source: Source) -> Option<u64> {
        self.shelves
            .get()
            .and_then(|shelves| shelves.lock().ok())
            .map(|held| held.of(source).len() as u64)
    }

    /// Brings a shelf's index in: downloaded once - MAGICK's from the
    /// release, then the Hub, then hf-mirror.com, checked against
    /// [`INDEX_SHA256`] whichever answered; OpenMoji's from its own
    /// repository - and parsed. Blocks - run it on a worker - and
    /// reports its per-cent through the status the page is already
    /// polling. A download already running in this process is not an
    /// error, just nothing to do.
    pub fn prepare(&self, source: Source, cancel: &AtomicBool) -> Result<(), String> {
        if index_path(&self.dirs, source).is_file() {
            // Present, but perhaps not parseable; the caller's status
            // will say which, and there is nothing to download either
            // way.
            return self.open_index(source);
        }
        let seat = source.seat();
        if self.running[seat].swap(true, Ordering::SeqCst) {
            return Ok(()); // One is already running; its progress is
            // the one to watch.
        }
        let outcome = self.download_index(source, cancel);
        self.running[seat].store(false, Ordering::SeqCst);
        self.progress[seat].store(0, Ordering::Relaxed);
        outcome
    }

    /// The download itself, in the shape [`crate::models::download`]
    /// gives: a partial file resumed by range, a read timeout, and a
    /// cancel flag a closed window can set.
    fn download_index(&self, source: Source, cancel: &AtomicBool) -> Result<(), String> {
        let target = index_path(&self.dirs, source);
        let folder = target.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
        // The partial file carries this process's pid, so the window and
        // a headless agent fetching at the same moment write two files
        // and the first to finish renames over the second, rather than
        // both appending to one corrupt thing.
        let name = target.file_name().map_or_else(
            || "index".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let partial = folder.join(format!("{name}.partial-{}", std::process::id()));
        let progress = &self.progress[source.seat()];
        let mut report = move |done: u64, total: u64| {
            let percent = if total > 0 {
                ((done as f64 / total as f64) * 100.0).round() as u32
            } else {
                0
            };
            progress.store(percent.min(100), Ordering::Relaxed);
        };
        let mut last = String::new();
        for url in index_sources(source) {
            let estimate = match source {
                Source::Magick => INDEX_BYTES,
                Source::Openmoji => OPENMOJI_BYTES,
            };
            match crate::models::download(
                &url,
                &partial,
                estimate,
                cancel,
                "cancelled",
                &mut report,
            ) {
                Ok(_) => {
                    let checked = match source {
                        // The MAGICK index is a frozen dataset: whatever
                        // source answered, the bytes must be the ones
                        // this build was built against.
                        Source::Magick => crate::models::verify(&partial, INDEX_SHA256)
                            .map_err(|error| format!("the MAGICK index did not verify: {error}")),
                        // OpenMoji is a living upstream: the JSON is
                        // parsed below, and a parse that fails is
                        // refused there.
                        Source::Openmoji => Ok(()),
                    };
                    match checked {
                        Ok(()) => {
                            std::fs::rename(&partial, &target).map_err(|error| {
                                format!("could not keep {}: {error}", target.display())
                            })?;
                            match self.open_index(source) {
                                Ok(()) => return Ok(()),
                                Err(error) => {
                                    // An OpenMoji index that will not
                                    // parse - the living upstream's JSON,
                                    // with no digest to have checked - is
                                    // taken back off the shelf rather
                                    // than left as a file every later
                                    // `prepare` refuses on sight: the
                                    // next attempt downloads it again.
                                    // MAGICK's digest has already said
                                    // these bytes are the build's own,
                                    // so its file stands to be argued
                                    // with in words instead.
                                    if let Source::Openmoji = source {
                                        let _ = std::fs::remove_file(&target);
                                    }
                                    return Err(error);
                                }
                            }
                        }
                        Err(error) => {
                            let _ = std::fs::remove_file(&partial);
                            last = error;
                        }
                    }
                }
                Err(error) if error == "cancelled" => {
                    let _ = std::fs::remove_file(&partial);
                    return Err(error);
                }
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    /// Parses a shelf's index into the shelves, if they are not open
    /// already. The MAGICK TSV is the dataset's own - `page_id` to
    /// `picked`, a header row, the not-safe-for-work rows left out - and
    /// the OpenMoji JSON is the project's own list of annotations and
    /// tags. A second opener - the pane and the API server at once -
    /// keeps the first one's rows: both parsed the same file, and the
    /// one that arrives second is simply dropped.
    fn open_index(&self, source: Source) -> Result<(), String> {
        let shelves = self.shelves.get_or_init(|| Mutex::new(Shelves::default()));
        {
            let held = shelves
                .lock()
                .map_err(|_| "the sticker library's index is busy".to_owned())?;
            if !held.of(source).is_empty() {
                return Ok(());
            }
        }
        let bytes = std::fs::read(index_path(&self.dirs, source))
            .map_err(|error| format!("could not read the index: {error}"))?;
        let parsed = match source {
            Source::Magick => parse_magick(&bytes)?,
            Source::Openmoji => parse_openmoji(&bytes)?,
        };
        let mut held = shelves
            .lock()
            .map_err(|_| "the sticker library's index is busy".to_owned())?;
        if held.of(source).is_empty() {
            match source {
                Source::Magick => held.magick = parsed,
                Source::Openmoji => held.openmoji = parsed,
            }
        }
        Ok(())
    }

    /// Runs a query against the shelves. Every filter is optional; a
    /// search against a shelf that is not ready is refused with the
    /// words that say what to do about it, and a search with no shelf
    /// named is refused only when neither is ready.
    pub fn search(&self, query: &Query) -> Result<Found, String> {
        let shelves = self.shelves.get_or_init(|| Mutex::new(Shelves::default()));
        // Open what a cached index left unopened; a shelf with no index
        // on this machine is a refusal below, not a quiet empty page.
        {
            let held = shelves
                .lock()
                .map_err(|_| "the sticker library's index is busy".to_owned())?;
            let open: Vec<Source> = Source::ALL
                .into_iter()
                .filter(|source| {
                    held.of(*source).is_empty() && index_path(&self.dirs, *source).is_file()
                })
                .collect();
            drop(held);
            for source in open {
                self.open_index(source)?;
            }
        }
        let held = shelves
            .lock()
            .map_err(|_| "the sticker library's index is busy".to_owned())?;
        let wanted = query.source.unwrap_or(Source::Magick);
        let fallback = query.source.is_none();
        let ready = |source: Source| !held.of(source).is_empty();
        if !ready(wanted) && !(fallback && (ready(Source::Magick) || ready(Source::Openmoji))) {
            // The shelf's own status, asked for its words - after the
            // lock above is let go, because opening an index takes the
            // same lock a second time.
            drop(held);
            return Err(match self.status(wanted) {
                Status::Downloading(percent) => format!(
                    "the {} shelf is still downloading ({percent}%); ask magick.status until \
                     it is ready",
                    wanted.name()
                ),
                Status::Failed(error) => {
                    format!("the {} shelf is unusable: {error}", wanted.name())
                }
                _ => format!(
                    "the {} shelf is not on this machine yet; call magick.prepare once, then \
                     ask magick.status until it is ready",
                    wanted.name()
                ),
            });
        }
        let sources: Vec<Source> = if fallback {
            Source::ALL
                .into_iter()
                .filter(|source| ready(*source))
                .collect()
        } else {
            vec![wanted]
        };
        // The heart's list, read once when it narrows the search; and
        // the ask's own words, cut and de-glued.
        let hearts = query.favourites.then(|| self.favourite_ids());
        let words = query_words(query.text.as_deref().unwrap_or_default());
        let words = &words[..words.len().min(MOST_WORDS)];
        let phrase = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| text.to_lowercase())
            .unwrap_or_default();
        // How many captions each word lands in - counted before the
        // scoring pass, because a word's rarity is what its match is
        // worth, and rarity is a fact about the shelf rather than about
        // the one caption in hand.
        let mut seen: Vec<u32> = vec![0; words.len()];
        let mut rows_searched = 0u32;
        for source in &sources {
            for row in held.of(*source) {
                if hearts
                    .as_ref()
                    .is_some_and(|hearts| !hearts.contains(&row.id))
                {
                    continue;
                }
                rows_searched += 1;
                for (at, word) in words.iter().enumerate() {
                    if lands(word, &row.lowered) {
                        seen[at] += 1;
                    }
                }
            }
        }
        // The scoring pass: a word scores where it landed - whole (+20)
        // or as a prefix (+12) - scaled by its rarity, `ln((N+1)/(df+1))`
        // turned into half again to a half more, so a word every caption
        // holds is still worth half its points and a word two captions
        // share is worth about half again. The whole phrase said as one
        // run of words is worth +100, and a caption pays a fifth of a
        // point a character for its sprawl.
        let mut scored: Vec<(i64, &Row, Source)> = Vec::new();
        for source in &sources {
            for row in held.of(*source) {
                if hearts
                    .as_ref()
                    .is_some_and(|hearts| !hearts.contains(&row.id))
                {
                    continue;
                }
                if words.is_empty() {
                    scored.push((0, row, *source));
                    continue;
                }
                let mut score: i64 = 0;
                let mut matched_all = true;
                for (at, word) in words.iter().enumerate() {
                    let Some(points) = lands_how(word, &row.lowered) else {
                        matched_all = false;
                        break;
                    };
                    let rare = ((rows_searched as f64 + 1.0) / (seen[at] as f64 + 1.0)).ln() / 10.0;
                    score += (points as f64 * (0.5 + rare)) as i64;
                }
                if !matched_all {
                    continue;
                }
                if !phrase.is_empty() && row.lowered.contains(&phrase) {
                    score += 100;
                }
                score -= (row.lowered.chars().count() as i64) / 5;
                scored.push((score, row, *source));
            }
        }
        if !words.is_empty() {
            // Best first; the sort is stable, so captions that score the
            // same stay in the index's own order.
            scored.sort_by(|one, other| other.0.cmp(&one.0));
        }
        let total = scored.len() as u64;
        let stickers = scored
            .into_iter()
            .skip(query.offset as usize)
            .take(query.limit.clamp(1, 100) as usize)
            .map(|(_, row, source)| Sticker {
                id: row.id.clone(),
                source,
                subject: row.subject.clone(),
            })
            .collect();
        Ok(Found { stickers, total })
    }

    /// One picture by id, from whichever shelf was named.
    pub fn sticker(&self, source: Source, id: &str) -> Option<Sticker> {
        if self.shelves.get().is_none() && index_path(&self.dirs, source).is_file() {
            // An index on disk that no search opened yet: opened here,
            // so a get by id works before any search has run.
            self.open_index(source).ok()?;
        }
        let held = self.shelves.get()?.lock().ok()?;
        held.of(source)
            .iter()
            .find(|row| row.id == id)
            .map(|row| Sticker {
                id: row.id.clone(),
                source,
                subject: row.subject.clone(),
            })
    }

    /// One picture by id, from whichever shelf holds it. The ids never
    /// overlap - MAGICK's are ten mixed-case alphanumerics, OpenMoji's
    /// are upper-case hex - so one id names one picture without saying
    /// its shelf.
    pub fn find(&self, id: &str) -> Option<Sticker> {
        self.sticker(Source::Magick, id)
            .or_else(|| self.sticker(Source::Openmoji, id))
    }

    /// Where a picture is kept once fetched: `images/{id}.png`, or
    /// `None` when it is not on this machine. The id is the dataset's
    /// own - MAGICK's `page_id`, OpenMoji's `hexcode` - stable for as
    /// long as the dataset is, so a picture fetched twice is fetched
    /// once; both shelves share the one folder, since the ids never
    /// overlap.
    pub fn fetched(&self, id: &str) -> Option<PathBuf> {
        let path = image_path(&self.dirs, id);
        path.is_file().then_some(path)
    }

    /// Fetches a picture into the cache - one plain GET of its own PNG,
    /// ~1 MB for a MAGICK picture, a few KB for an emoji - and says
    /// where it landed. The bytes are checked to be a PNG that can hold
    /// transparency before they are kept: colour type 6 (RGBA), which is
    /// what MAGICK serves, or a palette PNG that carries a `tRNS`, which
    /// is what OpenMoji serves. Blocks: a worker's.
    pub fn fetch(
        &self,
        source: Source,
        id: &str,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<PathBuf, String> {
        let target = image_path(&self.dirs, id);
        if target.is_file() {
            return Ok(target);
        }
        let Some(url) = image_url(source, id) else {
            return Err(format!("there is no picture {id}"));
        };
        let folder = images_dir(&self.dirs);
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
        let name = target.file_name().map_or_else(
            || "sticker.png".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let partial = folder.join(format!("{name}.partial-{}", std::process::id()));
        let mut last = String::new();
        // The Hub first, hf-mirror.com behind it: the same bytes, and
        // the check below is what either is believed by.
        for url in [url.clone()]
            .into_iter()
            .chain(crate::models::hf_mirror(&url))
        {
            match crate::models::download(&url, &partial, 0, cancel, "cancelled", progress) {
                Ok(_) => {
                    let bytes = std::fs::read(&partial)
                        .map_err(|error| format!("could not read what arrived: {error}"))?;
                    if !is_png_with_alpha(&bytes) {
                        let _ = std::fs::remove_file(&partial);
                        last = format!("{id} did not arrive as a transparent PNG");
                        continue;
                    }
                    std::fs::rename(&partial, &target)
                        .map_err(|error| format!("could not keep {}: {error}", target.display()))?;
                    return Ok(target);
                }
                Err(error) if error == "cancelled" => {
                    let _ = std::fs::remove_file(&partial);
                    return Err(error);
                }
                Err(error) => last = error,
            }
        }
        let _ = std::fs::remove_file(&partial);
        Err(last)
    }

    /// The heart's list, as ids: every picture the heart was pressed on,
    /// whichever shelf it is on, newest first.
    pub fn favourite_ids(&self) -> Vec<String> {
        let mut held = self
            .favourites
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if held.is_none() {
            *held = std::fs::read(dir(&self.dirs).join("favourites.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
        }
        held.clone().unwrap_or_default()
    }

    /// Whether a picture is on the heart's list.
    pub fn is_favourite(&self, id: &str) -> bool {
        self.favourite_ids().iter().any(|held| held == id)
    }

    /// Presses or lifts the heart on a picture, and says which it is
    /// now: newest first in the file, so the page can show the list in
    /// the order it grew.
    pub fn toggle_favourite(&self, id: &str) -> Result<bool, String> {
        let mut held = self
            .favourites
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut list = held.clone().unwrap_or_default();
        if let Some(at) = list.iter().position(|held| held == id) {
            list.remove(at);
            *held = Some(list);
            self.write_favourites(held.as_deref().unwrap_or(&[]))?;
            return Ok(false);
        }
        list.insert(0, id.to_owned());
        *held = Some(list);
        self.write_favourites(held.as_deref().unwrap_or(&[]))?;
        Ok(true)
    }

    /// Writes the heart's list, through a temporary file and a rename,
    /// so a crash mid-write leaves the last one whole.
    fn write_favourites(&self, list: &[String]) -> Result<(), String> {
        let folder = dir(&self.dirs);
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
        let path = folder.join("favourites.json");
        let partial = folder.join("favourites.json.partial");
        let bytes = serde_json::to_vec_pretty(list).map_err(|error| error.to_string())?;
        std::fs::write(&partial, bytes)
            .and_then(|()| std::fs::rename(&partial, &path))
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    }
}

/// Where a shelf's index is kept.
fn index_path(dirs: &AppDirs, source: Source) -> PathBuf {
    match source {
        Source::Magick => dir(dirs).join("magick_index.tsv"),
        Source::Openmoji => dir(dirs).join("openmoji.json"),
    }
}

/// The pictures' folder, both shelves' together: the id is the dataset's
/// own and never overlaps - MAGICK's are ten mixed-case alphanumerics,
/// OpenMoji's are upper-case hex.
fn images_dir(dirs: &AppDirs) -> PathBuf {
    dir(dirs).join("images")
}

/// Where one picture is kept once fetched. Both shelves share the one
/// folder: the id is the dataset's own and never overlaps.
fn image_path(dirs: &AppDirs, id: &str) -> PathBuf {
    images_dir(dirs).join(format!("{id}.png"))
}

/// Where to fetch a picture from: MAGICK's PNG sits two folders deep -
/// the id's first two characters, then the id - and OpenMoji's sits by
/// its hexcode. `None` for an id that cannot be one, so a fetch refuses
/// rather than asking the dataset for a file that cannot exist.
fn image_url(source: Source, id: &str) -> Option<String> {
    let plain = |id: &str| {
        !id.is_empty()
            && id.len() < 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if !plain(id) {
        return None;
    }
    match source {
        Source::Magick if id.len() >= 2 => Some(format!("{DATASET_URL}/{}/{id}.png", &id[..2])),
        Source::Magick => None,
        Source::Openmoji => Some(format!("{OPENMOJI_PNG}/{id}.png")),
    }
}

/// Where a shelf's index download tries, in order. Every source that
/// could hold the index is in the list once; a mirror that cannot be
/// reached - a proxy that blocks a host, a release still being
/// published, a region that cannot see it - costs a retry rather than a
/// shelf.
fn index_sources(source: Source) -> Vec<String> {
    match source {
        Source::Magick => {
            // The Hub's own copy behind the release, with hf-mirror.com
            // behind that: the same bytes, the digest the judge of
            // whichever answers.
            let mut urls = vec![INDEX_URL.to_owned()];
            urls.extend(crate::models::hf_mirror(INDEX_URL_HF));
            urls.push(INDEX_URL_HF.to_owned());
            urls
        }
        Source::Openmoji => vec![OPENMOJI_INDEX.to_owned()],
    }
}

/// The MAGICK index, as the dataset ships it: a header row, then a row a
/// picture - `page_id`, `sim_score`, `color`, `nsfw`, `subject`,
/// `tag_idk`, `picked`. The rows the index itself marks
/// not-safe-for-work are left out here, once, at parse: a sticker shelf
/// is not where they belong, and 145,193 of the 148,193 remain.
fn parse_magick(bytes: &[u8]) -> Result<Vec<Row>, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| "the MAGICK index is not UTF-8".to_owned())?;
    let mut rows = Vec::new();
    for line in text.split('\n').skip(1) {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(id), Some(_), Some(_), Some(nsfw), Some(subject)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        if id.is_empty() || nsfw.trim() != "0" {
            continue;
        }
        let subject = subject.trim();
        if subject.is_empty() {
            continue;
        }
        rows.push(Row {
            id: id.to_owned(),
            subject: subject.to_owned(),
            lowered: subject.to_lowercase(),
        });
    }
    if rows.is_empty() {
        return Err("the MAGICK index holds no pictures".to_owned());
    }
    Ok(rows)
}

/// The OpenMoji index, as the project's own `openmoji.json` has it. The
/// caption a card shows is the annotation - "grinning face" - and the
/// words a search reads are the annotation and the tags together, so
/// "smile" finds what the tags say even though the annotation does not.
fn parse_openmoji(bytes: &[u8]) -> Result<Vec<Row>, String> {
    #[derive(Deserialize)]
    struct Entry {
        #[serde(default)]
        hexcode: String,
        #[serde(default)]
        annotation: String,
        #[serde(default)]
        tags: String,
    }
    let entries: Vec<Entry> = serde_json::from_slice(bytes)
        .map_err(|error| format!("the OpenMoji index is not readable: {error}"))?;
    let rows: Vec<Row> = entries
        .into_iter()
        .filter(|entry| !entry.hexcode.is_empty())
        .map(|entry| {
            let subject = entry.annotation.trim().to_owned();
            let lowered = format!("{} {}", subject.to_lowercase(), entry.tags.to_lowercase());
            Row {
                id: entry.hexcode,
                subject,
                lowered,
            }
        })
        .collect();
    if rows.is_empty() {
        return Err("the OpenMoji index holds no pictures".to_owned());
    }
    Ok(rows)
}

/// The words of a query: lowercased, cut at anything that is not a
/// letter or a digit, empties dropped - "Red  BALLOON!" is red and
/// balloon - and the little words that only glue a sentence together
/// dropped rather than matched, so "a red balloon for the party" asks
/// what it means and not one word more. The same cuts a caption's words
/// are read by.
fn query_words(query: &str) -> Vec<String> {
    const GLUE: [&str; 12] = [
        "a", "an", "the", "for", "with", "and", "or", "of", "to", "that", "on", "in",
    ];
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty() && !GLUE.contains(word))
        .map(str::to_owned)
        .collect()
}

/// Whether `word` lands anywhere in `caption` - as one of its own words
/// or as the front of one - by the same cuts [`query_words`] makes.
fn lands(word: &str, caption: &str) -> bool {
    caption
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token.starts_with(word))
}

/// How `word` lands in `caption`, as points: 20 for one of the
/// caption's own words, 12 for the front of one ("waving" finds
/// "waves"), `None` when it matches nothing there. The AND of a search
/// is a `None` here: a word that lands nowhere leaves the caption out.
fn lands_how(word: &str, caption: &str) -> Option<i64> {
    let mut best = None;
    for token in caption.split(|c: char| !c.is_alphanumeric()) {
        if token == word {
            return Some(20);
        }
        if token.starts_with(word) {
            best = Some(12);
        }
    }
    best
}

/// Whether `bytes` is a PNG that can hold transparency: the signature,
/// a header whose width and height are real, and either colour type 6
/// (truecolour with an alpha channel, what MAGICK serves) or colour
/// type 3 with a `tRNS` chunk (a palette PNG that says which of its
/// colours are see-through, what OpenMoji serves).
fn is_png_with_alpha(bytes: &[u8]) -> bool {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 33 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return false;
    }
    let (width, height) = (
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
    );
    if width == 0 || height == 0 {
        return false;
    }
    let colour = bytes[25];
    if colour == 6 {
        return true;
    }
    if colour != 3 {
        return false;
    }
    // A palette PNG is transparent only when it says which colours are:
    // a `tRNS` chunk anywhere before the pixels.
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let length =
            u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        let kind = &bytes[at + 4..at + 8];
        if kind == b"tRNS" {
            return true;
        }
        if kind == b"IDAT" {
            return false; // The pixels came before any transparency.
        }
        at += 12 + length;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("concat-magick-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A library over a scratch folder, its shelves parsed in - the
    /// shape an open index leaves behind - ready to search.
    fn made(name: &str, rows: Shelves) -> (Library, PathBuf) {
        let dir = scratch(name);
        (
            Library {
                dirs: AppDirs::under(&dir),
                shelves: OnceLock::from(Mutex::new(rows)),
                progress: [AtomicU32::new(0), AtomicU32::new(0)],
                running: [AtomicBool::new(false), AtomicBool::new(false)],
                favourites: Mutex::new(None),
            },
            dir,
        )
    }

    /// A little of both shelves to search, in the shape each index has.
    fn shelves() -> Shelves {
        Shelves {
            magick: vec![
                Row {
                    id: "QlhA9gJbNR".into(),
                    subject:
                        "close up of a middle age girl with ginger short hair tattoos waving"
                            .into(),
                    lowered:
                        "close up of a middle age girl with ginger short hair tattoos waving"
                            .into(),
                },
                Row {
                    id: "3z3KUvfhPP".into(),
                    subject: "middle age soccer player girl with strawberry blonde afro hair blue eyes sunglasses wearing dress waving".into(),
                    lowered: "middle age soccer player girl with strawberry blonde afro hair blue eyes sunglasses wearing dress waving".into(),
                },
                Row {
                    id: "abcdRedBall".into(),
                    subject: "a red balloon floating over a party".into(),
                    lowered: "a red balloon floating over a party".into(),
                },
            ],
            openmoji: vec![
                Row {
                    id: "1F600".into(),
                    subject: "grinning face".into(),
                    lowered: "grinning face cheerful, cheery, face, grin, grinning, happy, laugh, nice, smile, smiling, teeth".into(),
                },
                Row {
                    id: "1F389".into(),
                    subject: "party popper".into(),
                    lowered: "party popper celebration, congrats, party, tada".into(),
                },
            ],
        }
    }

    /// The TSV as the dataset ships it - header, columns, a
    /// not-safe-for-work row among the rest - parses into rows without
    /// the row the index itself refuses, its captions lowercased for the
    /// search.
    #[test]
    fn the_magick_index_parses_and_drops_the_refused() {
        let tsv = "page_id\tsim_score\tcolor\tnsfw\tsubject\ttag_idk\tpicked\n\
                   wvuRmfeuim\t0.987\tgreen\t0\tstressed out asian woman, plugs ears\t0\t\n\
                   7KWBoDICUI\t0.983\tgreen\t0\tBlack sorceress woman WAVING\t0\t\n\
                   5bH7lMfQgi\t0.99\tgreen\t1\tscared businesswoman, waving no\t0\t\n\
                   \t0.1\tgreen\t0\tan id that is not one\t0\t\n";
        let rows = parse_magick(tsv.as_bytes()).expect("parses");
        assert_eq!(rows.len(), 2, "the nsfw row is left out");
        assert_eq!(rows[0].id, "wvuRmfeuim");
        assert_eq!(rows[1].lowered, "black sorceress woman waving");
        assert!(
            parse_magick(b"page_id\tbroken\n").is_err(),
            "an index with no rows refuses"
        );
    }

    /// The OpenMoji index parses into rows whose caption is the
    /// annotation and whose search words are the annotation and the tags
    /// together.
    #[test]
    fn the_openmoji_index_reads_the_annotation_and_the_tags() {
        let json = r#"[{"hexcode":"1F600","annotation":"grinning face","tags":"cheerful, smile"},
                       {"hexcode":"","annotation":"no id","tags":""},{"annotation":"no id either","tags":""}]"#;
        let rows = parse_openmoji(json.as_bytes()).expect("parses");
        assert_eq!(rows.len(), 1, "the id-less entries are left out");
        assert_eq!(rows[0].id, "1F600");
        assert_eq!(rows[0].subject, "grinning face");
        assert!(rows[0].lowered.starts_with("grinning face"));
        assert!(rows[0].lowered.contains("smile"));
        assert!(
            parse_openmoji(b"[]").is_err(),
            "an index with no rows refuses"
        );
    }

    /// The words are ANDed and prefix-matched, the glue is dropped, and
    /// the plain shelf order stands when there is nothing to match.
    #[test]
    fn words_are_anded_and_glue_is_dropped() {
        let (library, dir) = made("anded", shelves());
        let ids = |query: &str| -> Vec<String> {
            library
                .search(&Query {
                    text: Some(query.into()),
                    limit: 100,
                    ..Query::default()
                })
                .unwrap()
                .stickers
                .into_iter()
                .map(|sticker| sticker.id)
                .collect()
        };
        assert_eq!(ids("waving").len(), 2);
        assert_eq!(
            ids("a middle age girl waving").len(),
            2,
            "glue dropped, prefix matched"
        );
        assert_eq!(ids("waving afro"), vec!["3z3KUvfhPP".to_owned()]);
        assert!(ids("spaceship").is_empty());
        // The emoji shelf reads its tags: "smile" finds the grinning
        // face by a word the annotation alone does not say.
        assert_eq!(ids("smile"), vec!["1F600".to_owned()]);
        assert_eq!(ids("party").len(), 2, "the balloon and the popper");
        // Nothing asked is the shelf as it comes.
        assert_eq!(
            library
                .search(&Query {
                    limit: 100,
                    ..Query::default()
                })
                .unwrap()
                .stickers
                .len(),
            5
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole phrase said as one run of words outranks its words
    /// apart, and a caption that says the thing in fewer words outranks
    /// a sprawling one that says it among other things.
    #[test]
    fn the_phrase_and_the_terse_caption_outrank() {
        let (library, dir) = made("ranked", shelves());
        let found = library
            .search(&Query {
                text: Some("waving girl".into()),
                limit: 100,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(found.stickers[0].id, "QlhA9gJbNR", "the shorter caption");
        let found = library
            .search(&Query {
                text: Some("middle age girl with ginger short hair tattoos waving".into()),
                limit: 100,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(found.stickers[0].id, "QlhA9gJbNR", "the phrase, said whole");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The shelf filter narrows before the words, and a search against a
    /// shelf with no index is a refusal with the words that say what to
    /// do about it, not a quiet empty page.
    #[test]
    fn a_shelf_is_a_filter_and_an_empty_one_a_refusal() {
        let (library, dir) = made("shelves", shelves());
        let emoji = library
            .search(&Query {
                text: Some("party".into()),
                source: Some(Source::Openmoji),
                limit: 100,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(emoji.stickers.len(), 1);
        assert_eq!(emoji.stickers[0].id, "1F389");
        // A library whose shelves were never opened refuses, with the
        // words that say what to do about it.
        let (fresh, fresh_dir) = made("refused", Shelves::default());
        let error = fresh
            .search(&Query {
                source: Some(Source::Openmoji),
                ..Query::default()
            })
            .unwrap_err();
        assert!(error.contains("magick.prepare"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&fresh_dir);
    }

    /// The limit and offset page through what a search found, and the
    /// total says how many match in all.
    #[test]
    fn the_limit_and_offset_page() {
        let (library, dir) = made("paged", shelves());
        let found = library
            .search(&Query {
                limit: 2,
                offset: 1,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(found.stickers.len(), 2);
        assert_eq!(found.total, 5);
        let found = library
            .search(&Query {
                limit: 0,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(
            found.stickers.len(),
            1,
            "a limit of nothing is one, as the music page's is"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The picture URLs are the datasets' own shapes, and an id that
    /// cannot be one is refused rather than asked for.
    #[test]
    fn the_picture_urls_are_the_datasets_own() {
        assert_eq!(
            image_url(Source::Magick, "7KWBoDICUI").as_deref(),
            Some(
                "https://huggingface.co/datasets/OneOverZero/MAGICK/resolve/main/images/7K/7KWBoDICUI.png"
            )
        );
        assert_eq!(
            image_url(Source::Openmoji, "1F389").as_deref(),
            Some(
                "https://raw.githubusercontent.com/hfg-gmuend/openmoji/master/color/618x618/1F389.png"
            )
        );
        assert!(image_url(Source::Magick, "7").is_none());
        assert!(image_url(Source::Magick, "../escape").is_none());
        assert!(image_url(Source::Openmoji, "../escape").is_none());
    }

    /// Only PNGs that can hold transparency are believed: truecolour
    /// with an alpha channel, and a palette PNG that says which of its
    /// colours are see-through. The rest - a JPEG wearing the name, an
    /// opaque palette, a truncated file - are not.
    #[test]
    fn only_pngs_that_can_hold_transparency_are_believed() {
        let mut rgba = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        rgba.extend_from_slice(b"IHDR");
        rgba.extend_from_slice(&1024u32.to_be_bytes());
        rgba.extend_from_slice(&1024u32.to_be_bytes());
        rgba.extend_from_slice(&[8, 6, 0, 0, 0]);
        rgba.extend_from_slice(&[0, 0, 0, 0, b'I', b'D', b'A', b'T']);
        assert!(is_png_with_alpha(&rgba));
        // Colour type 2 (truecolour, no alpha) is not.
        let mut opaque = rgba.clone();
        opaque[25] = 2;
        assert!(!is_png_with_alpha(&opaque));
        // A palette PNG is transparent only with a tRNS.
        let mut palette = rgba.clone();
        palette[25] = 3;
        assert!(!is_png_with_alpha(&palette), "no tRNS yet");
        let mut with_trns = Vec::new();
        with_trns.extend_from_slice(&palette[..33]);
        with_trns.extend_from_slice(&[0, 0, 0, 1, b't', b'R', b'N', b'S', 0x80, 0, 0, 0, 0]);
        with_trns.extend_from_slice(&[0, 0, 0, 0, b'I', b'D', b'A', b'T']);
        assert!(is_png_with_alpha(&with_trns));
        // Not a PNG at all.
        assert!(!is_png_with_alpha(b"GIF89a not a png"));
        assert!(!is_png_with_alpha(&rgba[..20]), "truncated");
    }

    /// The heart's list round-trips through the file, newest first, a
    /// toggle says which way it went, and a fresh process reads what the
    /// last one wrote.
    #[test]
    fn the_hearts_list_round_trips() {
        let dir = scratch("favourites");
        let dirs = AppDirs::under(&dir);
        let library = Library {
            dirs: dirs.clone(),
            shelves: OnceLock::new(),
            progress: [AtomicU32::new(0), AtomicU32::new(0)],
            running: [AtomicBool::new(false), AtomicBool::new(false)],
            favourites: Mutex::new(None),
        };
        assert!(!library.is_favourite("QlhA9gJbNR"));
        assert!(library.toggle_favourite("QlhA9gJbNR").unwrap());
        assert!(library.toggle_favourite("1F600").unwrap());
        assert!(library.is_favourite("1F600"));
        assert_eq!(
            library.favourite_ids(),
            vec!["1F600".to_owned(), "QlhA9gJbNR".to_owned()],
            "newest first"
        );
        assert!(!library.toggle_favourite("1F600").unwrap());
        assert!(!library.is_favourite("1F600"));
        let reread = Library {
            dirs,
            shelves: OnceLock::new(),
            progress: [AtomicU32::new(0), AtomicU32::new(0)],
            running: [AtomicBool::new(false), AtomicBool::new(false)],
            favourites: Mutex::new(None),
        };
        assert_eq!(reread.favourite_ids(), vec!["QlhA9gJbNR".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real shelves answer: the MAGICK index down from the release
    /// (then the Hub, then hf-mirror.com), the OpenMoji index from its
    /// own repository, a search over each, and one picture of each
    /// fetched through its own plain GET and checked to be a PNG that
    /// can hold transparency before it is kept - the second fetch of the
    /// same picture the same file, fetched once. Marked `ignore` because
    /// it moves real bytes and needs the network; run it with
    /// `cargo test -p concat-host magick -- --ignored` when a source or
    /// this module changes.
    #[test]
    #[ignore]
    fn the_real_sources_answer() {
        let dir = scratch("live");
        let dirs = AppDirs::under(&dir);
        let library = shared(&dirs);
        let cancel = AtomicBool::new(false);

        // MAGICK: the index arrives, verifies against the digest baked
        // into this build, and parses into 145,193 rows.
        library
            .prepare(Source::Magick, &cancel)
            .expect("the MAGICK index arrives");
        assert_eq!(
            library.rows(Source::Magick),
            Some(145_193),
            "the shelf this build was built against"
        );

        // OpenMoji: the project's own list arrives and parses.
        library
            .prepare(Source::Openmoji, &cancel)
            .expect("the OpenMoji index arrives");
        let emoji_rows = library.rows(Source::Openmoji).unwrap_or_default();
        assert!(emoji_rows > 4_500, "the colour set holds {emoji_rows}");

        // A search over both shelves at once finds what either says.
        let found = library
            .search(&Query {
                text: Some("waving".to_owned()),
                limit: 5,
                ..Query::default()
            })
            .expect("searches");
        assert!(!found.stickers.is_empty(), "somebody is waving");
        assert!(
            found
                .stickers
                .iter()
                .all(|sticker| sticker.subject.to_lowercase().contains("waving"))
        );

        // A MAGICK picture: fetched, checked, cached; fetched again from
        // the cache, the same file.
        let magick = found.stickers[0].clone();
        let path = library
            .fetch(magick.source, &magick.id, &cancel, &mut |_, _| {})
            .expect("the picture arrives");
        assert!(path.is_file());
        let again = library
            .fetch(magick.source, &magick.id, &cancel, &mut |_, _| {})
            .expect("the picture is kept");
        assert_eq!(again, path, "fetched once");
        let _ = std::fs::remove_file(&path);

        // An emoji of the other shelf, by the words its tags say.
        let party = library
            .search(&Query {
                text: Some("party".to_owned()),
                source: Some(Source::Openmoji),
                limit: 5,
                ..Query::default()
            })
            .expect("searches the emoji shelf");
        assert!(!party.stickers.is_empty());
        let emoji = party.stickers[0].clone();
        let emoji_path = library
            .fetch(emoji.source, &emoji.id, &cancel, &mut |_, _| {})
            .expect("the emoji arrives");
        assert!(emoji_path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The favourites filter narrows a search to the heart's list.
    #[test]
    fn the_hearts_filter_narrows_a_search() {
        let (library, dir) = made("hearts", shelves());
        library.toggle_favourite("1F389").unwrap();
        let found = library
            .search(&Query {
                favourites: true,
                limit: 100,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(found.stickers.len(), 1);
        assert_eq!(found.stickers[0].id, "1F389");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The index as this build reads it, on the fixture cut from the
    /// real one's head, refused rows and all: the shape of every
    /// column, a not-safe-for-work row among the rest to prove the
    /// parse leaves it out, and captions lowercased for the search.
    #[test]
    fn the_index_fixture_parses_and_drops_the_refused() {
        let bytes = include_bytes!("../fixtures/magick-index-head.tsv");
        let rows = parse_magick(bytes).expect("the fixture parses");
        assert_eq!(rows.len(), 25, "27 rows less the 2 refused");
        assert!(
            rows.iter().all(|row| row.id != "5bH7lMfQgi"),
            "the nsfw rows are left out"
        );
        assert!(
            rows.iter()
                .all(|row| row.lowered == row.subject.to_lowercase())
        );
        assert!(
            rows[0].subject.starts_with("stressed out asian woman"),
            "the columns land where the parse reads them"
        );
    }
}
