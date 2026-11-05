// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Project folders on disk, and the list of recently opened ones.
//!
//! A project is a directory containing `concat.json`. That file is written the
//! moment the project is created, so a project is a real thing from the start
//! rather than a promise the app keeps only if you remember to save.
//!
//! The recents list lives in the app's config directory, not in any project.
//! It describes this machine's history, and copying a project folder to
//! another machine should not drag one person's recent files along with it.

use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MANIFEST: &str = "concat.json";
/// The manifest's earlier names, newest first. Projects created before a
/// rename keep theirs forever: the manifest is read and written under the
/// name it has, and only brand-new projects get the current one. A rename
/// must never orphan an edit.
const LEGACY_MANIFESTS: &[&str] = &["wolfcut.json"];

/// The manifest file this project actually uses: the current name, a legacy
/// name if that is what exists, the current name for a fresh folder.
pub fn manifest_path(root: &Path) -> PathBuf {
    let current = root.join(MANIFEST);
    if current.is_file() {
        return current;
    }
    for legacy in LEGACY_MANIFESTS {
        let candidate = root.join(legacy);
        if candidate.is_file() {
            return candidate;
        }
    }
    current
}

/// Whether `root` holds a project: any manifest, current or legacy.
pub fn is_project(root: &Path) -> bool {
    manifest_path(root).is_file()
}

const RECENTS: &str = "recents.json";
/// Long enough to be a working set of projects, short enough that the
/// launch screen's grid stays scannable - and past a page of it, the screen
/// has a search and a sort to find one by.
const MAX_RECENTS: usize = 40;

/// Everything needed to reopen a project.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    /// The project folder.
    pub path: String,
    /// The name shown in the title bar and the recents list.
    pub name: String,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Frame rate as an exact fraction; the decimal is never stored.
    pub rate_num: i64,
    /// Denominator of the frame rate.
    pub rate_den: i64,
    /// Milliseconds since the epoch.
    pub opened_at: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    /// The version of the app that wrote the file, under whichever name the
    /// app had at the time. Informational: nothing reads it back.
    #[serde(alias = "wolfcut")]
    concat: String,
    name: String,
    video: Video,
    /// Everything else in the file is ignored when reading settings for the
    /// recents list. `flatten` keeps it from being dropped on a rewrite.
    #[serde(flatten)]
    #[allow(dead_code)]
    rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Video {
    width: u32,
    height: u32,
    rate_num: i64,
    rate_den: i64,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Creates the project folder and writes its manifest.
///
/// Refuses to touch a folder that already holds a manifest. Overwriting
/// someone's edit because they reused a name is not a recoverable mistake.
pub fn create(
    location: &str,
    name: &str,
    width: u32,
    height: u32,
    rate_num: i64,
    rate_den: i64,
) -> Result<ProjectInfo, String> {
    let root = Path::new(location).join(folder_name(name));
    let manifest = root.join(MANIFEST);

    // Any name counts as an existing project - a legacy folder must be
    // just as safe from being clobbered as a new one.
    if is_project(&root) {
        return Err(format!(
            "a Concat project already exists at {}",
            root.display()
        ));
    }

    std::fs::create_dir_all(&root)
        .map_err(|error| format!("could not create {}: {error}", root.display()))?;

    // Settings only: a fresh project has no edit yet. The full document -
    // timelines included - is written by the session's save from the first
    // change.
    let document = Manifest {
        concat: env!("CARGO_PKG_VERSION").to_owned(),
        name: name.to_owned(),
        video: Video {
            width,
            height,
            rate_num,
            rate_den,
        },
        rest: serde_json::Map::new(),
    };

    let encoded = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("could not encode the manifest: {error}"))?;
    std::fs::write(&manifest, encoded)
        .map_err(|error| format!("could not write {}: {error}", manifest.display()))?;

    Ok(ProjectInfo {
        path: root.to_string_lossy().into_owned(),
        name: name.to_owned(),
        width,
        height,
        rate_num,
        rate_den,
        opened_at: now_millis(),
    })
}

/// A snapshot's place in this process's write order, and in every
/// process's.
///
/// Minted by [`ticket`] where a document is captured - on the thread that
/// captures it, before the write is handed to a worker - and carried to
/// [`save_ordered`], where the write happens. Two snapshots of one project
/// are written in ticket order, so a worker that finishes late cannot put an
/// older document back on disk after a newer one: the sequence counts this
/// process's order, and the capture time - the clock every process on the
/// machine shares - orders what this process cannot see, another process
/// writing the same folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ticket {
    /// This process's counter: the order of two snapshots it minted.
    seq: u64,
    /// When the snapshot was captured, in nanoseconds since the epoch: the
    /// order of two snapshots from different processes, where neither
    /// knows the other's counter.
    at: u64,
}

