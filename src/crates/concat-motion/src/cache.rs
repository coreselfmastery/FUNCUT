// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Cache correctness and lifecycle: hashed right, bounded, and published
//! in one piece.
//!
//! A cache that serves a stale frame is worse than no cache - it lies with
//! a straight face - and a cache that grows until the disk is full is a
//! time bomb. This module is the three rules that keep both from happening,
//! in the order they went wrong in the field:
//!
//! 1. **Hash the content, not the name, and hash it in chunks.** A file is
//!    hashed by reading it in 1 MiB pieces, so a 2 GB plate costs a megabyte
//!    of RAM and not the whole file - the difference between a hash and an
//!    OOM on the 4 GB machine this has to render on. FNV-1a is byte-serial,
//!    so the chunked hash is *numerically identical* to the whole-file hash
//!    it replaces: every key that was valid stays valid, and no cache in
//!    the wild misses on the upgrade.
//! 2. **Publish in one piece.** An entry appears in the cache by writing
//!    beside it and renaming into place, so a concurrent render - another
//!    process, another agent, a farm worker - never reads a half-written
//!    file. The reel cache's `.done` markers are the sentinel: written
//!    after the rename, atomically themselves, so a marker that exists
//!    promises an entry that is whole. A bake's `manifest.json` is written
//!    last for the same reason.
//! 3. **Bound the growth.** Every render sweeps its own cache root under
//!    a size budget ([`DEFAULT_BUDGET_BYTES`], 2 GiB): the
//!    least-recently-used *complete* entries go first, the ones this render
//!    just wrote never do (their clocks are new), and junk - orphan
//!    markers, entries whose marker never arrived, temp files a crashed
//!    render left - goes before anything useful does.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The size every cache root is swept to stay under: two gibibytes, the
/// point where a cache stops being an accelerator and starts being the
/// reason the render ran out of disk.
pub const DEFAULT_BUDGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// How big a piece a hashed file is read in. A megabyte is small against a
/// 2 GB plate and large against the metadata cost of a chunk: one million
/// reads for the worst case, none of them holding more than this.
const CHUNK: usize = 1024 * 1024;

/// A temp file a crashed render left behind is swept once it is this old.
/// Younger ones are left alone: another render may be writing one right
/// now, and deleting a file somebody is filling is corruption, not
/// housekeeping.
const PART_LIFETIME: Duration = Duration::from_secs(60 * 60);

/// FNV-1a over a domain and a file's bytes, **read in chunks**: the same
/// number the whole-file hash produced, at a constant megabyte of memory.
///
/// The hash is a name, not a security property - the cache is not
/// adversarial - but it must be *stable*: two machines hashing the same
/// file reach the same key, or the shared cache is a lottery.
pub fn stream_hash(domain: &str, path: &Path) -> Option<u64> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(domain.as_bytes());
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(hash);
        }
        eat(&buffer[..read]);
    }
}

/// What a sweep found and did: the honest report a render can carry into
/// its reply, so a cache that ate itself is a cache the caller heard about.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Sweep {
    /// The entries removed, oldest first, by name.
    pub evicted: Vec<String>,
    /// The bytes the evictions freed.
    pub freed: u64,
    /// The bytes the root held after the sweep.
    pub kept: u64,
    /// The budget the sweep held the root under.
    pub budget: u64,
    /// Junk removed: orphan markers, markerless entries in the families that
    /// use markers, and temp files a crashed render left. None of these
    /// were ever servable, so none of them count as evictions.
    pub junk: Vec<String>,
}

/// The reel cache's layout, as the sweep reads it: each family is a
/// subdirectory of the root, its entries share one extension, and the
/// families that publish with a `.done` sentinel say so.
const FAMILIES: &[(&str, &str, bool)] = &[
    ("scenes", "mp4", true),
    ("joins", "mp4", false),
    ("beds", "m4a", false),
];

