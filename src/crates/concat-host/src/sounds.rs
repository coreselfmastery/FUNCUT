// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The user's own sound library: the Audio page's Mine shelf.
//!
//! A sound brought in here is copied into `<library>/audio/` (see
//! [`AppDirs::library`]), so it is there in every project and survives the
//! original being moved or deleted. Beside the copies is `library.json`,
//! the index: each sound's name, its file, how long it is and a small
//! envelope of it - sixty-four bars, read once at import - so the shelf
//! draws every card without decoding anything when it opens.
//!
//! Beside them, Concat's own sounds: a manifest read from the assets
//! repository when the shelf is first asked for, and each sound fetched
//! on first use into `<data>/sounds/`, checked against its digest; see
//! `docs/AUDIO_LIBRARY.md`.
//!
//! Nothing here touches a project. A sound put on the timeline is copied
//! again into that project's own folder by the window, so a project never
//! points into the library and removing a sound from it breaks nothing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::AppDirs;

/// How many bars a card's envelope has.
pub const BARS: usize = 64;

/// The extensions the library takes: sound files, the way the bin's
/// import names them. A video's soundtrack is not a library sound.
pub const EXTENSIONS: [&str; 9] = [
    "mp3", "wav", "aac", "m4a", "flac", "ogg", "opus", "aif", "aiff",
];

/// One sound in the library.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    /// Stable for as long as the sound is in the library; what a drag
    /// carries and what the index is keyed by.
    pub id: String,
    /// What the card says: the file's name without its extension, at
    /// first.
    pub name: String,
    /// The copy's file name inside the library folder.
    pub file: String,
    /// Seconds.
    pub duration: f64,
    /// A shelf of the user's choosing; empty for none. Kept for the day
    /// the shelf offers categories of one's own.
    #[serde(default)]
    pub category: String,
    /// The envelope: [`BARS`] peaks, each `0..=1`, quantised to hundredths.
    #[serde(default)]
    pub bars: Vec<f32>,
    /// When it came in, as Unix seconds: the shelf's order, newest first.
    #[serde(default)]
    pub added: u64,
}

/// The index on disk.
#[derive(Serialize, Deserialize, Default)]
struct Index {
    #[serde(default)]
    sounds: Vec<Sound>,
}

/// The library: its folder and what is in it.
#[derive(Clone, Debug, Default)]
pub struct Library {
    /// The folder: the copies and the index beside them.
    pub dir: PathBuf,
    /// Newest first.
    pub sounds: Vec<Sound>,
}

impl Library {
    /// Where the library lives on this machine.
    pub fn dir_in(dirs: &AppDirs) -> PathBuf {
        dirs.library().join("audio")
    }

    /// The library as the index has it. A missing or unreadable index is
    /// an empty library, and a sound whose copy has gone is left out.
    pub fn open(dirs: &AppDirs) -> Library {
        Library::at(Self::dir_in(dirs))
    }

    /// The library in `dir`; see [`Library::open`].
    pub fn at(dir: PathBuf) -> Library {
        let index: Index = std::fs::read(dir.join("library.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let sounds = index
            .sounds
            .into_iter()
            .filter(|sound| is_plain_name(&sound.file) && dir.join(&sound.file).is_file())
            .collect();
        Library { dir, sounds }
    }

    /// The copy of `sound` on disk.
    pub fn path_of(&self, sound: &Sound) -> PathBuf {
        self.dir.join(&sound.file)
    }

    /// The sound by its id.
    pub fn get(&self, id: &str) -> Option<&Sound> {
        self.sounds.iter().find(|sound| sound.id == id)
    }

    /// Writes the index, through a temporary file and a rename, so a crash
    /// mid-write leaves the last one whole.
    fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|error| format!("could not create {}: {error}", self.dir.display()))?;
        let index = Index {
            sounds: self.sounds.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&index).map_err(|error| error.to_string())?;
        let path = self.dir.join("library.json");
        let partial = self.dir.join("library.json.partial");
        std::fs::write(&partial, bytes)
            .and_then(|()| std::fs::rename(&partial, &path))
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    }