/// Mints the next ticket; see [`Ticket`].
///
/// The counter is process-wide, so tickets are monotonic for every project,
/// and therefore for each project on its own. The capture time orders what
/// the counter cannot: snapshots another process minted over the same
/// folder, which [`save_ordered`] holds against the folder's own record.
pub fn ticket() -> Ticket {
    Ticket {
        seq: NEXT_TICKET.fetch_add(1, Ordering::Relaxed),
        at: captured(),
    }
}

/// When now is, in nanoseconds since the epoch: the clock every process on
/// the machine shares. `0` where the machine says it is before the epoch,
/// which no ticket minted after it can be superseded by.
fn captured() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos() as u64)
        .unwrap_or(0)
}

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

/// What an ordered save did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    /// The document is on disk.
    Written,
    /// A snapshot minted after this one was already written, so this older
    /// document was left alone rather than put back on disk. Nothing went
    /// wrong: the project is saved, to the newer document.
    Superseded,
}

/// One project's writes: the lock that runs them one at a time, and the
/// highest ticket already on disk.
struct Writes {
    /// Held for a whole write - the temporary file, its flush and the
    /// rename - so two workers never write one manifest at once.
    writing: Mutex<()>,
    /// The highest ticket whose document reached the manifest. Read and
    /// written under `writing`, so the check that skips an older snapshot
    /// and the write that follows it cannot be split by another writer.
    written: AtomicU64,
}

/// The per-project write state, keyed by the folder as the filesystem
/// spells it; see [`project_key`].
fn writes() -> &'static Mutex<HashMap<PathBuf, Arc<Writes>>> {
    static WRITES: OnceLock<Mutex<HashMap<PathBuf, Arc<Writes>>>> = OnceLock::new();
    WRITES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One key per project folder: the real path where the filesystem will say
/// it, so `/var` and `/private/var`, a trailing slash, a symlinked parent
/// and a different case on a case-insensitive volume all name one project.
/// The path as it was given when it cannot be resolved, which still names
/// the folder it was given.
fn project_key(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// The write state for one project folder.
fn write_state(key: &Path) -> Arc<Writes> {
    let mut all = writes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(all.entry(key.to_path_buf()).or_insert_with(|| {
        Arc::new(Writes {
            writing: Mutex::new(()),
            written: AtomicU64::new(0),
        })
    }))
}

/// Writes the whole project document to the project's manifest, now.
///
/// The document is passed through as opaque JSON rather than being mirrored
/// into Rust types: the engine (`concat-project`) owns the canonical model and
/// produced this document, and the host has no decisions to make about its
/// contents beyond writing it safely.
///
/// The write is ordered like [`save_ordered`], with its ticket minted here: a
/// caller that captured the document and writes it at once - the API, a
/// template - is in call order already. A caller that captures a document on
/// one thread and writes it on another (the window's autosave) mints the
/// ticket where it captures the document and calls [`save_ordered`] instead.
pub fn save(path: &str, document: &serde_json::Value) -> Result<(), String> {
    save_ordered(path, document, ticket()).map(|_| ())
}

/// Writes a document captured at `ticket`, in this process's write order
/// and against every other process's.
///
/// A snapshot older than the one already on disk is skipped whole - see
/// [`Saved`] - so a worker that finishes late cannot put an older document
/// back over a newer one, whatever order the two are scheduled in: this
/// process's snapshots by their sequence, and another process's by the
/// capture time both shared, checked against the record the folder keeps
/// under [`SAVE_LOCK`] (audit 2026-11-04, #16). A write that fails is
/// always a failure: a document that should have been written and could not
/// returns the error, never [`Saved::Superseded`].
///
/// The temporary file is this call's own (`create_new`), so two saves -
/// including ones in different processes on the same folder - never write one
/// another's bytes, and the rename over the manifest is atomic on a local
/// disk (not always on FAT or a network share, where it is still one call
/// that replaces or fails). If the folder's lock cannot be opened, acquired,
/// or updated, the save fails closed: it never silently writes without the
/// cross-process conflict check. The order record is advanced before the
/// manifest write, so a failed replacement conservatively bars older
/// pending snapshots rather than letting one overwrite newer intent.
pub fn save_ordered(
    path: &str,
    document: &serde_json::Value,
    ticket: Ticket,
) -> Result<Saved, String> {
    let root = PathBuf::from(path);
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("could not create {}: {error}", root.display()))?;

    let state = write_state(&project_key(&root));
    // Held across the write: the check below and the write it decides are
    // one step, so two saves cannot both decide to write and have the older
    // one land second.
    let _writing = state
        .writing
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if ticket.seq < state.written.load(Ordering::Acquire) {
        return Ok(Saved::Superseded);
    }

    // The folder's lock is mandatory: the check and manifest write are one
    // cross-process transaction. A peer that holds it past the bounded wait
    // makes this save fail rather than fall back to process-local ordering.
    let mut order = SaveOrder::take(&root)?;
    if order
        .recorded()?
        .is_some_and(|recorded| ticket.at < recorded)
    {
        return Ok(Saved::Superseded);
    }

    let encoded = serde_json::to_vec_pretty(document)
        .map_err(|error| format!("could not encode the project: {error}"))?;
    // Advance the high-water mark before replacing the manifest. If the
    // replacement fails, older queued snapshots remain barred from
    // overwriting this newer intent; the caller receives the write error.
    order.record(ticket.at)?;
    write_manifest(&root, &encoded)?;
    state.written.store(ticket.seq, Ordering::Release);
    Ok(Saved::Written)
}