/// Sweep a reel cache root under a budget: the least-recently-used complete
/// entries go until the root fits, junk goes first, and the entries the
/// current render just wrote stay - a sweep that ate the render's own
/// output would be a cache that charges for admission.
///
/// "Used" is the newer of the entry and its marker, so serving an entry
/// (which touches the marker) counts as a use: the sweep is
/// least-*recently-used*, not least-recently-written.
pub fn sweep(root: &Path, budget: u64) -> Sweep {
    let mut report = Sweep {
        budget,
        ..Sweep::default()
    };
    let mut entries: Vec<(PathBuf, PathBuf, u64, SystemTime, bool)> = Vec::new();
    // (data, marker-or-placeholder, size, last use, has a marker to keep)
    for (family, extension, marked) in FAMILIES {
        let dir = root.join(family);
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Temp files are junk when they are old enough to be a crash's
            // and not a neighbour's.
            if name.contains(".part") {
                let age = entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .map(|at| SystemTime::now().duration_since(at).unwrap_or_default())
                    .unwrap_or_default();
                if age > PART_LIFETIME {
                    let _ = std::fs::remove_file(&path);
                    report.junk.push(name);
                }
                continue;
            }
            let Some(key) = name.strip_suffix(&format!(".{extension}")) else {
                // A marker, or a file of some other suffix: handled by its
                // own pass below, or not the cache's.
                continue;
            };
            let marker = dir.join(format!("{key}.done"));
            let data_size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            let data_time = entry.metadata().and_then(|meta| meta.modified()).ok();
            let marker_time = marker.metadata().and_then(|meta| meta.modified()).ok();
            if *marked && marker_time.is_none() {
                // The rename landed and the marker never did: a crash
                // between the two, or a pre-atomic leftover. Never served,
                // never missed - it goes.
                let _ = std::fs::remove_file(&path);
                report.junk.push(name);
                continue;
            }
            let (size, used) = if let Some(marker_time) = marker_time {
                (
                    data_size + marker.metadata().map(|meta| meta.len()).unwrap_or(0),
                    data_time.map_or(marker_time, |data_time| data_time.max(marker_time)),
                )
            } else {
                (data_size, data_time.unwrap_or(SystemTime::UNIX_EPOCH))
            };
            entries.push((path, marker, size, used, marker_time.is_some()));
        }
        // Orphan markers: a marker whose data never arrived. The promise
        // broken is the sweep's to keep - remove it, or the next render
        // believes an entry the cache cannot serve.
        for entry in std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(key) = name.strip_suffix(".done")
                && !dir.join(format!("{key}.{extension}")).is_file()
            {
                let _ = std::fs::remove_file(entry.path());
                report.junk.push(name);
            }
        }
    }
    entries.sort_by_key(|(_, _, _, used, _)| *used);
    report.kept = entries.iter().map(|(_, _, size, _, _)| *size).sum();
    while report.kept > budget {
        // oldest first; the sort above put them there.
        let Some((data, marker, size, _, _)) = entries.first() else {
            break;
        };
        let _ = std::fs::remove_file(data);
        let _ = std::fs::remove_file(marker);
        let name = data
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        report.evicted.push(name);
        report.kept = report.kept.saturating_sub(*size);
        report.freed += *size;
        entries.remove(0);
    }
    report
}