    /// Takes in a sound [`prepare`] made, and writes the index.
    pub fn add(&mut self, sound: Sound) -> Result<(), String> {
        self.sounds.retain(|held| held.id != sound.id);
        self.sounds.insert(0, sound);
        self.save()
    }

    /// Deletes the copy and forgets the sound. A copy already gone is not
    /// an error: what the person asked for is that it be gone.
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let Some(at) = self.sounds.iter().position(|sound| sound.id == id) else {
            return Ok(());
        };
        let sound = self.sounds.remove(at);
        let path = self.path_of(&sound);
        if let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            self.sounds.insert(at, sound);
            return Err(format!("could not delete {}: {error}", path.display()));
        }
        self.save()
    }
}

/// Whether a path is a sound the library takes, by its extension.
pub fn is_sound(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// A file name with nothing in it that could walk out of a folder.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && Path::new(name).file_name().is_some_and(|file| file == name)
}

/// Copies `source` into the library folder `dir` and reads what its card
/// needs: how long it is, and its envelope. Slow - a decode of the whole
/// file - so a worker's; [`Library::add`] then takes the result in on the
/// window's thread.
///
/// The copy keeps the file's own name, numbered when the folder already
/// has one by it, and a file that is not sound is refused before anything
/// is copied.
pub fn prepare(dir: &Path, source: &Path) -> Result<Sound, String> {
    if !is_sound(source) {
        return Err(format!("{} is not a sound file", source.display()));
    }
    let summary = crate::media::probe(&source.to_string_lossy())?;
    if summary.audio.is_none() {
        return Err(format!("{} has no sound in it", source.display()));
    }
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Sound".to_owned());
    let extension = source
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let file = free_name(dir, &stem, &extension);
    let target = dir.join(&file);
    std::fs::copy(source, &target)
        .map_err(|error| format!("could not copy {}: {error}", source.display()))?;
    let duration = summary.duration.unwrap_or(0.0);
    let bars = envelope(&target, duration).unwrap_or_default();
    let added = now();
    Ok(Sound {
        id: format!(
            "{:012x}",
            crate::media::fnv1a(format!("{file}|{added}").as_bytes()) & 0xffff_ffff_ffff
        ),
        name: stem,
        file,
        duration,
        category: String::new(),
        bars,
        added,
    })
}

/// `stem.extension` in `dir`, or `stem 2.extension` and on, whichever is
/// free, with anything that could not be a plain file name taken out.
fn free_name(dir: &Path, stem: &str, extension: &str) -> String {
    let clean: String = stem
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
    let clean = if clean.is_empty() { "Sound" } else { clean };
    let named = |n: u32| {
        let stem = if n == 1 {
            clean.to_owned()
        } else {
            format!("{clean} {n}")
        };
        if extension.is_empty() {
            stem
        } else {
            format!("{stem}.{extension}")
        }
    };
    (1..)
        .map(named)
        .find(|name| name != "library.json" && !dir.join(name).exists())
        .unwrap_or_else(|| "Sound".to_owned())
}