/// The file the folder's saves are ordered by: `concat.save.lock` beside
/// the manifest, locked for the check-and-write of each save and carrying,
/// as its whole contents, the capture time of the newest snapshot any
/// process has put on disk. The lock is the operating system's, so a
/// process that crashes lets go of it and the next saver takes over the
/// file it left.
const SAVE_LOCK: &str = "concat.save.lock";

/// How long a save waits for another process's save to let go of the
/// folder's lock: long enough for a save to finish - the temp file, its
/// flush and the rename are milliseconds - and never long enough to hang
/// an autosave on a peer that is stuck.
const SAVE_LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(200);

/// One save's hold on the folder's [`SAVE_LOCK`]. A save without this
/// operating-system lock is not allowed to replace the manifest.
struct SaveOrder {
    file: std::fs::File,
}

impl SaveOrder {
    /// Takes the folder's save lock. Waiting is bounded so a stuck peer
    /// cannot hang autosave indefinitely, but timeout or unsupported locking
    /// is an error: writing without the lock would reopen the cross-process
    /// lost-update race this guard exists to prevent.
    fn take(root: &Path) -> Result<SaveOrder, String> {
        use std::io::ErrorKind;

        let path = root.join(SAVE_LOCK);
        let started = std::time::Instant::now();
        loop {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|error| {
                    format!("could not open project save lock {}: {error}", path.display())
                })?;
            match file.try_lock() {
                Ok(()) => return Ok(SaveOrder { file }),
                Err(error)
                    if error.kind() == ErrorKind::WouldBlock
                        && started.elapsed() < SAVE_LOCK_WAIT =>
                {
                    drop(file);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    return Err(format!(
                        "timed out waiting for project save lock {}; the project was not saved",
                        path.display()
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "could not acquire project save lock {}: {error}",
                        path.display()
                    ));
                }
            }
        }
    }

    /// The capture time of the newest snapshot on disk. An empty lock file
    /// means no ordered save has reached the manifest yet; malformed or
    /// unreadable contents are errors, never an excuse to skip conflict
    /// detection.
    fn recorded(&mut self) -> Result<Option<u64>, String> {
        let mut text = String::new();
        self.file
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|error| format!("could not read project save order: {error}"))?;
        self.file
            .read_to_string(&mut text)
            .map_err(|error| format!("could not read project save order: {error}"))?;
        let text = text.trim();
        if text.is_empty() {
            Ok(None)
        } else {
            text.parse::<u64>()
                .map(Some)
                .map_err(|error| format!("invalid project save order record: {error}"))
        }
    }

    /// Records `at` as the capture time of the newest snapshot on disk.
    /// Only ever moves forward: a clock stepped back between two captures
    /// must not make the folder forget a newer save it already holds.
    fn record(&mut self, at: u64) -> Result<(), String> {
        let stamped = at.max(self.recorded()?.unwrap_or(0));
        self.file
            .set_len(0)
            .map_err(|error| format!("could not reset project save order: {error}"))?;
        self.file
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|error| format!("could not update project save order: {error}"))?;
        self.file
            .write_all(stamped.to_string().as_bytes())
            .map_err(|error| format!("could not update project save order: {error}"))?;
        self.file
            .sync_all()
            .map_err(|error| format!("could not flush project save order: {error}"))?;
        Ok(())
    }
}

