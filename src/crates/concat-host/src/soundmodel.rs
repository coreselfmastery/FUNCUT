// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The sound models: CLAP's two towers and tokenizer, YAMNet and its class
//! names. Fetched the way the cutout models are - into the same
//! `cutout-models` folder, each file verified against its digest - because
//! that is the folder `sound_model.py` reads, so a model downloaded here is
//! the model the reader uses without either side being told.
//!
//! They are one row in the sheet, not five: nobody wants to pick a
//! tokenizer. [`SOUND_MODELS`] is the whole set, [`installed`] says which
//! files are already on disk, and [`download`] fetches only the missing
//! ones, so a half-finished set resumes instead of starting again.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use concat_vision::models::{MODELS, ModelId, models_dir};

/// The files the sound reader needs, in the order they are fetched: the
/// smallest first, so an interrupted download still leaves a usable
/// class map and tokenizer behind.
pub const SOUND_MODELS: [ModelId; 5] = [
    ModelId::YamnetNames,
    ModelId::Yamnet,
    ModelId::ClapTokenizer,
    ModelId::ClapAudio,
    ModelId::ClapText,
];

/// Where the sound models live: the app's own model folder, the one the
/// vision models and the sidecar share.
pub fn folder(data: &Path) -> PathBuf {
    models_dir(data)
}

/// The bytes a full set takes, for the row's size before anything is
/// fetched.
pub fn bytes() -> u64 {
    SOUND_MODELS.iter().map(|id| id.spec().bytes).sum()
}

/// The bytes still to fetch.
pub fn missing_bytes(data: &Path) -> u64 {
    SOUND_MODELS
        .iter()
        .filter(|id| !installed(data, **id))
        .map(|id| id.spec().bytes)
        .sum()
}

/// Whether one file is on disk and whole.
pub fn installed(data: &Path, id: ModelId) -> bool {
    let file = folder(data).join(id.spec().file);
    file.is_file()
        && std::fs::metadata(&file)
            .map(|meta| meta.len() == id.spec().bytes)
            .unwrap_or(false)
}

/// Every file that is missing.
pub fn missing(data: &Path) -> Vec<ModelId> {
    SOUND_MODELS
        .iter()
        .copied()
        .filter(|id| !installed(data, *id))
        .collect()
}

/// Whether the reader will find a working set: CLAP needs both towers and
/// the tokenizer, and the labels need YAMNet, so a partial set is not
/// installed however many files are on disk.
pub fn ready(data: &Path) -> bool {
    SOUND_MODELS.iter().all(|id| installed(data, *id))
}

/// Fetch what is missing. `progress` is called with (received, total) over
/// the whole set, so one bar covers five files.
pub fn download(
    data: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let wanted = missing(data);
    if wanted.is_empty() {
        return Ok(());
    }
    let dir = folder(data);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let total: u64 = wanted.iter().map(|id| id.spec().bytes).sum();
    let mut done: u64 = 0;
    for id in wanted {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("sound models cancelled".to_owned());
        }
        let spec = id.spec();
        let file = dir.join(spec.file);
        let partial = file.with_extension("part");
        let base = done;
        progress(base, total);
        crate::models::fetch(
            spec.file,
            spec.upstream,
            &partial,
            spec.bytes,
            spec.sha256,
            cancel,
            "sound models cancelled",
            &mut |received, _total| progress(base + received, total),
        )?;
        std::fs::rename(&partial, &file)
            .map_err(|error| format!("could not finish {}: {error}", file.display()))?;
        done += spec.bytes;
        progress(done, total);
    }
    Ok(())
}

/// Remove the set. A model the user fetched is theirs to delete, and the
/// reader degrades to measurements alone when it is gone.
pub fn remove(data: &Path) -> Result<(), String> {
    for id in SOUND_MODELS {
        let file = folder(data).join(id.spec().file);
        if file.is_file() {
            std::fs::remove_file(&file)
                .map_err(|error| format!("could not remove {}: {error}", file.display()))?;
        }
    }
    Ok(())
}

/// Every sound model in the registry, for a test that keeps the two lists
/// saying the same thing.
pub fn registry_kinds() -> Vec<&'static str> {
    MODELS
        .iter()
        .filter(|spec| spec.id.kind() == "sound")
        .map(|spec| spec.file)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_is_every_sound_model_in_the_registry() {
        let mut from_set: Vec<&str> = SOUND_MODELS.iter().map(|id| id.spec().file).collect();
        let mut from_registry = registry_kinds();
        from_set.sort_unstable();
        from_registry.sort_unstable();
        assert_eq!(from_set, from_registry);
    }

    #[test]
    fn a_partial_set_is_not_ready() {
        let dir = std::env::temp_dir().join(format!("concat-soundmodel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(folder(&dir)).unwrap();
        assert!(!ready(&dir));
        // A file of the right size is the whole test the row needs: the
        // digest is checked when it arrives, not every time it is read.
        let id = ModelId::YamnetNames;
        let path = folder(&dir).join(id.spec().file);
        std::fs::write(&path, vec![0u8; id.spec().bytes as usize]).unwrap();
        assert!(installed(&dir, id));
        assert!(!ready(&dir));
        assert_eq!(missing(&dir).len(), 4);
        remove(&dir).unwrap();
        assert!(!installed(&dir, id));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