/// The envelope a card draws: [`BARS`] peaks across the whole file.
pub fn envelope(path: &Path, duration: f64) -> Option<Vec<f32>> {
    // A hundred buckets a second is plenty for sixty-four bars, and a
    // tenth of the work of the timeline's resolution.
    let peaks = concat_media::peaks::extract(path, 100, None).ok()?;
    let span = if duration > 0.0 {
        duration as f32
    } else {
        peaks.len() as f32 / peaks.buckets_per_second.max(1.0)
    };
    if span <= 0.0 {
        return None;
    }
    let pitch = span / BARS as f32;
    Some(
        (0..BARS)
            .map(|bar| {
                let (low, high) = peaks.extremes(bar as f32 * pitch, (bar + 1) as f32 * pitch);
                (high.max(-low).clamp(0.0, 1.0) * 100.0).round() / 100.0
            })
            .collect(),
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

// ── Concat's own sounds ─────────────────────────────────────────────────

/// The Concat shelf's manifest, as `concatapp/assets/audio/manifest.json`
/// has it; the schema is `docs/AUDIO_LIBRARY.md`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Catalogue {
    /// The format: 1.
    #[serde(default)]
    pub version: u32,
    /// The sounds, in the order the shelf shows them.
    #[serde(default)]
    pub sounds: Vec<Listed>,
}

/// One sound of Concat's.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Listed {
    /// Stable for ever: `[a-z0-9-]`.
    pub id: String,
    /// What the row shows.
    pub name: String,
    /// Whoosh, Impact, Riser, Ambience, Music, UI, Motion, Notification,
    /// Keyboard, Digital, Foley or Writing.
    #[serde(default)]
    pub category: String,
    /// The file's name in the repository's `audio/` folder.
    pub file: String,
    /// Seconds.
    #[serde(default)]
    pub duration: f64,
    /// Its size, for the progress bar; 0 when not given.
    #[serde(default)]
    pub bytes: u64,
    /// CC0-1.0, and nothing else is shown.
    #[serde(default)]
    pub license: String,
    /// Where it was found.
    #[serde(default)]
    pub source: String,
    /// Who made it, when known.
    #[serde(default)]
    pub author: String,
    /// Lowercase hex: what the download must match.
    #[serde(default)]
    pub sha256: String,
    /// Words a search narrows by, beyond the name: `["fast", "cinematic",
    /// "transition"]`. Optional; the manifest FUNCUT ships carries them,
    /// and the API's `sounds.search` reads them.
    #[serde(default)]
    pub tags: Vec<String>,
    /// What the sound is, in a sentence a person or an agent can read
    /// before hearing anything: "A fast, crisp whip of air with a bright
    /// edge and a little push behind it." Optional; the search reads it,
    /// after the name and the tags, and the API hands it back so an
    /// agent can pick with it.
    #[serde(default)]
    pub description: String,
    /// What the sound is for, as short lines: `["Whip pans", "Fast cuts
    /// between scenes"]`. Optional; the search reads them, and the API
    /// hands them back. What a sound is *not* for is deliberately not
    /// carried: the description says so in its own words when it matters,
    /// and a search that matched what to avoid would find the wrong
    /// sounds on purpose.
    #[serde(default, rename = "useFor")]
    pub use_for: Vec<String>,
    /// Seconds into the file the sound lands its peak: the moment a move
    /// should meet, when the library says so. Optional; the API hands it
    /// back so an agent can place the file by the moment it is for
    /// rather than by its start.
    #[serde(default, rename = "syncAt")]
    pub sync_at: Option<f64>,
}

impl Listed {
    /// Whether the entry can be offered: a plain file name, an id of the
    /// shape the cache keys on, a digest to check against, and CC0.
    pub fn is_offered(&self) -> bool {
        let license = self.license.to_ascii_uppercase();
        !self.id.is_empty()
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && is_plain_name(&self.file)
            && self.sha256.len() == 64
            && self.sha256.chars().all(|c| c.is_ascii_hexdigit())
            && (license == "CC0" || license == "CC0-1.0")
    }
}

impl Catalogue {
    /// Read from the manifest's bytes, keeping only the entries that can be
    /// offered.
    pub fn parse(bytes: &[u8]) -> Result<Catalogue, String> {
        let mut catalogue: Catalogue = serde_json::from_slice(bytes)
            .map_err(|error| format!("the sound list is not readable: {error}"))?;
        catalogue.sounds.retain(Listed::is_offered);
        for sound in &mut catalogue.sounds {
            sound.sha256.make_ascii_lowercase();
        }
        Ok(catalogue)
    }

    /// The sound by its id.
    pub fn get(&self, id: &str) -> Option<&Listed> {
        self.sounds.iter().find(|sound| sound.id == id)
    }