/// The temporary file a save writes: flushed and renamed into place on the
/// way through, removed on every other way out, so a failure leaves no
/// `.saving` file behind. The name is this call's own - process id and a
/// counter - so no two saves share it.
struct Temporary {
    path: PathBuf,
    file: Option<std::fs::File>,
    renamed: bool,
}

impl Temporary {
    /// Creates a fresh temporary beside the manifest. `create_new` never
    /// reuses or follows a name already there, so it cannot write through a
    /// symlink someone planted and fails rather than sharing a temporary
    /// with another save; a leftover from a crashed run makes it try another
    /// name.
    fn open(root: &Path) -> Result<Temporary, String> {
        let mut attempts = 0;
        loop {
            let nonce = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("{MANIFEST}.saving.{}.{nonce}", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Temporary {
                        path,
                        file: Some(file),
                        renamed: false,
                    });
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists && attempts < 16 =>
                {
                    attempts += 1;
                }
                Err(error) => return Err(format!("could not write {}: {error}", path.display())),
            }
        }
    }

    /// Writes `encoded` and flushes its contents before the atomic rename.
    /// Atomic replacement and durability are distinct: flushing the file
    /// reduces power-loss risk, but does not also sync the parent directory.
    fn write(&mut self, encoded: &[u8]) -> Result<(), String> {
        let file = self.file.as_mut().expect("a fresh temporary");
        std::io::Write::write_all(file, encoded)
            .map_err(|error| format!("could not write {}: {error}", self.path.display()))?;
        file.sync_all()
            .map_err(|error| format!("could not flush {}: {error}", self.path.display()))
    }

    /// Closes the file and renames it over the manifest. Every path out but
    /// this one removes the temporary when the value drops.
    fn commit(&mut self, manifest: &Path) -> Result<(), String> {
        // Closed first, so the name is free for Windows to replace.
        self.file.take();
        // A scanner, a sync client or a backup tool holding the manifest a
        // moment makes Windows refuse the replace (access denied, sharing
        // violation). That passes; a close that failed on it would not.
        let mut wait = std::time::Duration::from_millis(50);
        let mut tries = 0;
        loop {
            match std::fs::rename(&self.path, manifest) {
                Ok(()) => break,
                Err(error) if tries < 5 && transient(&error) => {
                    tries += 1;
                    std::thread::sleep(wait);
                    wait *= 2;
                }
                Err(error) => {
                    return Err(format!("could not replace {}: {error}", manifest.display()));
                }
            }
        }
        self.renamed = true;
        Ok(())
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        // The handle goes first: Windows will not take a file back while it
        // is open.
        self.file.take();
        if !self.renamed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Whether a failed replace is another program holding the file for a
/// moment. A rename is not refused that way on Linux, so nothing is
/// retried.
fn transient(_error: &std::io::Error) -> bool {
    false
}

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Writes `encoded` to the project's manifest: a temporary of its own,
/// flushed to the disk, then renamed over the manifest, because a save
/// interrupted halfway is worse than no save at all - a truncated manifest
/// loses the project, while a failed rename leaves the previous one intact.
fn write_manifest(root: &Path, encoded: &[u8]) -> Result<(), String> {
    let manifest = manifest_path(root);
    let mut temporary = Temporary::open(root)?;
    temporary.write(encoded)?;
    temporary.commit(&manifest)
}

/// True when a document carries no edit state at all - the settings-only
/// manifest `create` writes, in a project closed before its first save.
/// Such a document is a legitimately empty project. A document that claims
/// edit state the build then cannot load is a different thing: corrupt.
pub fn is_settings_only(document: &serde_json::Value) -> bool {
    document.is_object()
        && ["timelines", "tracks", "clips", "media"]
            .iter()
            .all(|key| document.get(key).is_none())
}

/// Reads the whole project document back: `None` when the folder has no
/// document yet, an error for one that is there and cannot be read or
/// parsed. The two are kept apart because a caller that treats them alike
/// opens a half-written document as an empty project and saves over it
/// (audit 2026-09-23, #1).
pub fn read_document(path: &str) -> Result<Option<serde_json::Value>, String> {
    let manifest = manifest_path(Path::new(path));
    let bytes = match std::fs::read(&manifest) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", manifest.display())),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("{} is not a Concat project: {error}", manifest.display()))
}

/// Reads an existing project's settings.
pub fn open(path: &str) -> Result<ProjectInfo, String> {
    let root = PathBuf::from(path);
    let manifest = manifest_path(&root);

    let bytes = std::fs::read(&manifest)
        .map_err(|error| format!("could not read {}: {error}", manifest.display()))?;
    let document: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{} is not a Concat project: {error}", manifest.display()))?;

    if document.video.rate_den == 0 {
        return Err(format!("{} has an invalid frame rate", manifest.display()));
    }

    Ok(ProjectInfo {
        path: root.to_string_lossy().into_owned(),
        name: document.name,
        width: document.video.width,
        height: document.video.height,
        rate_num: document.video.rate_num,
        rate_den: document.video.rate_den,
        opened_at: now_millis(),
    })
}

/// Moves a project to the front of the recents list.
pub fn remember(config: &Path, project: &ProjectInfo) -> Result<(), String> {
    let mut entries = read_recents(config);
    entries.retain(|entry| !same_path(&entry.path, &project.path));
    entries.insert(0, project.clone());
    entries.truncate(MAX_RECENTS);
    write_recents(config, &entries)
}

/// The recents list, most recent first, with vanished folders dropped.
///
/// Filtering on read rather than pruning on write means a project on a drive
/// that happens to be unplugged comes back when the drive does.
pub fn list(config: &Path) -> Vec<ProjectInfo> {
    read_recents(config)
        .into_iter()
        .filter(|entry| is_project(Path::new(&entry.path)))
        .collect()
}

/// One row of the recents list as the launch screen shows it.
#[derive(Clone, Debug)]
pub struct Recent {
    /// What the list remembers of it.
    pub info: ProjectInfo,
    /// The folder is there and still holds a project. One on a drive that
    /// is unplugged is not, and the launch screen shows it as missing
    /// rather than dropping it, so the person sees where it went and can
    /// forget it - and it comes back by itself when the drive does.
    pub present: bool,
    /// When the project was last written, in milliseconds since the epoch:
    /// the manifest's own time, which every autosave moves. None for a
    /// missing folder, or a file system that keeps no times.
    pub edited_at: Option<u64>,
}

/// The recents list, most recent first, missing folders included and
/// marked. One stat per project, so it is read when the list changes and
/// not on every frame.
pub fn recents(config: &Path) -> Vec<Recent> {
    read_recents(config)
        .into_iter()
        .map(|info| {
            let manifest = manifest_path(Path::new(&info.path));
            let meta = std::fs::metadata(&manifest)
                .ok()
                .filter(|meta| meta.is_file());
            let edited_at = meta
                .as_ref()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_millis() as u64);
            Recent {
                present: meta.is_some(),
                edited_at,
                info,
            }
        })
        .collect()
}

