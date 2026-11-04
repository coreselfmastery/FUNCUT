// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The scratch an export writes beside its destination, and the claim on
//! the destination itself.
//!
//! An export renders into three hidden siblings of the file it is making
//! - the silent picture, the mixed sound, the two joined - and moves the
//! finished one over the destination in a single rename, so the file a
//! person finds is a file that is whole. The siblings are named after
//! this process and this run, so two exports of one destination - a
//! window and a CLI, a window and a second window - never write one
//! another's bytes; and the destination is claimed for the run with an
//! operating-system lock, so the second of two such exports is refused
//! rather than left to race the first to the rename.
//!
//! The claim's file lives in the session's runtime folder - or the
//! system's temporary one where there is no session - keyed by the
//! destination's path, and the lock is the operating system's, not the
//! file's: it lives exactly as long as the process's handle, so an
//! export that crashes or is killed lets go of it and the next export to
//! the same destination starts clean. What a crash does leave behind is
//! the scratch it was writing, and the first thing the next export to
//! that destination does is sweep that away.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// One export's scratch: the three files it renders into, and the claim
/// on the destination they will rename over. Dropped on every way out of
/// the render - written, failed, cancelled, a panic unwinding through it
/// - so a folder is never left holding half an export that survived it.
#[derive(Debug)]
pub struct Scratch {
    /// The silent picture, before any sound is joined to it.
    video: PathBuf,
    /// The mixed sound, before it is joined to the picture.
    audio: PathBuf,
    /// The picture and the sound, joined.
    muxed: PathBuf,
    /// The destination's claim, held until this is dropped. `None` where
    /// the runtime folder would not take the file - the claim is a
    /// courtesy between processes, and its absence costs the refusal,
    /// not the export.
    lock: Option<File>,
}

impl Scratch {
    /// Claims `output`'s destination and names this run's scratch beside
    /// it. Refused when another process holds the destination; a folder
    /// that cannot hold the claim file at all is exported to without it.
    ///
    /// Whatever an earlier run left beside the destination goes now:
    /// with the claim held, nothing else can be writing to it, so any
    /// `.{stem}.concat-*` sibling is a leftover from a crashed or killed
    /// run - this process's or another's - and not a file in use.
    pub fn begin(output: &Path) -> Result<Scratch, String> {
        let stem = output.file_stem().map_or_else(
            || "concat".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let directory = output.parent().unwrap_or(Path::new(".")).to_path_buf();
        let lock = match claim(output) {
            Ok(lock) => lock,
            Err(error) => return Err(error),
        };
        let removed = sweep(&directory, &stem);
        if removed > 0 {
            log::info!(
                "export: cleared {removed} scratch file(s) left beside {}",
                output.display()
            );
        }
        Ok(Scratch {
            video: fresh(&directory, &stem, "video", "mp4"),
            audio: fresh(&directory, &stem, "audio", "m4a"),
            muxed: fresh(&directory, &stem, "muxed", "mp4"),
            lock,
        })
    }

    /// The silent picture's path.
    pub fn video(&self) -> &Path {
        &self.video
    }

    /// The mixed sound's path.
    pub fn audio(&self) -> &Path {
        &self.audio
    }

    /// The joined picture and sound's path.
    pub fn muxed(&self) -> &Path {
        &self.muxed
    }
}

impl Drop for Scratch {
    /// The scratch goes, whichever way the render ended. A file that was
    /// renamed into place is no longer at its scratch name, and removing
    /// a name that is not there costs nothing.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.video);
        let _ = std::fs::remove_file(&self.audio);
        let _ = std::fs::remove_file(&self.muxed);
        if let Some(lock) = self.lock.take() {
            let _ = lock.unlock();
        }
    }
}

/// Claims `output` for this process: an exclusive lock on a file in the
/// session's runtime folder, named after the destination as the
/// filesystem spells it. `Err` names the refusal when another process
/// holds it; `Ok(None)` when no claim could be made at all.
fn claim(output: &Path) -> Result<Option<File>, String> {
    let key = fnv1a(&spelled(output));
    let folder = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let path = folder.join(format!("concat-export-{key:016x}.lock"));
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) => {
            log::warn!(
                "export: no claim on the destination ({}: {error}); \
                 another process exporting to it would not be refused",
                path.display()
            );
            return Ok(None);
        }
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Err(format!(
            "{} is being exported by another process; wait for it or choose another name",
            output.display()
        )),
        Err(TryLockError::Error(error)) => {
            log::warn!(
                "export: no claim on the destination ({}: {error}); \
                 another process exporting to it would not be refused",
                path.display()
            );
            Ok(None)
        }
    }
}