    /// The catalogue narrowed by `query` and `category`, best first: the
    /// one search the shelf's field and the API's `sounds.search` both
    /// ask, so a person and an agent are answered by the same list. With
    /// neither set, the whole library in manifest order.
    pub fn search(&self, query: &str, category: Option<&str>) -> Vec<&Listed> {
        let words = query_words(query);
        let wanted = category.map(str::to_ascii_lowercase);
        let mut matched: Vec<&Listed> = self
            .sounds
            .iter()
            .filter(|sound| {
                wanted
                    .as_deref()
                    .is_none_or(|wanted| sound.category.to_ascii_lowercase() == wanted)
            })
            .filter(|sound| words.is_empty() || sound.score(&words).is_some())
            .collect();
        if !words.is_empty() {
            // Best first: what the name says outranks what the tags say,
            // which outranks what the description merely mentions. The
            // sort is stable, so sounds that score the same stay in the
            // order the manifest put them.
            matched.sort_by_key(|sound| std::cmp::Reverse(sound.score(&words)));
        }
        matched
    }
}

// ── the search ─────────────────────────────────────────────────────────

/// The words of a query: lowercased, cut at anything that is not a letter
/// or a digit, empties dropped - "Cinematic  IMPACT!" is cinematic and
/// impact. Digits stay, so "1s" and "808" are words. The little words
/// that only glue a sentence together - "a", "the", "for", "with" - are
/// dropped, so "soft swish for a card transition" asks what it means and
/// not one word more. They are dropped rather than matched: "in" and
/// "out" stay, being what a zip does, and "up" and "down" what a power
/// does.
fn query_words(query: &str) -> Vec<String> {
    const GLUE: [&str; 10] = [
        "a", "an", "the", "for", "with", "and", "or", "of", "to", "that",
    ];
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty() && !GLUE.contains(word))
        .map(str::to_owned)
        .collect()
}

/// The words of a sound's field, the same cuts [`query_words`] makes, so
/// "Whoosh, fast and hard" and "whoosh fast" speak the same language.
fn field_words(text: &str) -> Vec<String> {
    query_words(text)
}

/// Where `word` lands in `field`: `exact` for one of the field's own
/// words, `prefix` for the front of one ("whoosh" finds "whooshes"),
/// `None` when it matches nothing there.
fn hit(word: &str, field: &[String], exact: u32, prefix: u32) -> Option<u32> {
    if field.iter().any(|token| token == word) {
        Some(exact)
    } else if field.iter().any(|token| token.starts_with(word)) {
        Some(prefix)
    } else {
        None
    }
}

impl Listed {
    /// How well this sound answers the query's `words`: `None` when any
    /// word matches nowhere - the query is ANDed, as it always was - and
    /// otherwise a number that says where the words matched, so the
    /// search can sort by it. A word scores by its best field: the name
    /// counts most (a word of it 12, a prefix of it 9), then the tags
    /// (8 and 6), then the id and the category (5 and 4), then the
    /// use-for lines (3 and 2) and the description (2 and 1), which say
    /// what a sound is for rather than what it is. Every word of the
    /// query landing in the name adds 4 more, so "fast whoosh" puts
    /// *Whoosh, fast* ahead of a whoosh merely tagged fast.
    pub fn score(&self, words: &[String]) -> Option<u32> {
        if words.is_empty() {
            return Some(0);
        }
        let name = field_words(&self.name);
        let tags: Vec<String> = self.tags.iter().flat_map(|tag| field_words(tag)).collect();
        let id = field_words(&self.id);
        let category = field_words(&self.category);
        let used: Vec<String> = self
            .use_for
            .iter()
            .flat_map(|line| field_words(line))
            .collect();
        let description = field_words(&self.description);
        let mut total = 0;
        let mut all_in_name = true;
        for word in words {
            let in_name = hit(word, &name, 12, 9);
            let best = [
                in_name,
                hit(word, &tags, 8, 6),
                hit(word, &id, 5, 4),
                hit(word, &category, 5, 4),
                hit(word, &used, 3, 2),
                hit(word, &description, 2, 1),
            ]
            .into_iter()
            .flatten()
            .max()?;
            all_in_name &= in_name.is_some();
            total += best;
        }
        Some(total + u32::from(all_in_name) * 4)
    }
}

