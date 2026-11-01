// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The sticker shelf: Microsoft's Fluent Emoji, one thousand two hundred
//! and eighty-five 3D PNGs under the MIT licence, searched in a local
//! index, fetched one picture at a time.
//!
//! The set - [microsoft/fluentui-emoji] on GitHub - is every emoji the
//! project carries, in its 3D style: rendered at 1024x1024 with real
//! transparency, free for commercial and personal use alike, which is
//! the whole reason this shelf exists. The two shelves it replaces were
//! not: MAGICK's pictures were CC BY-NC - unusable in a monetized video
//! - and OpenMoji's were CC BY-SA, and a sticker library that has to be
//! explained before it can be used is a liability wearing a search box.
//! The swap was the fix, not a bigger warning.
//!
//! What the page and the agent need once is the index: ~135 KB of TSV -
//! every emoji's codepoints, its CLDR name, its keywords, its group, its
//! licence and the path of its PNG in the repository - published as a
//! release asset of this repository, checked against a digest baked into
//! the build, and parsed into memory once by
//! [scripts/fluent-emoji-index.py]'s own output. The pictures themselves
//! stay where Microsoft keeps them: one plain HTTPS GET each, from
//! raw.githubusercontent.com with jsDelivr's mirror behind it.
//!
//! The search reads the words the set's own metadata says: the CLDR name
//! and the keywords together, so "party" finds the popper and "smile"
//! the grinning face. The engine is the one the sticker page always
//! had: terms ANDed and prefix-matched, ranked by where and how rarely
//! the words landed, the whole phrase as one run counting most.
//!
//! Two doors lead here. The library pane's Stickers page is the person's:
//! a search field, cards with names and groups, a click to preview, a
//! plus to lay an emoji over the video at the playhead, a heart to keep
//! it on the list. The API's `magick.*` methods are the agent's - the
//! names stayed for compatibility, and reach the same index - so an
//! agent can pick an emoji by its words without a window, then import
//! and place it in one call.
//!
//! [microsoft/fluentui-emoji]: https://github.com/microsoft/fluentui-emoji
//! [scripts/fluent-emoji-index.py]: ../../scripts/fluent-emoji-index.py

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::AppDirs;

/// Where the Fluent Emoji index is published: a release asset on this
/// repository, so the download needs no account and no token. The digest
/// beside it is baked into this build; a new index is a new release tag
/// and a new constant, and a file whose bytes disagree is re-downloaded.
pub const INDEX_URL: &str = "https://github.com/coreselfmastery/FUNCUT/releases/download/fluent-emoji-index-v1/fluent_index.tsv";

/// The index file's SHA-256, lowercase hex. A download that does not
/// match is deleted, not installed.
pub const INDEX_SHA256: &str = "bd12802847f2ec64787a89c17e46a792abbaa9fee7152b9e943a50600545cc8a";

/// The repository the shelf's pictures live in. A picture is one plain
/// GET of `…/main/<its path in the repository>`, the path exactly as the
/// index carries it.
pub const ASSET_URL: &str = "https://raw.githubusercontent.com/microsoft/fluentui-emoji/main";

/// The same pictures through jsDelivr's mirror of the repository, for
/// when raw.githubusercontent.com does not reach this machine - the same
/// bytes, and the PNG check below is what either is believed by.
pub const ASSET_MIRROR: &str = "https://cdn.jsdelivr.net/gh/microsoft/fluentui-emoji@main";

/// How big the index is, for the download's progress bar: ~135 KB.
const INDEX_BYTES: u64 = 140_000;

/// Where the page's own files live: `<data>/magick/`, the index beside
/// the pictures and the favourites - the folder keeps the name the two
/// shelves it replaces wrote, so a machine that held the old library
/// does not carry two. `data` is large state by the app's convention,
/// which is right for both.
pub fn dir(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("magick")
}

/// How many words a search will read, at most. A sentence of glue and
/// twelve words is a sentence the search can answer; the thirteenth
/// word has stopped being the ask.
const MOST_WORDS: usize = 12;

/// The shelf, as a filter names it and a result carries it. One shelf:
/// the enum stays because the API's words and the pane's state were
/// shaped around it, and the names a caller may still say - `magick`,
/// `openmoji`, `emoji` - all mean this one now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Microsoft's Fluent Emoji: 1,285 3D PNGs, MIT.
    Fluent,
}