/// Removes one project from the list. The folder itself is left alone.
pub fn forget(config: &Path, path: &str) -> Result<(), String> {
    let mut entries = read_recents(config);
    entries.retain(|entry| !same_path(&entry.path, path));
    write_recents(config, &entries)
}

fn read_recents(config: &Path) -> Vec<ProjectInfo> {
    // A missing or corrupt list is not an error worth showing anyone; it just
    // means there is no history yet.
    std::fs::read(config.join(RECENTS))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_recents(config: &Path, entries: &[ProjectInfo]) -> Result<(), String> {
    std::fs::create_dir_all(config)
        .map_err(|error| format!("could not create {}: {error}", config.display()))?;

    let encoded = serde_json::to_vec_pretty(entries)
        .map_err(|error| format!("could not encode the recents list: {error}"))?;
    std::fs::write(config.join(RECENTS), encoded)
        .map_err(|error| format!("could not write the recents list: {error}"))
}

/// Compares paths, with a trailing separator naming the same folder.
fn same_path(left: &str, right: &str) -> bool {
    fn spelled(path: &str) -> String {
        path.trim_end_matches('/').to_owned()
    }
    spelled(left) == spelled(right)
}

/// Turns a project name into something a filesystem will accept. Also used
/// for template bundle folders and the files inside them.
pub fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_control() || r#"<>:"/\|?*"#.contains(character) {
                '-'
            } else {
                character
            }
        })
        .collect();

    // Windows refuses names ending in a dot or a space.
    let trimmed = cleaned.trim_matches(['.', ' ']);
    if trimmed.is_empty() {
        "Untitled project".to_owned()
    } else if is_device_name(trimmed) {
        // A folder called `NUL` is made, but nothing can be written in it.
        format!("_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

/// Whether Windows takes `name` for a device - `CON`, `NUL`, `COM1` and
/// the rest - rather than a file. Only the part before the first dot
/// counts, so `nul.mp4` is the device too.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let stem = stem.to_ascii_uppercase();
    let port = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|number| {
            matches!(number.as_bytes(), [b'0'..=b'9']) || matches!(number, "¹" | "²" | "³")
        })
    };
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || port("COM") || port("LPT")
}