/// Where Concat's sounds and their manifest are cached: `<data>/sounds/`.
pub fn cache_dir(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("sounds")
}

/// The manifest as last fetched, when it has been.
pub fn cached_catalogue(dirs: &AppDirs) -> Option<Catalogue> {
    let bytes = std::fs::read(cache_dir(dirs).join("manifest.json")).ok()?;
    Catalogue::parse(&bytes).ok()
}

/// Fetches the manifest, and keeps it for the sessions after. `Ok(None)`
/// when there is none yet - the repository answers 404 until the pack is
/// published - which the shelf says as "coming soon" rather than as an
/// error.
pub fn fetch_catalogue(dirs: &AppDirs) -> Result<Option<Catalogue>, String> {
    use std::io::Read;
    let agent = crate::net::agent(
        std::time::Duration::from_secs(15),
        std::time::Duration::from_secs(30),
    );
    let response = match agent.get(crate::models::SOUNDS_MANIFEST).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(error) => return Err(format!("the sound list did not answer: {error}")),
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(4 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the sound list did not arrive: {error}"))?;
    let catalogue = Catalogue::parse(&bytes)?;
    let dir = cache_dir(dirs);
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("manifest.json"), &bytes);
    }
    Ok(Some(catalogue))
}

/// Where `sound` is kept once downloaded: its digest's head in the name,
/// so new bytes under the same name are a new file.
pub fn cached_file(dirs: &AppDirs, sound: &Listed) -> PathBuf {
    cache_dir(dirs).join(format!("{}-{}", &sound.sha256[..12], sound.file))
}