impl Source {
    /// The shelf's own name, as the API's words and the pane's rows say
    /// it.
    pub fn name(self) -> &'static str {
        match self {
            Source::Fluent => "fluent",
        }
    }

    /// The shelf a name means; `None` for one that is no shelf's. The
    /// old shelves' names still answer - a script or an agent that says
    /// `magick` or `openmoji` is pointed at the one shelf rather than
    /// refused, because the shelf a name means is a fact about this
    /// build, not about the caller's history.
    pub fn parse(name: &str) -> Option<Source> {
        match name.to_ascii_lowercase().as_str() {
            "fluent" | "fluent-emoji" | "magick" | "openmoji" | "emoji" => Some(Source::Fluent),
            _ => None,
        }
    }

    /// The shelf's seat among the library's per-shelf arrays: a
    /// download, and its progress, are a shelf's own. One shelf, one
    /// seat; the array stays because the pane's polling was shaped
    /// around it.
    fn seat(self) -> usize {
        match self {
            Source::Fluent => 0,
        }
    }

    /// The shelf, in the order the page lists it.
    pub const ALL: [Source; 1] = [Source::Fluent];
}

/// One picture of the shelf, as a search result carries it and as the
/// API reports it. The subject is what the search read; the bytes
/// themselves arrive only when the picture is picked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sticker {
    /// The picture's id - its codepoints, `1f389` for the popper, a
    /// hyphen-joined run for a sequence - and the id every verb takes
    /// back.
    pub id: String,
    /// Which shelf it is on.
    pub source: Source,
    /// What the picture shows, in the set's own words: the CLDR name,
    /// "party popper".
    pub subject: String,
    /// The licence the picture carries, as the index read it: `MIT`.
    /// Carried per picture rather than assumed of the shelf, so the
    /// answer to "can I put this in a monetized video" is a field, not
    /// a footnote.
    pub license: String,
}

/// What a search asks for. Every field is optional; an all-`None` search
/// is "show me stickers", the shelf as it comes. The same type serves
/// the window's page and the API's `magick.search`, so the two cannot
/// disagree about what a filter means.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Words to find in the names and keywords. Terms are ANDed and
    /// match the front of a word: `party popper` finds the emoji holding
    /// both a word starting "party" and one starting "popper", ranked by
    /// relevance.
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
/// all - the count so the page can say "42 stickers" without fetching
/// forty-two cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The stickers, `limit` of them from `offset`.
    pub stickers: Vec<Sticker>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// Where the shelf stands, as the page and the API report it. Cheap to
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

/// One row of a parsed index: the picture's id, its CLDR name as a card
/// shows it, the same name with the keywords and the group folded in -
/// which is the one string the search reads, precomputed once so a
/// search never lowercases per row - and the path and licence a fetch
/// and an answer need.
#[derive(Clone, Debug)]
struct Row {
    id: String,
    subject: String,
    lowered: String,
    path: String,
    license: String,
}

/// The shelf's parsed index, held by the one library per process.
#[derive(Default)]
struct Shelves {
    fluent: Vec<Row>,
}

impl Shelves {
    /// The rows of one shelf.
    fn of(&self, source: Source) -> &Vec<Row> {
        match source {
            Source::Fluent => &self.fluent,
        }
    }
}

/// The whole library in one place: its index's state, its parsed rows
/// and the pictures on disk. One per process, reached through
/// [`shared`], because the window and its embedded API server are one
/// process and a download started twice would race itself; a *separate*
/// process - the headless MCP server - gets its own instance, and the
/// two are safe together because an index is only ever replaced by an
/// atomic rename.
pub struct Library {
    dirs: AppDirs,
    /// The parsed index, once opened. Opened once and kept: parsing
    /// 135 KB of TSV per keystroke would be absurd, and per search
    /// merely wasteful.
    shelves: OnceLock<Mutex<Shelves>>,
    /// Where the shelf's download stands, as a per-cent 0-100. An
    /// atomic rather than the shelves' mutex, so a download's worker
    /// can tick it without taking a lock a search is reading.
    progress: [AtomicU32; 1],
    /// Set while a download is running in *this* process, so a second
    /// `prepare` - from the pane and the window's API server at once,
    /// say - cannot start a second one. Across processes, the partial
    /// file's name carries the pid, so two downloaders never write the
    /// same file either.
    running: [AtomicBool; 1],
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
        progress: [AtomicU32::new(0)],
        running: [AtomicBool::new(false)],
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