/// Sweep a comps bake cache root under a budget: the same three rules the
/// reel cache's [`sweep`] keeps, read over directories instead of files.
/// A bake is a directory whose `manifest.json` is its marker - written
/// last and renamed in, so a manifest present is a bake whole, and a
/// directory without one is a bake still filling or a crash's orphan.
///
/// "Used" is the manifest's clock, and a cache hit touches it (see
/// [`touch_file`]), so the sweep is least-*recently-used*, not merely
/// least-recently-baked. The bake the current render just published is
/// `keep`: never evicted, always counted - a sweep that ate the render's
/// own output would be a cache that charges for admission. Junk goes
/// before anything useful, and an in-flight bake (no manifest, younger
/// than an hour) is left entirely alone: deleting a directory somebody
/// is filling is corruption, not housekeeping.
pub fn sweep_comps(root: &Path, budget: u64, keep: Option<&Path>) -> Sweep {
    let mut report = Sweep {
        budget,
        ..Sweep::default()
    };
    let Ok(read) = std::fs::read_dir(root) else {
        return report;
    };
    // (dir, size, last use, protected from eviction)
    let mut entries: Vec<(PathBuf, u64, SystemTime, bool)> = Vec::new();
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            // A file in the root is a manifest's `.part` a crash left
            // behind; junk once it is old enough to be a crash's and not
            // a neighbour's.
            if name.contains(".part") && older_than(&meta, PART_LIFETIME) {
                let _ = std::fs::remove_file(&path);
                report.junk.push(name);
            }
            continue;
        }
        let manifest = path.join("manifest.json");
        if !manifest.is_file() {
            // No manifest: a bake still filling, or a crash's orphan.
            // Younger than an hour it is somebody's; older - and not the
            // bake this render is writing - it is junk.
            if older_than(&meta, PART_LIFETIME) && Some(path.as_path()) != keep {
                let _ = std::fs::remove_dir_all(&path);
                report.junk.push(name);
            }
            continue;
        }
        let used = manifest
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        entries.push((path, 0, used, false));
    }
    // Sizes after the junk pass, so nothing dead is walked or counted,
    // and the protected one is named where the caller said it is.
    for entry in &mut entries {
        entry.1 = dir_size(&entry.0);
        entry.3 = Some(entry.0.as_path()) == keep;
    }
    report.kept = entries.iter().map(|(_, size, _, _)| *size).sum();
    entries.sort_by_key(|(_, _, used, _)| *used);
    while report.kept > budget {
        // Oldest first, and never the protected one: the sort put the
        // oldest at the head, and the head is skipped while it is. When
        // everything left is protected the budget has to wait for a
        // later sweep - the one after the next bake.
        let Some(index) = entries.iter().position(|(_, _, _, protected)| !protected) else {
            break;
        };
        let (path, size, _, _) = entries.remove(index);
        let _ = std::fs::remove_dir_all(&path);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        report.evicted.push(name);
        report.kept = report.kept.saturating_sub(size);
        report.freed += size;
    }
    report
}

/// A path's whole size, files and all: a bake's bytes, frames first and
/// the manifest last, walked once per sweep.
fn dir_size(dir: &Path) -> u64 {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in read.filter_map(Result::ok) {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

/// Whether `meta` was last changed longer ago than `lifetime`.
fn older_than(meta: &std::fs::Metadata, lifetime: Duration) -> bool {
    meta.modified()
        .map(|at| SystemTime::now().duration_since(at).unwrap_or_default())
        .is_ok_and(|age| age > lifetime)
}

/// Touch a file's clock so a *served* cache entry counts as used: the
/// comps sweep keys on the manifest's mtime, and a bake every render
/// hits would be the first evicted if its popularity never showed in
/// the clock. A missing file is left alone - the sweep's junk pass is
/// the one that clears what never arrived.
pub fn touch_file(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// The machine's physical memory where the platform says: `/proc/meminfo`
/// on Linux - the machine the farm and the 4 GB laptop both are. `None`
/// anywhere else, and the budget takes its usual size rather than guess.
fn machine_memory() -> Option<u64> {
    static TOTAL: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *TOTAL.get_or_init(|| {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = info.lines().find(|line| line.starts_with("MemTotal:"))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib.saturating_mul(1024))
    })
}

/// The comps budget, from its two inputs: the operator's named gibibytes,
/// or the machine's own size - a small machine (six gigabytes and under,
/// the budget-laptop class whose RAM is shared with a whole desktop) gets
/// half, so the cache is never the reason a render swapped. Pure, so the
/// step-down line can be tested without a machine.
fn budget_from(named: Option<f64>, machine: Option<u64>) -> u64 {
    const GIB: u64 = 1024 * 1024 * 1024;
    if let Some(named) = named
        && named.is_finite()
        && named > 0.0
    {
        // A named budget is the operator's own arithmetic, clamped to the
        // range a disk survives: a tenth of a gigabyte to a terabyte.
        let bytes = (named * GIB as f64) as u64;
        return bytes.clamp(GIB / 10, 1024 * GIB);
    }
    const SMALL: u64 = 6 * GIB;
    if machine.is_some_and(|total| total <= SMALL) {
        GIB
    } else {
        DEFAULT_BUDGET_BYTES
    }
}

/// The size every comps bake cache is swept to stay under: the operator's
/// `CONCAT_MOTION_CACHE_GIB`, or the machine's own answer - two gibibytes
/// on a workstation, one on the budget laptop. `cache/comps` grows a
/// directory per comp-content and size, and without a budget it grows
/// forever.
pub fn comps_budget() -> u64 {
    let named = std::env::var("CONCAT_MOTION_CACHE_GIB")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok());
    budget_from(named, machine_memory())
}

/// A unique temp path beside `to`: the name a render fills before it
/// publishes. Two renders of one entry, from two processes, each fill
/// their own - that is the whole point of the `pid` in the name.
///
/// The temp name keeps the destination's extension and hides its dot:
/// the engine reads an output's **format** from its extension, so a part
/// named `.<key>.mp4.part.<pid>.<n>` would be read as an asset of some
/// unknown kind and refused (E501) - the part is named
/// `.<key>.part.<pid>.<n>.mp4` instead, an mp4 beside its own place.
pub fn part_path(to: &Path) -> PathBuf {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = to
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let part = match name.rsplit_once('.') {
        Some((stem, extension)) => {
            format!(".{stem}.part.{}.{count}.{extension}", std::process::id())
        }
        None => format!(".{name}.part.{}.{}", std::process::id(), count),
    };
    let parent = to.parent().unwrap_or_else(|| Path::new("."));
    parent.join(part)
}

/// Publish a file that is already rendered: rename `from` to `to`,
/// atomically, so no reader ever sees `to` half-written. Both paths are
/// the caller's own - a render into [`part_path`], published here.
pub fn publish(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)
}