/// Downloads `sound` into the cache from the mirror, then the repository,
/// and checks it against its digest; a file that does not match is
/// deleted. Returns where it is. Blocks: a worker's.
pub fn fetch_sound(
    dirs: &AppDirs,
    sound: &Listed,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let target = cached_file(dirs, sound);
    if target.is_file() {
        return Ok(target);
    }
    let dir = cache_dir(dirs);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let partial = target.with_extension("partial");
    let mut last = String::new();
    for url in crate::models::sound_sources(&sound.file) {
        match crate::models::download(&url, &partial, sound.bytes, cancel, "cancelled", progress) {
            Ok(_) => match crate::models::verify(&partial, &sound.sha256) {
                Ok(()) => {
                    std::fs::rename(&partial, &target)
                        .map_err(|error| format!("could not keep {}: {error}", target.display()))?;
                    return Ok(target);
                }
                Err(error) => {
                    let _ = std::fs::remove_file(&partial);
                    last = error;
                }
            },
            Err(error) if error == "cancelled" => return Err(error),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("concat-sounds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_name_is_numbered_rather_than_overwritten() {
        let dir = scratch("names");
        assert_eq!(free_name(&dir, "Whoosh", "wav"), "Whoosh.wav");
        std::fs::write(dir.join("Whoosh.wav"), b"x").unwrap();
        assert_eq!(free_name(&dir, "Whoosh", "wav"), "Whoosh 2.wav");
        assert_eq!(free_name(&dir, "../up", "wav"), "_up.wav");
        assert_eq!(free_name(&dir, "  ", "mp3"), "Sound.mp3");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_sound_files_are_taken() {
        assert!(is_sound(Path::new("/a/Rain.WAV")));
        assert!(is_sound(Path::new("b.m4a")));
        assert!(!is_sound(Path::new("clip.mp4")));
        assert!(!is_sound(Path::new("noextension")));
    }

    /// The index keeps what is added, newest first, survives a reopen, and
    /// forgets a sound whose copy has gone; a removal deletes the copy.
    #[test]
    fn the_index_round_trips_and_a_removal_deletes_the_copy() {
        let dir = scratch("index");
        let mut library = Library::at(dir.clone());
        assert!(library.sounds.is_empty());
        for (id, file) in [("a", "One.wav"), ("b", "Two.wav")] {
            std::fs::write(dir.join(file), b"riff").unwrap();
            library
                .add(Sound {
                    id: id.into(),
                    name: file.trim_end_matches(".wav").into(),
                    file: file.into(),
                    duration: 1.5,
                    category: String::new(),
                    bars: vec![0.5; BARS],
                    added: 1,
                })
                .unwrap();
        }
        let reopened = Library::at(dir.clone());
        assert_eq!(
            reopened
                .sounds
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(reopened.get("a").unwrap().bars.len(), BARS);

        library.remove("b").unwrap();
        assert!(!dir.join("Two.wav").exists());
        std::fs::remove_file(dir.join("One.wav")).unwrap();
        assert!(
            Library::at(dir.clone()).sounds.is_empty(),
            "a missing copy is left out"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only CC0 entries with a digest and a plain file name are offered.
    #[test]
    fn a_manifest_offers_only_what_it_can_check() {
        let digest = "A".repeat(64);
        let bytes = format!(
            r#"{{"version":1,"sounds":[
                {{"id":"whoosh-1","name":"Whoosh","category":"Whoosh","file":"w.ogg","duration":1.2,"license":"CC0-1.0","source":"s","sha256":"{digest}"}},
                {{"id":"bad","name":"No digest","file":"x.ogg","license":"CC0"}},
                {{"id":"paid","name":"Not CC0","file":"y.ogg","license":"CC-BY-4.0","sha256":"{digest}"}},
                {{"id":"up","name":"Escapes","file":"../z.ogg","license":"CC0","sha256":"{digest}"}}
            ]}}"#
        );
        let catalogue = Catalogue::parse(bytes.as_bytes()).expect("parses");
        assert_eq!(catalogue.sounds.len(), 1);
        let sound = catalogue.get("whoosh-1").unwrap();
        assert_eq!(sound.sha256, "a".repeat(64), "digests compare in lowercase");
        let dirs = AppDirs::under(Path::new("/cache"));
        assert_eq!(
            cached_file(&dirs, sound),
            Path::new("/cache/sounds/aaaaaaaaaaaa-w.ogg")
        );
        assert!(Catalogue::parse(b"not json").is_err());
    }

    /// A little catalogue to search, in the shape the manifest FUNCUT
    /// ships now carries: names, tags, a description and use-for lines.
    fn search_fixture() -> Catalogue {
        let digest = "a".repeat(64);
        let rows = [
            format!(
                r#"{{"id":"whoosh-fast","name":"Whoosh, fast","category":"Whoosh",
                "file":"a.ogg","duration":0.25,"license":"CC0-1.0","sha256":"{digest}",
                "tags":["fast","air"],
                "description":"A fast, crisp whip of air.",
                "useFor":["Whip pans","Fast cuts"]}}"#
            ),
            format!(
                r#"{{"id":"whoosh-slow","name":"Whoosh, slow","category":"Whoosh",
                "file":"b.ogg","duration":0.9,"license":"CC0-1.0","sha256":"{digest}",
                "tags":["slow","air"],
                "description":"A slow swell of air.",
                "useFor":["Reveals"]}}"#
            ),
            format!(
                r#"{{"id":"click-button","name":"Button click","category":"UI",
                "file":"c.ogg","duration":0.05,"license":"CC0-1.0","sha256":"{digest}",
                "tags":["interface","tap"],
                "description":"A small plastic button.",
                "useFor":["Menus","Buttons"]}}"#
            ),
            format!(
                r#"{{"id":"swipe-fast","name":"Swipe, fast","category":"Motion",
                "file":"d.ogg","duration":0.3,"license":"CC0-1.0","sha256":"{digest}",
                "tags":["fast","swipe"],
                "description":"A quick swipe of air for a card transition.",
                "useFor":["Fast cuts between scenes"]}}"#
            ),
        ];
        let body = format!(r#"{{"version":1,"sounds":[{}]}}"#, rows.join(","),);
        Catalogue::parse(body.as_bytes()).expect("the fixture parses")
    }

    /// The words are still ANDed and still prefix-matched, so the old
    /// queries find what they found; a word that matches nowhere leaves
    /// the sound out, and no query at all is the whole manifest order.
    #[test]
    fn words_are_anded_and_prefix_matched() {
        let catalogue = search_fixture();
        let ids = |query: &str| -> Vec<&str> {
            catalogue
                .search(query, None)
                .into_iter()
                .map(|sound| sound.id.as_str())
                .collect()
        };
        assert_eq!(ids("whoosh"), ["whoosh-fast", "whoosh-slow"]);
        assert_eq!(ids("fast whoosh"), ["whoosh-fast"]);
        // "whip" is neither name, id, tag nor category: only the fast
        // whoosh's description says it, and that is enough to find it.
        assert_eq!(ids("whip"), ["whoosh-fast"]);
        assert!(ids("explosion").is_empty());
        assert_eq!(
            ids(""),
            ["whoosh-fast", "whoosh-slow", "click-button", "swipe-fast"]
        );
        // "swe" is the front of "swell" in a description: a prefix finds
        // a word anywhere it is written.
        assert_eq!(ids("swe"), ["whoosh-slow"]);
        // "swi" is the front of both "Swipe"'s name and the word "swipe"
        // in its description, and of nothing else.
        assert_eq!(ids("swi"), ["swipe-fast"]);
        // The glue of a sentence is not a word of it: "a whoosh with a
        // fast edge" asks for a whoosh that is fast, and "the explosion"
        // still finds nothing.
        assert_eq!(ids("a whoosh"), ["whoosh-fast", "whoosh-slow"]);
        assert_eq!(ids("the whip"), ["whoosh-fast"]);
        assert!(ids("the explosion").is_empty());
    }

    /// Where a word matched says how high it ranks: all three of these
    /// sounds answer "air", but the two whose *tags* say it come before
    /// the one only a description mentions, and sounds that tie keep the
    /// manifest's order.
    #[test]
    fn where_a_word_landed_says_how_high_it_ranks() {
        let catalogue = search_fixture();
        let ids: Vec<&str> = catalogue
            .search("air", None)
            .into_iter()
            .map(|sound| sound.id.as_str())
            .collect();
        assert_eq!(ids, ["whoosh-fast", "whoosh-slow", "swipe-fast"]);
        let tags = catalogue.get("whoosh-fast").unwrap();
        let description = catalogue.get("swipe-fast").unwrap();
        assert!(
            tags.score(&["air".to_owned()]).unwrap()
                > description.score(&["air".to_owned()]).unwrap()
        );
        // A word of the name outranks a word of the id: "swipe" is both
        // the swipe's name and nowhere else in the fixture.
        assert_eq!(
            catalogue
                .get("swipe-fast")
                .unwrap()
                .score(&["swipe".to_owned()]),
            Some(16)
        );
    }

    /// The category narrows first and the words narrow what is left, and
    /// the case of either is the other's.
    #[test]
    fn the_category_narrows_before_the_words() {
        let catalogue = search_fixture();
        let ids: Vec<&str> = catalogue
            .search("click", Some("ui"))
            .into_iter()
            .map(|sound| sound.id.as_str())
            .collect();
        assert_eq!(ids, ["click-button"]);
        assert!(catalogue.search("whoosh", Some("UI")).is_empty());
    }

    /// The manifest's new fields round-trip - a description, use-for
    /// lines and a sync point survive a parse - and an old manifest
    /// without them still reads, its sounds searching on name, id,
    /// category and tags alone.
    #[test]
    fn the_new_fields_round_trip_and_old_manifests_still_read() {
        let with = search_fixture();
        assert_eq!(
            with.get("whoosh-fast").unwrap().description,
            "A fast, crisp whip of air."
        );
        assert_eq!(
            with.get("whoosh-fast").unwrap().use_for,
            ["Whip pans", "Fast cuts"]
        );
        let digest = "a".repeat(64);
        let old = format!(
            r#"{{"version":1,"sounds":[{{"id":"w","name":"Whoosh","category":"Whoosh",
                "file":"w.ogg","duration":1.0,"license":"CC0-1.0","sha256":"{digest}"}}]}}"#
        );
        let catalogue = Catalogue::parse(old.as_bytes()).expect("an old manifest reads");
        assert_eq!(catalogue.search("whoosh", None).len(), 1);
        assert_eq!(catalogue.sounds[0].description, "");
        assert!(catalogue.sounds[0].use_for.is_empty());
        assert_eq!(catalogue.sounds[0].sync_at, None);
    }

    /// A sync point is carried as seconds, and stays absent when the
    /// manifest does not say one.
    #[test]
    fn a_sync_point_is_carried_in_seconds() {
        let digest = "a".repeat(64);
        let body = format!(
            r#"{{"version":1,"sounds":[
                {{"id":"w","name":"Whoosh","category":"Whoosh","file":"w.ogg",
                  "duration":0.25,"license":"CC0-1.0","sha256":"{digest}","syncAt":0.092}}
            ]}}"#
        );
        let catalogue = Catalogue::parse(body.as_bytes()).expect("parses");
        assert_eq!(catalogue.sounds[0].sync_at, Some(0.092));
    }

    /// The manifest this repository ships, read where it is committed:
    /// it parses whole, every row offered, and the searches the shelf
    /// and an agent both ask find what they should, ranked. Skipped
    /// quietly outside a checkout - a crate vendored alone carries no
    /// library to read.
    #[test]
    fn the_shipped_manifest_parses_and_searches() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../audio/manifest.json");
        let Ok(bytes) = std::fs::read(&manifest) else {
            return;
        };
        let catalogue = Catalogue::parse(&bytes).expect("the shipped manifest parses");
        assert!(
            catalogue.sounds.len() >= 430,
            "the whole library is offered, {} rows",
            catalogue.sounds.len()
        );
        assert_eq!(catalogue.search("", None).len(), catalogue.sounds.len());
        let top = |query: &str| {
            catalogue
                .search(query, None)
                .into_iter()
                .take(3)
                .map(|sound| sound.id.as_str())
                .collect::<Vec<_>>()
        };
        // Every word of a natural ask, glue dropped, finds the one sound
        // that says them all.
        assert_eq!(
            top("soft swish for a card transition"),
            ["whoosh-medium-soft"]
        );
        assert_eq!(
            top("fast whoosh"),
            ["whoosh-fast-hard", "whoosh-fast-medium", "whoosh-fast-pass"]
        );
        assert_eq!(
            top("success notification")[0],
            "notification-success-bubble"
        );
        assert_eq!(top("pen writing")[0], "writing-pen-write");
        assert_eq!(top("glitch")[0], "digital-glitch-clear");
        assert_eq!(top("data chart")[0], "data-chart-reveal");
        assert_eq!(top("sub drop")[0], "bass-sub-drop-long");
        // and the words a description says are found by their front
        assert_eq!(top("sparkle reveal")[0], "shimmer-long-sparkle");
    }

    /// An index that names a file outside the folder is not believed.
    #[test]
    fn an_index_cannot_point_out_of_the_folder() {
        let dir = scratch("escape");
        std::fs::write(
            dir.join("library.json"),
            br#"{"sounds":[{"id":"x","name":"x","file":"../x.wav","duration":1}]}"#,
        )
        .unwrap();
        assert!(Library::at(dir.clone()).sounds.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