    /// How many pictures the shelf holds, once ready.
    pub fn rows(&self, source: Source) -> Option<u64> {
        self.shelves
            .get()
            .and_then(|shelves| shelves.lock().ok())
            .map(|held| held.of(source).len() as u64)
    }

    /// Brings the shelf's index in: downloaded once - from this
    /// repository's release, checked against [`INDEX_SHA256`] - and
    /// parsed. Blocks - run it on a worker - and reports its per-cent
    /// through the status the page is already polling. A download
    /// already running in this process is not an error, just nothing
    /// to do.
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
            let estimate = INDEX_BYTES;
            match crate::models::download(
                &url,
                &partial,
                estimate,
                cancel,
                "cancelled",
                &mut report,
            ) {
                Ok(_) => {
                    // The index is a frozen build: whatever source
                    // answered, the bytes must be the ones this build
                    // was built against.
                    let checked = crate::models::verify(&partial, INDEX_SHA256)
                        .map_err(|error| format!("the Fluent Emoji index did not verify: {error}"));
                    match checked {
                        Ok(()) => {
                            std::fs::rename(&partial, &target).map_err(|error| {
                                format!("could not keep {}: {error}", target.display())
                            })?;
                            if let Err(error) = self.open_index(source) {
                                // A digest that matched and a parse that
                                // refused is a build's own argument with
                                // itself: the file stands, said in words.
                                return Err(error);
                            }
                            return Ok(());
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

    /// Parses the shelf's index into the shelves, if it is not open
    /// already. A second opener - the pane and the API server at once -
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
            Source::Fluent => parse_fluent(&bytes)?,
        };
        let mut held = shelves
            .lock()
            .map_err(|_| "the sticker library's index is busy".to_owned())?;
        if held.of(source).is_empty() {
            match source {
                Source::Fluent => held.fluent = parsed,
            }
        }
        Ok(())
    }

    /// Runs a query against the shelf. Every filter is optional; a
    /// search against a shelf that is not ready is refused with the
    /// words that say what to do about it.
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
        let wanted = query.source.unwrap_or(Source::Fluent);
        if held.of(wanted).is_empty() {
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
        for row in held.of(wanted) {
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
        // The scoring pass: a word scores where it landed - whole (+20)
        // or as a prefix (+12) - scaled by its rarity, `ln((N+1)/(df+1))`
        // turned into half again to a half more, so a word every caption
        // holds is still worth half its points and a word two captions
        // share is worth about half again. The whole phrase said as one
        // run of words is worth +100, and a caption pays a fifth of a
        // point a character for its sprawl.
        let mut scored: Vec<(i64, &Row)> = Vec::new();
        for row in held.of(wanted) {
            if hearts
                .as_ref()
                .is_some_and(|hearts| !hearts.contains(&row.id))
            {
                continue;
            }
            if words.is_empty() {
                scored.push((0, row));
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
            scored.push((score, row));
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
            .map(|(_, row)| Sticker {
                id: row.id.clone(),
                source: wanted,
                subject: row.subject.clone(),
                license: row.license.clone(),
            })
            .collect();
        Ok(Found { stickers, total })
    }

    /// One picture by id.
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
                license: row.license.clone(),
            })
    }

    /// One picture by id, from whichever shelf holds it - one shelf now,
    /// so this is the one - kept for the callers that never cared which.
    pub fn find(&self, id: &str) -> Option<Sticker> {
        self.sticker(Source::Fluent, id)
    }

    /// Where a picture is kept once fetched: `images/{id}.png`, or
    /// `None` when it is not on this machine. The id is the set's own -
    /// its codepoints - stable for as long as the set is, so a picture
    /// fetched twice is fetched once.
    pub fn fetched(&self, id: &str) -> Option<PathBuf> {
        let path = image_path(&self.dirs, id);
        path.is_file().then_some(path)
    }

    /// Fetches a picture into the cache - one plain GET of its own PNG,
    /// a few hundred KB of rendered 3D - and says where it landed. The
    /// bytes are checked to be a PNG that can hold transparency before
    /// they are kept. Blocks: a worker's.
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
        // The path comes from the index the digest has already vouched
        // for, but a fetch refuses an id the index never said all the
        // same: the guard is cheap and the alternative is a request
        // shaped by whatever an id carried in. `sticker` first, because
        // it is the one that opens an index a search has not run
        // against yet.
        self.sticker(source, id)
            .ok_or_else(|| format!("there is no picture {id}"))?;
        let path = {
            let held = self
                .shelves
                .get()
                .and_then(|shelves| shelves.lock().ok())
                .ok_or_else(|| "the sticker library's index is busy".to_owned())?;
            held.of(source)
                .iter()
                .find(|row| row.id == id)
                .map(|row| row.path.clone())
                .ok_or_else(|| format!("there is no picture {id}"))?
        };
        let urls = asset_urls(&path).ok_or_else(|| format!("{id} has no usable path"))?;
        let folder = images_dir(&self.dirs);
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
        let name = target.file_name().map_or_else(
            || "sticker.png".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let partial = folder.join(format!("{name}.partial-{}", std::process::id()));
        let mut last = String::new();
        // raw.githubusercontent.com first, jsDelivr behind it: the same
        // bytes, and the check below is what either is believed by.
        for url in urls {
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
    /// newest first.
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

/// Where the shelf's index is kept.
fn index_path(dirs: &AppDirs, source: Source) -> PathBuf {
    match source {
        Source::Fluent => dir(dirs).join("fluent_index.tsv"),
    }
}

/// The pictures' folder.
fn images_dir(dirs: &AppDirs) -> PathBuf {
    dir(dirs).join("images")
}

/// Where one picture is kept once fetched.
fn image_path(dirs: &AppDirs, id: &str) -> PathBuf {
    images_dir(dirs).join(format!("{id}.png"))
}

/// Where to fetch a picture from, in order: the repository itself, then
/// jsDelivr's mirror of it. `None` for a path that cannot be one, so a
/// fetch refuses rather than asking for a file that cannot exist.
fn asset_urls(path: &str) -> Option<Vec<String>> {
    // The index's paths are `assets/<name>/3D/<file>_3d.png`; the guard
    // keeps a path that is anything else - a climb out, an absolute
    // path, an empty one - from ever reaching a URL.
    if !path.starts_with("assets/")
        || !path.ends_with("_3d.png")
        || path.contains("..")
        || path.contains('\\')
    {
        return None;
    }
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    Some(vec![
        format!("{ASSET_URL}/{encoded}"),
        format!("{ASSET_MIRROR}/{encoded}"),
    ])
}

/// Where the index download tries, in order. Every source that could
/// hold the index is in the list once; a mirror that cannot be reached
/// costs a retry rather than a shelf.
fn index_sources(_source: Source) -> Vec<String> {
    vec![INDEX_URL.to_owned()]
}

/// The Fluent Emoji index, as [scripts/fluent-emoji-index.py] writes it:
/// a header row, then a row an emoji - `unicode`, `cldr`, `keywords`,
/// `group`, `license`, `path`. The licence column is read, not assumed:
/// a row that says something other than MIT is still a row, and the
/// answer to "which licence" is the field, not the shelf's reputation.
///
/// [scripts/fluent-emoji-index.py]: ../../scripts/fluent-emoji-index.py
fn parse_fluent(bytes: &[u8]) -> Result<Vec<Row>, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| "the Fluent Emoji index is not UTF-8".to_owned())?;
    let mut rows = Vec::new();
    for line in text.split('\n').skip(1) {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(id), Some(subject), Some(keywords), Some(_group), Some(license), Some(path)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        let subject = subject.trim();
        if id.trim().is_empty() || subject.is_empty() {
            continue;
        }
        let lowered = format!("{} {}", subject.to_lowercase(), keywords.to_lowercase());
        rows.push(Row {
            id: id.trim().to_owned(),
            subject: subject.to_owned(),
            lowered,
            path: path.trim().to_owned(),
            license: license.trim().to_owned(),
        });
    }
    if rows.is_empty() {
        return Err("the Fluent Emoji index holds no pictures".to_owned());
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
/// (truecolour with an alpha channel, what the 3D style renders) or
/// colour type 3 with a `tRNS` chunk (a palette PNG that says which of
/// its colours are see-through).
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

    /// A library over a scratch folder, its shelf parsed in - the
    /// shape an open index leaves behind - ready to search.
    fn made(name: &str, rows: Shelves) -> (Library, PathBuf) {
        let dir = scratch(name);
        (
            Library {
                dirs: AppDirs::under(&dir),
                shelves: OnceLock::from(Mutex::new(rows)),
                progress: [AtomicU32::new(0)],
                running: [AtomicBool::new(false)],
                favourites: Mutex::new(None),
            },
            dir,
        )
    }

    /// A little of the shelf to search, in the shape the index has.
    fn shelves() -> Shelves {
        Shelves {
            fluent: vec![
                Row {
                    id: "1f44b".into(),
                    subject: "waving hand".into(),
                    lowered: "waving hand hand wave waving".into(),
                    path: "assets/Waving hand/3D/waving_hand_3d.png".into(),
                    license: "MIT".into(),
                },
                Row {
                    id: "1f389".into(),
                    subject: "party popper".into(),
                    lowered: "party popper celebration congrats party tada".into(),
                    path: "assets/Party popper/3D/party_popper_3d.png".into(),
                    license: "MIT".into(),
                },
                Row {
                    id: "1f600".into(),
                    subject: "grinning face".into(),
                    lowered: "grinning face cheerful cheery face grin happy smile".into(),
                    path: "assets/Grinning face/3D/grinning_face_3d.png".into(),
                    license: "MIT".into(),
                },
                Row {
                    id: "1f5a4".into(),
                    subject: "black heart".into(),
                    lowered: "black heart black heart".into(),
                    path: "assets/Black heart/3D/black_heart_3d.png".into(),
                    license: "MIT".into(),
                },
            ],
        }
    }

    /// The TSV as the build script writes it - header, six columns - parses
    /// into rows whose search words are the CLDR name and the keywords
    /// together, and whose licence and path are carried as fields.
    #[test]
    fn the_fluent_index_parses_its_own_shape() {
        let tsv = "unicode\tcldr\tkeywords\tgroup\tlicense\tpath\n\
                   1f44b\twaving hand\thand wave waving\tPeople & Body\tMIT\tassets/Waving hand/3D/waving_hand_3d.png\n\
                   1f600\tgrinning face\tcheerful smile\tSmileys & Emotion\tMIT\tassets/Grinning face/3D/grinning_face_3d.png\n\
                   \t\t\t\t\t\n\
                   1f947\t1st place medal\tfirst gold medal\tActivities\tMIT\tassets/1st place medal/3D/1st_place_medal_3d.png\n";
        let rows = parse_fluent(tsv.as_bytes()).expect("parses");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, "1f44b");
        assert_eq!(rows[0].subject, "waving hand");
        assert!(rows[0].lowered.starts_with("waving hand"));
        assert!(rows[0].lowered.contains("wave"));
        assert_eq!(rows[0].license, "MIT");
        assert!(rows[0].path.ends_with("_3d.png"));
        assert!(
            parse_fluent(b"unicode\tbroken\n").is_err(),
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
        assert_eq!(ids("party"), vec!["1f389".to_owned()]);
        // The keywords are read beside the name: "tada" finds the popper
        // by a word the name alone does not say.
        assert_eq!(ids("tada"), vec!["1f389".to_owned()]);
        // And "smile" the grinning face.
        assert_eq!(ids("smile"), vec!["1f600".to_owned()]);
        assert!(ids("spaceship").is_empty());
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
            4
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
                text: Some("waving hand".into()),
                limit: 100,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(found.stickers[0].id, "1f44b", "the phrase, said whole");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A search against a shelf with no index is a refusal with the
    /// words that say what to do about it, not a quiet empty page.
    #[test]
    fn an_unopened_shelf_is_a_refusal() {
        let (fresh, fresh_dir) = made("refused", Shelves::default());
        let error = fresh
            .search(&Query {
                source: Some(Source::Fluent),
                ..Query::default()
            })
            .unwrap_err();
        assert!(error.contains("magick.prepare"), "{error}");
        let _ = std::fs::remove_dir_all(&fresh_dir);
    }

    /// Every result carries the licence its row read, so the answer to
    /// "can I ship this" is a field on the sticker.
    #[test]
    fn every_result_carries_its_licence() {
        let (library, dir) = made("licence", shelves());
        let found = library
            .search(&Query {
                text: Some("party".into()),
                ..Query::default()
            })
            .unwrap();
        assert_eq!(found.stickers.len(), 1);
        assert_eq!(found.stickers[0].license, "MIT");
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(found.total, 4);
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

    /// The picture URLs are the repository's own shape - the path the
    /// index carries, percent-encoded, from the repository and its
    /// jsDelivr mirror - and a path that cannot be one is refused rather
    /// than asked for.
    #[test]
    fn the_picture_urls_are_the_repositorys_own() {
        let urls = asset_urls("assets/1st place medal/3D/1st_place_medal_3d.png")
            .expect("a real path answers");
        assert_eq!(
            urls[0],
            "https://raw.githubusercontent.com/microsoft/fluentui-emoji/main/assets/1st%20place%20medal/3D/1st_place_medal_3d.png"
        );
        assert_eq!(
            urls[1],
            "https://cdn.jsdelivr.net/gh/microsoft/fluentui-emoji@main/assets/1st%20place%20medal/3D/1st_place_medal_3d.png"
        );
        assert!(asset_urls("../escape").is_none());
        assert!(asset_urls("assets/../escape.png").is_none());
        assert!(asset_urls("/absolute.png").is_none());
        assert!(asset_urls("assets/colour.png").is_none(), "not a 3D png");
        assert!(asset_urls("assets/emoji\\back.png").is_none());
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
            progress: [AtomicU32::new(0)],
            running: [AtomicBool::new(false)],
            favourites: Mutex::new(None),
        };
        assert!(!library.is_favourite("1f44b"));
        assert!(library.toggle_favourite("1f44b").unwrap());
        assert!(library.toggle_favourite("1f389").unwrap());
        assert!(library.is_favourite("1f389"));
        assert_eq!(
            library.favourite_ids(),
            vec!["1f389".to_owned(), "1f44b".to_owned()],
            "newest first"
        );
        assert!(!library.toggle_favourite("1f389").unwrap());
        assert!(!library.is_favourite("1f389"));
        let reread = Library {
            dirs,
            shelves: OnceLock::new(),
            progress: [AtomicU32::new(0)],
            running: [AtomicBool::new(false)],
            favourites: Mutex::new(None),
        };
        assert_eq!(reread.favourite_ids(), vec!["1f44b".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The favourites filter narrows a search to the heart's list.
    #[test]
    fn the_hearts_filter_narrows_a_search() {
        let (library, dir) = made("hearts", shelves());
        library.toggle_favourite("1f389").unwrap();
        let found = library
            .search(&Query {
                favourites: true,
                limit: 100,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(found.stickers.len(), 1);
        assert_eq!(found.stickers[0].id, "1f389");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The index as this build reads it, on the fixture cut from the
    /// real one's head: the shape of every column, the licence read
    /// rather than assumed, and the search words the name and the
    /// keywords together.
    #[test]
    fn the_index_fixture_parses() {
        let bytes = include_bytes!("../fixtures/fluent-index-head.tsv");
        let rows = parse_fluent(bytes).expect("the fixture parses");
        assert_eq!(rows.len(), 20);
        assert!(rows.iter().all(|row| row.license == "MIT"));
        assert!(
            rows.iter()
                .all(|row| row.lowered.starts_with(&row.subject.to_lowercase()))
        );
        assert!(
            rows.iter()
                .all(|row| row.path.starts_with("assets/") && row.path.ends_with("_3d.png"))
        );
    }

    /// The real shelf answers: the index down from the release and
    /// verified against the digest baked into this build, parsed into
    /// 1,285 rows, a search over it, and one picture fetched through its
    /// own plain GET and checked to be a PNG that can hold transparency
    /// before it is kept - the second fetch of the same picture the same
    /// file, fetched once. Marked `ignore` because it moves real bytes
    /// and needs the network; run it with
    /// `cargo test -p concat-host magick -- --ignored` when a source or
    /// this module changes.
    #[test]
    #[ignore]
    fn the_real_sources_answer() {
        let dir = scratch("live");
        let dirs = AppDirs::under(&dir);
        let library = shared(&dirs);
        let cancel = AtomicBool::new(false);

        // The index arrives, verifies, and parses into the 1,285 emoji
        // this build was built against.
        library
            .prepare(Source::Fluent, &cancel)
            .expect("the Fluent Emoji index arrives");
        assert_eq!(
            library.rows(Source::Fluent),
            Some(1_285),
            "the shelf this build was built against"
        );

        // A search finds what the words say.
        let found = library
            .search(&Query {
                text: Some("party".to_owned()),
                limit: 5,
                ..Query::default()
            })
            .expect("searches");
        assert!(!found.stickers.is_empty(), "somebody is partying");
        assert!(
            found
                .stickers
                .iter()
                .all(|sticker| sticker.license == "MIT")
        );

        // A picture: fetched, checked, cached; fetched again from the
        // cache, the same file.
        let sticker = found.stickers[0].clone();
        let path = library
            .fetch(sticker.source, &sticker.id, &cancel, &mut |_, _| {})
            .expect("the picture arrives");
        assert!(path.is_file());
        let again = library
            .fetch(sticker.source, &sticker.id, &cancel, &mut |_, _| {})
            .expect("the picture is kept");
        assert_eq!(again, path, "fetched once");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
