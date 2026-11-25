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
//!
//! Three generators live beside them, each a set of its own and each its
//! own row: [`SFX_SMALL`] (TinyAudio-MF), [`SFX_LARGE`] (Stable Audio Open
//! Small) and [`BGM`] (TinyMusician). The same download path serves all
//! four sets; only the row id differs, and `sound_model.py generate`
//! reads the files from the same folder.

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

/// TinyAudio-MF: the small SFX generator. 87 M parameters, one flow step,
/// ten seconds of 44.1 kHz audio per call - the cheapest of the three by
/// a wide margin, which is why the accents use it.
pub const SFX_SMALL: [ModelId; 5] = [
    ModelId::SfxSmallPipeline,
    ModelId::SfxSmallTokenizer,
    ModelId::SfxSmallVae,
    ModelId::SfxSmallText,
    ModelId::SfxSmallFlow,
];

/// Stable Audio Open Small: the heavier SFX generator. 341 M parameters,
/// eight distilled steps, 11.9 seconds of stereo per call. Spent on the
/// one accent that carries the hook, not on every hit.
pub const SFX_LARGE: [ModelId; 9] = [
    ModelId::SfxLargeMeta,
    ModelId::SfxLargeTokenizer,
    ModelId::SfxLargeSeconds,
    ModelId::SfxLargeVae,
    ModelId::SfxLargeVaeData,
    ModelId::SfxLargeT5,
    ModelId::SfxLargeT5Data,
    ModelId::SfxLargeDit,
    ModelId::SfxLargeDitData,
];

/// TinyMusician: the BGM generator. MusicGen at 15 layers, four
/// codebooks, 32 kHz mono, and no cache in its exported decoder - the
/// slowest of the three per second of audio.
pub const BGM: [ModelId; 5] = [
    ModelId::BgmConfig,
    ModelId::BgmTokenizer,
    ModelId::BgmCodec,
    ModelId::BgmText,
    ModelId::BgmDecoder,
];

/// Where the sound models live: the app's own model folder, the one the
/// vision models and the sidecar share.
pub fn folder(data: &Path) -> PathBuf {
    models_dir(data)
}

/// The bytes a full set takes, for the row's size before anything is
/// fetched.
pub fn bytes() -> u64 {
    set_bytes(&SOUND_MODELS)
}

/// The bytes still to fetch.
pub fn missing_bytes(data: &Path) -> u64 {
    set_missing_bytes(data, &SOUND_MODELS)
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
    set_missing(data, &SOUND_MODELS)
}

/// Whether the reader will find a working set: CLAP needs both towers and
/// the tokenizer, and the labels need YAMNet, so a partial set is not
/// installed however many files are on disk.
pub fn ready(data: &Path) -> bool {
    set_ready(data, &SOUND_MODELS)
}

/// Fetch what is missing. `progress` is called with (received, total) over
/// the whole set, so one bar covers five files.
pub fn download(
    data: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    set_download(data, &SOUND_MODELS, cancel, progress)
}

/// Remove the set. A model the user fetched is theirs to delete, and the
/// reader degrades to measurements alone when it is gone.
pub fn remove(data: &Path) -> Result<(), String> {
    set_remove(data, &SOUND_MODELS)
}

/// The three generator sets, by the row id the settings sheet uses.
pub fn generator_set(id: &str) -> Option<&'static [ModelId]> {
    match id {
        "sfx-small" => Some(&SFX_SMALL),
        "sfx-large" => Some(&SFX_LARGE),
        "bgm" => Some(&BGM),
        _ => None,
    }
}

/// The bytes a generator set takes.
pub fn set_bytes(set: &[ModelId]) -> u64 {
    set.iter().map(|id| id.spec().bytes).sum()
}

/// The bytes of a set still to fetch.
pub fn set_missing_bytes(data: &Path, set: &[ModelId]) -> u64 {
    set.iter()
        .filter(|id| !installed(data, **id))
        .map(|id| id.spec().bytes)
        .sum()
}

/// Every file of a set that is missing.
pub fn set_missing(data: &Path, set: &[ModelId]) -> Vec<ModelId> {
    set.iter()
        .copied()
        .filter(|id| !installed(data, *id))
        .collect()
}

/// Whether every file of a set is on disk and whole.
pub fn set_ready(data: &Path, set: &[ModelId]) -> bool {
    set.iter().all(|id| installed(data, *id))
}

/// Fetch what is missing from one set, smallest first. `progress` is
/// called with (received, total) over the files still to fetch.
pub fn set_download(
    data: &Path,
    set: &[ModelId],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let wanted = set_missing(data, set);
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

/// Remove one set.
pub fn set_remove(data: &Path, set: &[ModelId]) -> Result<(), String> {
    for id in set {
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
    fn the_generator_sets_cover_every_generator_in_the_registry() {
        let mut from_sets: Vec<&str> = SFX_SMALL
            .iter()
            .chain(SFX_LARGE.iter())
            .chain(BGM.iter())
            .map(|id| id.spec().file)
            .collect();
        let mut from_registry: Vec<&str> = MODELS
            .iter()
            .filter(|spec| spec.id.kind() == "generate")
            .map(|spec| spec.file)
            .collect();
        from_sets.sort_unstable();
        from_registry.sort_unstable();
        assert_eq!(from_sets, from_registry);
        for (id, set) in [
            ("sfx-small", &SFX_SMALL[..]),
            ("sfx-large", &SFX_LARGE[..]),
            ("bgm", &BGM[..]),
        ] {
            assert_eq!(generator_set(id), Some(set));
        }
        assert_eq!(generator_set("sound"), None);
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