/// Copy `from` into the cache as `to`, in one piece: the bytes land in a
/// temp file beside the destination and are renamed in, so a join copied
/// into the cache is never readable before it is whole.
pub fn publish_copy(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = part_path(to);
    std::fs::copy(from, &part)?;
    publish(&part, to)?;
    Ok(())
}

/// Write the `.done` marker that makes a cache entry servable: the last
/// write of a publish, itself atomic, so the promise it makes is never
/// ahead of the bytes it promises for. The `key` inside must match what the
/// next render computes or the entry is (correctly) re-rendered.
pub fn mark_done(entry: &Path, key: &str, seconds: f64) -> Result<(), std::io::Error> {
    let marker = entry.with_extension("done");
    let part = part_path(&marker);
    std::fs::write(&part, format!("{key} {seconds:.3}"))?;
    std::fs::rename(&part, &marker)?;
    Ok(())
}

/// Touch an entry's marker so a *served* entry counts as used: the sweep
/// evicts what nothing has read lately, not what was merely baked longest
/// ago. A missing marker is left alone - touching a marker onto an entry
/// that has none would make junk look servable, and the content never
/// changes: only the clock does.
pub fn touch(entry: &Path) {
    let marker = entry.with_extension("done");
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&marker) {
        let _ = file.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chunked hash answers exactly what the whole-file hash did: the
    /// keys every cache in the wild already holds stay valid.
    #[test]
    fn the_chunked_hash_answers_the_whole_file_number() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plate.bin");
        let mut file = std::fs::File::create(&path).unwrap();
        // comfortably past one chunk boundary
        for round in 0..(CHUNK / 1024 * 3) {
            file.write_all(&[u8::try_from(round % 251).unwrap(); 1024])
                .unwrap();
        }
        drop(file);
        let bytes = std::fs::read(&path).unwrap();
        let mut whole: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in b"concat-motion/reel-cache/1".iter().chain(bytes.iter()) {
            whole ^= u64::from(*byte);
            whole = whole.wrapping_mul(0x0000_0100_0000_01b3);
        }
        assert_eq!(
            stream_hash("concat-motion/reel-cache/1", &path),
            Some(whole)
        );
    }

    /// A missing file hashes to nothing: an asset that is not there is a
    /// miss, never a hash of the empty string pretending otherwise.
    #[test]
    fn a_missing_file_hashes_to_nothing() {
        assert_eq!(stream_hash("any", Path::new("/no/such/file")), None);
    }

    /// The sweep holds the budget by evicting the least recently used, and
    /// a served entry outlives one merely baked earlier.
    #[cfg(unix)]
    #[test]
    fn the_sweep_evicts_the_least_recently_used_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (name, size) in [("old", 40), ("read", 40), ("new", 40)] {
            let entry = root.join(format!("scenes/{name}.mp4"));
            std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
            std::fs::write(&entry, vec![0u8; size]).unwrap();
            mark_done(&entry, name, 1.0).unwrap();
        }
        // 2015, 2020, 2025: the "read" one was served in 2025, so the
        // oldest *baked* one is the one that goes first.
        let touch_old = |name: &str, stamp: &str| {
            let _ = std::process::Command::new("touch")
                .arg("-t")
                .arg(stamp)
                .arg(root.join(format!("scenes/{name}.mp4")))
                .arg(root.join(format!("scenes/{name}.done")))
                .status();
        };
        touch_old("old", "201501010000");
        touch_old("read", "202001010000");
        touch_old("new", "202501010000");
        touch(&root.join("scenes/read.mp4")); // served now: the newest mtime of the lot

        // Three 49-byte entries is ~147: a budget of 100 evicts exactly
        // the stale one and stops.
        let report = sweep(root, 100);
        assert_eq!(
            report.evicted,
            vec!["old.mp4".to_owned()],
            "the stale one goes, not the served one"
        );
        assert!(root.join("scenes/read.mp4").is_file());
        assert!(root.join("scenes/new.mp4").is_file());
        assert!(!root.join("scenes/old.mp4").is_file());
        assert!(report.kept <= 100);
    }

    /// Junk goes before anything useful: a marker without its entry and an
    /// entry without its marker are both promises the cache cannot keep,
    /// and a temp file a crash left is neither.
    #[cfg(unix)]
    #[test]
    fn the_sweep_clears_junk_before_evicting_anything_useful() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let scenes = root.join("scenes");
        std::fs::create_dir_all(&scenes).unwrap();
        std::fs::write(scenes.join("kept.mp4"), vec![0u8; 10]).unwrap();
        mark_done(&scenes.join("kept.mp4"), "kept", 1.0).unwrap();
        std::fs::write(scenes.join("orphan.done"), b"orphan 1.000").unwrap();
        std::fs::write(scenes.join("markerless.mp4"), vec![0u8; 10]).unwrap();
        let stale_part = part_path(&scenes.join("never-finished.mp4"));
        std::fs::write(&stale_part, vec![0u8; 10]).unwrap();
        let _ = std::process::Command::new("touch")
            .arg("-t")
            .arg("202001010000")
            .arg(&stale_part)
            .status();

        let report = sweep(root, DEFAULT_BUDGET_BYTES);
        assert!(report.junk.contains(&"orphan.done".to_owned()));
        assert!(report.junk.contains(&"markerless.mp4".to_owned()));
        assert!(
            report
                .junk
                .iter()
                .any(|name| name.contains("never-finished"))
        );
        assert!(report.evicted.is_empty());
        assert!(scenes.join("kept.mp4").is_file());
    }

    /// Publishing a copy never leaves the destination half-written: the
    /// temp file is beside the destination, and the destination appears
    /// whole, with no `.part` left behind.
    #[test]
    fn a_published_copy_lands_whole_and_leaves_no_part_behind() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("rendered.mp4");
        std::fs::write(&from, b"the bytes a render made").unwrap();
        let to = dir.path().join("cache/joins/abc.mp4");
        publish_copy(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"the bytes a render made");
        let parts: Vec<_> = std::fs::read_dir(to.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".part"))
            .collect();
        assert!(parts.is_empty(), "parts left behind: {parts:?}");
    }

    /// The marker matches the entry it marks, or the next render re-renders;
    /// a marker that cannot be read does not become a hit by accident of
    /// existing.
    #[test]
    fn a_marker_names_its_entry_and_its_duration() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("scenes/key.mp4");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, b"segment").unwrap();
        mark_done(&entry, "key", 2.5).unwrap();
        let text = std::fs::read_to_string(entry.with_extension("done")).unwrap();
        assert_eq!(text, "key 2.500");
    }

    /// The comps sweep evicts the least recently used bake - a served one
    /// outliving one merely baked earlier - and keeps the bake this render
    /// just wrote, counting its bytes all the same: a cache that charges
    /// for admission is no cache at all.
    #[cfg(unix)]
    #[test]
    fn the_comps_sweep_evicts_the_stale_bake_and_keeps_the_fresh_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("comps");
        let bake = |name: &str, size: usize| {
            let path = root.join(name);
            std::fs::create_dir_all(path.join("frames")).unwrap();
            std::fs::write(path.join("frames/0001.png"), vec![0u8; size]).unwrap();
            std::fs::write(path.join("manifest.json"), b"{}").unwrap();
        };
        bake("old", 40);
        bake("read", 40);
        bake("fresh", 40);
        let stamp = |name: &str, when: &str| {
            let _ = std::process::Command::new("touch")
                .arg("-t")
                .arg(when)
                .arg(root.join(format!("{name}/manifest.json")))
                .status();
        };
        stamp("old", "201501010000");
        stamp("read", "202001010000");
        stamp("fresh", "202501010000");
        touch_file(&root.join("read/manifest.json")); // served now

        // Three ~42-byte bakes is ~126: a budget of 100 evicts exactly the
        // stale one and stops.
        let report = sweep_comps(&root, 100, Some(&root.join("fresh")));
        assert_eq!(
            report.evicted,
            vec!["old".to_owned()],
            "the stale one goes, not the served one and not the fresh one"
        );
        assert!(root.join("fresh/manifest.json").is_file());
        assert!(root.join("read/manifest.json").is_file());
        assert!(!root.join("old").exists());
        assert!(report.kept <= 100);
    }

    /// Junk goes before anything useful: a bake directory whose manifest
    /// never arrived is a crash's orphan once it is an hour old, and a
    /// bake still filling - young, manifestless - is left alone, because
    /// deleting a directory somebody is filling is corruption.
    #[cfg(unix)]
    #[test]
    fn the_comps_sweep_clears_orphans_and_leaves_the_in_flight_bake() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("comps");
        let kept = root.join("kept-1x1@30");
        std::fs::create_dir_all(kept.join("frames")).unwrap();
        std::fs::write(kept.join("frames/0001.png"), vec![0u8; 10]).unwrap();
        std::fs::write(kept.join("manifest.json"), b"{}").unwrap();
        let orphan = root.join("crashed-1x1@30");
        std::fs::create_dir_all(orphan.join("frames")).unwrap();
        std::fs::write(orphan.join("frames/half.png"), vec![0u8; 10]).unwrap();
        let _ = std::process::Command::new("touch")
            .arg("-t")
            .arg("202001010000")
            .arg(&orphan)
            .status();
        let in_flight = root.join("baking-2x1@30");
        std::fs::create_dir_all(in_flight.join("frames")).unwrap();

        let report = sweep_comps(&root, DEFAULT_BUDGET_BYTES, None);
        assert_eq!(report.junk, vec!["crashed-1x1@30".to_owned()]);
        assert!(report.evicted.is_empty());
        assert!(kept.join("manifest.json").is_file());
        assert!(in_flight.is_dir(), "an in-flight bake is never swept");
    }

    /// The budget answers the operator first and the machine second: a
    /// named size is the operator's own arithmetic, a small machine gets
    /// half the default, and a machine that will not say keeps the whole
    /// of it.
    #[test]
    fn the_comps_budget_answers_the_operator_then_the_machine() {
        let gib = 1024 * 1024 * 1024;
        assert_eq!(budget_from(Some(0.5), None), gib / 2);
        assert_eq!(budget_from(Some(3.0), None), 3 * gib);
        assert_eq!(budget_from(Some(-1.0), None), DEFAULT_BUDGET_BYTES);
        assert_eq!(budget_from(None, Some(4 * gib)), gib);
        assert_eq!(budget_from(None, Some(6 * gib)), gib);
        assert_eq!(budget_from(None, Some(32 * gib)), DEFAULT_BUDGET_BYTES);
        assert_eq!(budget_from(None, None), DEFAULT_BUDGET_BYTES);
    }
}