/// `output` as the filesystem spells it: the folder resolved to its real
/// path - so a symlinked parent, a `..`, and a trailing separator all
/// name one destination - with the file's own name back on it. The file
/// itself does not exist yet, so it cannot be resolved whole.
fn spelled(output: &Path) -> String {
    let name = output.file_name().map_or_else(
        || output.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    match output
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
    {
        Some(parent) => parent.join(name).to_string_lossy().into_owned(),
        None => output.to_string_lossy().into_owned(),
    }
}

/// Removes the scratch an earlier run left beside a destination: the
/// files this module has ever named there. Returns how many went.
fn sweep(directory: &Path, stem: &str) -> usize {
    let prefix = format!(".{stem}.concat-");
    let legacy = [
        format!(".{stem}.concat-video.mp4"),
        format!(".{stem}.concat-audio.m4a"),
        format!(".{stem}.concat-muxed.mp4"),
    ];
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let scratch = legacy.iter().any(|leftover| *leftover == name)
            || (name.starts_with(&prefix)
                && ["video.", "audio.", "muxed."]
                    .iter()
                    .any(|stage| name[prefix.len()..].starts_with(stage)));
        if scratch && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// A scratch name no other run - not one in this process, not one in
/// another - is writing to: the destination's stem, the stage, this
/// process and a counter of this call.
fn fresh(directory: &Path, stem: &str, stage: &str, extension: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let mut taken = 0;
    loop {
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(
            ".{stem}.concat-{stage}.{}.{nonce}.{extension}",
            std::process::id()
        ));
        // `exists` is a race and not a lock: the encoder creates the
        // file itself. A name already taken is tried again under the
        // next counter; sixteen in a row cannot happen, and the last
        // resort is a name that is still this run's own.
        if !candidate.exists() || taken >= 16 {
            return candidate;
        }
        taken += 1;
    }
}

/// FNV-1a, sixty-four bits, over the bytes of `text`: a short, stable
/// name for a path, so a claim file made by one build is found by the
/// next.
fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scratch names are this run's own: two asks never share one,
    /// and a name says what it is - hidden, the stem, the stage, the
    /// process, and the extension the muxer needs.
    #[test]
    fn scratch_names_are_this_runs_own() {
        let directory = Path::new("/scratch");
        let first = fresh(directory, "movie", "video", "mp4");
        let second = fresh(directory, "movie", "video", "mp4");
        assert_ne!(first, second, "two scratch files never share a name");
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(".movie.concat-video."), "{name}");
        assert!(name.ends_with(".mp4"), "{name}");
        assert!(
            name.contains(&format!(".{}.", std::process::id())),
            "{name}"
        );
    }

    /// A destination spelled two ways is one destination: the claim is
    /// keyed by the folder the filesystem resolves to.
    #[test]
    fn a_destination_spelled_two_ways_is_one() {
        let real = std::env::temp_dir();
        let one = real.join("concat-claims-the-same.mp4");
        let two = real.join(".").join("concat-claims-the-same.mp4");
        let parent = real.canonicalize().unwrap_or(real.clone());
        assert_eq!(
            spelled(&one),
            parent.join("concat-claims-the-same.mp4").to_string_lossy(),
        );
        assert_eq!(spelled(&one), spelled(&two));
    }

    /// One folder this test owns, cleaned around itself like the crate's
    /// other tests: no dev-dependencies for a directory.
    fn folder(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("concat-scratch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    /// The sweep takes the scratch beside a destination and nothing
    /// else: the leftovers of this naming, the leftovers of the naming
    /// before it, and not the destination's neighbours.
    #[test]
    fn the_sweep_takes_the_scratch_and_nothing_else() {
        let scratch = folder("sweep");
        let video = scratch.join(".movie.concat-video.123.1.mp4");
        let audio = scratch.join(".movie.concat-audio.123.2.m4a");
        let old = scratch.join(".movie.concat-muxed.mp4");
        let movie = scratch.join("movie.mp4");
        let other = scratch.join(".other.concat-video.123.1.mp4");
        let deep = scratch.join(".movie.concat-videos.123.1.mp4");
        for file in [&video, &audio, &old, &movie, &other, &deep] {
            std::fs::write(file, b"x").expect("writes");
        }
        assert_eq!(sweep(&scratch, "movie"), 3);
        assert!(!video.exists() && !audio.exists() && !old.exists());
        assert!(movie.exists() && other.exists() && deep.exists());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A second claim on one destination is refused, and a different
    /// destination is not: the claim is the destination's alone. The
    /// second claim in one process meets the same refusal a second
    /// process would - the lock is the open file's, not the process's.
    #[test]
    fn a_second_claim_on_a_destination_is_refused() {
        let scratch = folder("claim");
        let one = scratch.join("one.mp4");
        let two = scratch.join("two.mp4");
        let held = Scratch::begin(&one).expect("claims");
        assert_eq!(
            held.video().extension().and_then(|e| e.to_str()),
            Some("mp4")
        );
        let refused = Scratch::begin(&one).expect_err("held");
        assert!(
            refused.contains("another process"),
            "the refusal says who: {refused}"
        );
        let elsewhere = Scratch::begin(&two).expect("another destination is free");
        drop(held);
        drop(elsewhere);
        let again = Scratch::begin(&one).expect("free once let go");
        drop(again);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Dropping the scratch clears it away, the rename having taken what
    /// it wanted: a failed or cancelled render leaves nothing beside the
    /// destination.
    #[test]
    fn dropping_the_scratch_clears_it_away() {
        let scratch = folder("drop");
        let output = scratch.join("gone.mp4");
        let held = Scratch::begin(&output).expect("claims");
        for file in [held.video(), held.audio(), held.muxed()] {
            std::fs::write(file, b"half an export").expect("writes");
        }
        drop(held);
        for name in [
            ".gone.concat-video",
            ".gone.concat-audio",
            ".gone.concat-muxed",
        ] {
            let left: Vec<_> = std::fs::read_dir(&scratch)
                .expect("reads")
                .flatten()
                .map(|entry| entry.file_name())
                .filter(|file| file.to_string_lossy().starts_with(name))
                .collect();
            assert!(left.is_empty(), "{name} files left behind: {left:?}");
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