/// Removes all cached artwork and waveforms from a project folder.
///
/// The cache is entirely regenerable, so the whole folder goes, subfolders
/// (`art/`, peaks and all) included. `remove_file` alone would skip them:
/// it refuses directories.
pub fn clear_cache(path: &str) -> Result<usize, String> {
    let cache_dir = Path::new(path).join("cache");
    if !cache_dir.exists() {
        return Ok(0);
    }
    let count = count_files(&cache_dir);
    std::fs::remove_dir_all(&cache_dir)
        .map_err(|error| format!("cannot remove {}: {error}", cache_dir.display()))?;
    Ok(count)
}

/// Recursively counts the files under a directory, for the clear-cache toast.
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() { count_files(&path) } else { 1 }
        })
        .sum()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitises_names_windows_would_reject() {
        assert_eq!(folder_name("My: Project?"), "My- Project-");
        assert_eq!(folder_name("  trailing.  "), "trailing");
        assert_eq!(folder_name("..."), "Untitled project");
        assert_eq!(folder_name(""), "Untitled project");
    }

    #[test]
    fn device_names_get_a_folder_windows_will_open() {
        for name in [
            "NUL", "con", "Aux", "prn", "COM1", "lpt9", "nul.mp4", "CON .txt",
        ] {
            assert_eq!(folder_name(name), format!("_{name}"));
        }
        // Only the device names themselves, not names that start like them.
        assert_eq!(folder_name("Console"), "Console");
        assert_eq!(folder_name("COM10"), "COM10");
        assert_eq!(folder_name("nul-and-void"), "nul-and-void");
    }

    #[test]
    fn a_project_named_nul_is_created_and_reads_back() {
        let location = scratch("device-name");
        let info = create(&location, "nul", 1920, 1080, 30, 1).expect("creates");
        assert!(is_project(Path::new(&info.path)));
        assert_eq!(info.name, "nul");
    }

    #[test]
    fn a_legacy_manifest_keeps_its_name() {
        for legacy in LEGACY_MANIFESTS {
            let scratch = std::env::temp_dir().join(format!("concat-legacy-{legacy}-test"));
            let _ = std::fs::remove_dir_all(&scratch);
            std::fs::create_dir_all(&scratch).expect("scratch dir");
            // A fresh folder resolves to the current name...
            assert!(manifest_path(&scratch).ends_with(MANIFEST));
            assert!(!is_project(&scratch));
            // ...a pre-rename project keeps the manifest it has, forever...
            std::fs::write(scratch.join(legacy), b"{}").expect("writes");
            assert!(manifest_path(&scratch).ends_with(legacy));
            assert!(is_project(&scratch));
            // ...and the current name wins only where it actually exists.
            std::fs::write(scratch.join(MANIFEST), b"{}").expect("writes");
            assert!(manifest_path(&scratch).ends_with(MANIFEST));
            let _ = std::fs::remove_dir_all(&scratch);
        }
    }

    #[test]
    fn a_manifest_under_any_of_its_names_opens() {
        let scratch = std::env::temp_dir().join("concat-manifest-key-test");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        for key in ["concat", "wolfcut"] {
            let document = serde_json::json!({
                key: "0.2.0",
                "name": "Old",
                "video": { "width": 1920, "height": 1080, "rateNum": 30, "rateDen": 1 },
            });
            std::fs::write(
                scratch.join(MANIFEST),
                serde_json::to_vec(&document).unwrap(),
            )
            .expect("writes");
            let info = open(&scratch.to_string_lossy()).expect("opens");
            assert_eq!((info.width, info.height, info.rate_num), (1920, 1080, 30));
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_fresh_manifest_is_settings_only_and_a_saved_edit_is_not() {
        // Exactly what `create` writes: settings, no edit state.
        let fresh = serde_json::json!({
            "concat": "0.2.0",
            "name": "Untitled project",
            "video": { "width": 1080, "height": 1920, "rateNum": 30, "rateDen": 1 },
        });
        assert!(is_settings_only(&fresh));
        // Any edit state present means the document must load or fail loudly.
        assert!(!is_settings_only(&serde_json::json!({ "tracks": [] })));
        assert!(!is_settings_only(
            &serde_json::json!({ "timelines": "garbage" })
        ));
        assert!(!is_settings_only(&serde_json::json!("nonsense")));
    }

    #[test]
    fn paths_compare_the_same_folder() {
        assert!(same_path("/work/Film", "/work/Film"));
        assert!(!same_path("/work/Film", "/work/Other"));
        assert!(!same_path("/work/film", "/work/Film"));
        assert!(same_path("/work/film/", "/work/film"));
    }

    /// A project folder of this test's own, swept if it is already there.
    fn scratch(name: &str) -> String {
        let root =
            std::env::temp_dir().join(format!("concat-projects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch dir");
        root.to_string_lossy().into_owned()
    }

    /// A whole document, as a save writes one.
    fn document(marker: &str) -> serde_json::Value {
        serde_json::json!({ "marker": marker, "tracks": [] })
    }

    /// How many temporary files a folder holds: none, after any save.
    fn temporaries(root: &str) -> usize {
        std::fs::read_dir(root)
            .expect("reads the scratch")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{MANIFEST}.saving"))
            })
            .count()
    }

    #[test]
    fn a_late_snapshot_never_lands_after_a_newer_one() {
        let root = scratch("late-snapshot");
        let older = document("older");
        let newer = document("newer");
        // Minted where each document is captured: the older one first.
        let older_ticket = ticket();
        let newer_ticket = ticket();
        // The newer worker finishes first, the older one arrives after it -
        // the order two workers on the pool can take.
        assert_eq!(
            save_ordered(&root, &newer, newer_ticket).expect("writes"),
            Saved::Written
        );
        assert_eq!(
            save_ordered(&root, &older, older_ticket).expect("skips, it does not fail"),
            Saved::Superseded
        );
        assert_eq!(
            read_document(&root).expect("reads"),
            Some(newer),
            "the newer document stands"
        );
        assert_eq!(temporaries(&root), 0, "no temporary is left behind");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn concurrent_saves_keep_the_document_readable() {
        let root = scratch("concurrent");
        const SAVES: usize = 8;
        let documents: Vec<serde_json::Value> = (0..SAVES)
            .map(|index| document(&format!("doc-{index}")))
            .collect();
        let tickets: Vec<Ticket> = (0..SAVES).map(|_| ticket()).collect();
        std::thread::scope(|scope| {
            for (document, ticket) in documents.iter().zip(&tickets) {
                let root = root.clone();
                scope.spawn(move || {
                    save_ordered(&root, document, *ticket).expect("a save that cannot fail here");
                });
            }
        });
        assert_eq!(
            read_document(&root).expect("reads"),
            Some(documents[SAVES - 1].clone()),
            "the last ticket's document stands, whatever order the workers took"
        );
        assert_eq!(temporaries(&root), 0, "no temporary is left behind");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_older_ticket_is_skipped_and_a_real_failure_is_not() {
        let root = scratch("skip-or-fail");
        let first = ticket();
        let second = ticket();
        save_ordered(&root, &document("one"), first).expect("writes");
        save_ordered(&root, &document("two"), second).expect("writes");
        // An older ticket is a skip, told apart from a failure.
        assert_eq!(
            save_ordered(&root, &document("stale"), first).expect("not an error"),
            Saved::Superseded
        );
        assert_eq!(read_document(&root).expect("reads"), Some(document("two")));

        // A write that truly cannot happen is an error, never a skip.
        let blocked = scratch("skip-or-fail-blocked");
        let manifest = Path::new(&blocked).join(MANIFEST);
        std::fs::create_dir(&manifest).expect("the manifest spot");
        std::fs::write(manifest.join("kept"), b"kept").expect("something worth keeping");
        let error = save_ordered(&blocked, &document("three"), ticket())
            .expect_err("cannot replace a directory");
        assert!(!error.is_empty());
        assert!(
            manifest.join("kept").exists(),
            "what was there is untouched"
        );
        assert_eq!(
            temporaries(&blocked),
            0,
            "the temporary is cleaned up on failure"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&blocked);
    }

    #[test]
    fn a_failed_save_keeps_the_document_it_had() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("keeps-document");
        let saved = document("saved");
        save(&root, &saved).expect("first save");
        let manifest = Path::new(&root).join(MANIFEST);
        let before = std::fs::read(&manifest).expect("reads");

        // A folder that refuses new files: the temporary cannot be created.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555))
            .expect("read-only folder");
        let probe = Path::new(&root).join("probe");
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .is_ok()
        {
            // A privileged account ignores the permission; nothing to test.
            let _ = std::fs::remove_file(&probe);
            let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_dir_all(&root);
            return;
        }

        let error = save(&root, &document("next")).expect_err("the folder refuses a temporary");
        assert!(!error.is_empty());
        assert_eq!(
            std::fs::read(&manifest).expect("reads"),
            before,
            "the document that was saved stands"
        );
        assert_eq!(temporaries(&root), 0, "no temporary is left behind");

        let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A snapshot captured before another process's save - the case the
    /// in-process counter cannot see, a window and a `concat --mcp` on one
    /// folder - is not put back over the newer document: the folder's lock
    /// file records what landed, and a save older than that is superseded
    /// exactly as one this process minted late is (audit 2026-11-04, #16).
    /// What stands in for the other process here is the record itself,
    /// written as its save would have written it.
    #[test]
    fn an_older_snapshot_from_another_process_is_not_put_back() {
        let root = scratch("cross-process-order");
        let ours = ticket();
        let written = document("written by the other process");
        {
            let mut held = SaveOrder::take(Path::new(&root)).expect("takes the folder lock");
            held.record(u64::MAX).expect("records the other process's save");
        }
        assert_eq!(
            save_ordered(&root, &document("stale"), ours).expect("a skip, not a failure"),
            Saved::Superseded,
            "a snapshot captured before what the folder holds does not land"
        );
        // A capture after the record writes, and the record follows it.
        let after = Ticket {
            seq: ours.seq + 1,
            at: u64::MAX,
        };
        assert_eq!(
            save_ordered(&root, &written, after).expect("writes"),
            Saved::Written
        );
        assert_eq!(
            read_document(&root).expect("reads"),
            Some(written),
            "the newer document stands"
        );
        let mut order = SaveOrder::take(Path::new(&root)).expect("takes the folder lock");
        assert_eq!(order.recorded().expect("reads the record"), Some(u64::MAX));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A peer that never releases the folder lock must not make this
    /// process fall back to process-local ordering and overwrite the
    /// saved manifest. Timeout is a failed save, with no temp left behind.
    #[test]
    fn a_save_fails_closed_when_another_writer_keeps_the_lock() {
        let root = scratch("save-lock-timeout");
        let saved = document("already saved");
        save(&root, &saved).expect("first save");
        let before = std::fs::read(Path::new(&root).join(MANIFEST)).expect("reads manifest");

        let path = Path::new(&root).join(SAVE_LOCK);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .expect("the other process's lock file");
        held.try_lock().expect("held as another process would");

        let error = save(&root, &document("must not overwrite"))
            .expect_err("a save without the cross-process lock must fail");
        assert!(
            error.contains("timed out waiting for project save lock"),
            "unexpected error: {error}"
        );
        assert_eq!(
            std::fs::read(Path::new(&root).join(MANIFEST)).expect("reads manifest"),
            before,
            "the already saved project is untouched"
        );
        assert_eq!(temporaries(&root), 0, "no temporary is left behind");

        drop(held);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A damaged order record is not treated as an empty one: doing so
    /// would let an older process overwrite a manifest whose ordering can
    /// no longer be established.
    #[test]
    fn a_malformed_save_order_record_fails_closed() {
        let root = scratch("save-order-malformed");
        let saved = document("already saved");
        save(&root, &saved).expect("first save");
        let before = std::fs::read(Path::new(&root).join(MANIFEST)).expect("reads manifest");
        std::fs::write(Path::new(&root).join(SAVE_LOCK), b"not-a-ticket")
            .expect("corrupts the order record");

        let error = save(&root, &document("must not overwrite"))
            .expect_err("a malformed order record must fail");
        assert!(error.contains("invalid project save order record"), "{error}");
        assert_eq!(
            std::fs::read(Path::new(&root).join(MANIFEST)).expect("reads manifest"),
            before,
            "the already saved project is untouched"
        );
        assert_eq!(temporaries(&root), 0, "no temporary is left behind");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A save lock held by another writer is waited for, briefly, and a
    /// save that gets it after the wait goes through: what a window's
    /// autosave does behind an API save finishing on the same folder. The
    /// lock held here stands for the other process's, the same file and
    /// the same operating-system lock either way.
    #[test]
    fn a_save_waits_briefly_for_the_folder_and_then_writes() {
        let root = scratch("save-waits");
        let path = Path::new(&root).join(SAVE_LOCK);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .expect("the other process's lock file");
        held.try_lock().expect("taken as another process would");
        let writer = {
            let root = root.clone();
            std::thread::spawn(move || save(&root, &document("after the wait")).expect("writes"))
        };
        // Long enough for the save to be waiting, short enough that it
        // gets the lock inside its budget.
        std::thread::sleep(std::time::Duration::from_millis(60));
        drop(held);
        writer.join().expect("no panic");
        assert_eq!(
            read_document(&root).expect("reads"),
            Some(document("after the wait")),
            "the save went through once the folder was free"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
