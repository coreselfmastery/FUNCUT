// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The picture reader's four models: a shot boundary, a depth map, a visual
//! embedding and CLIP's vision tower. Fetched into the same `cutout-models`
//! folder the cutout and sound models use, each file verified against its
//! digest, because that is the folder `visual_sense.py` reads - so a set
//! downloaded here is the set the reader measures with, without either side
//! being told about the other.
//!
//! One row in the sheet, not four: they are what the AUTO EDIT's stock
//! choice spends together. The other twenty-six rows on the reader's shelf
//! are specialist doors - an action classifier, a tracker, a gaze model -
//! and are installed per model through `sense_visual {action: "download"}`,
//! so the sheet carries the set a first edit needs and not 3 GB of
//! everything.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use concat_vision::models::{MODELS, ModelId, models_dir};

/// The files the picture reader spends on a first edit, smallest first, so
/// an interrupted download still leaves the depth and similarity reads
/// behind.
pub const VISUAL_MODELS: [ModelId; 4] = [
    ModelId::Dinov2,
    ModelId::DepthAnything,
    ModelId::TransNetV2,
    ModelId::ClipVision,
];

/// Where they live: the app's own model folder, the one the cutout models,
/// the sound models and the sidecar share.
pub fn folder(data: &Path) -> PathBuf {
    models_dir(data)
}

/// The bytes a full set takes, for the row's size before anything is
/// fetched.
pub fn bytes() -> u64 {
    VISUAL_MODELS.iter().map(|id| id.spec().bytes).sum()
}

/// The bytes still to fetch.
pub fn missing_bytes(data: &Path) -> u64 {
    VISUAL_MODELS
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
    VISUAL_MODELS
        .iter()
        .copied()
        .filter(|id| !installed(data, *id))
        .collect()
}

/// Whether the reader will find a working set. The reads are independent -
/// a depth map without a shot boundary is still a depth map - so this is
/// every file, and [`installed`] is what a partial set answers.
pub fn ready(data: &Path) -> bool {
    VISUAL_MODELS.iter().all(|id| installed(data, *id))
}

/// Fetch what is missing. `progress` is called with (received, total) over
/// the whole set, so one bar covers four files.
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
            return Err("picture models cancelled".to_owned());
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
            "picture models cancelled",
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
/// reader degrades to the classical reads alone when it is gone.
pub fn remove(data: &Path) -> Result<(), String> {
    for id in VISUAL_MODELS {
        let file = folder(data).join(id.spec().file);
        if file.is_file() {
            std::fs::remove_file(&file)
                .map_err(|error| format!("could not remove {}: {error}", file.display()))?;
        }
    }
    Ok(())
}

/// Every picture model in the registry, for a test that keeps the two lists
/// saying the same thing.
pub fn registry_kinds() -> Vec<&'static str> {
    MODELS
        .iter()
        .filter(|spec| spec.id.kind() == "picture")
        .map(|spec| spec.file)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_is_every_picture_model_in_the_registry() {
        let mut from_set: Vec<&str> = VISUAL_MODELS.iter().map(|id| id.spec().file).collect();
        let mut from_registry = registry_kinds();
        from_set.sort_unstable();
        from_registry.sort_unstable();
        assert_eq!(from_set, from_registry);
    }

    #[test]
    fn every_file_in_the_set_has_a_digest_to_check_it_against() {
        for id in VISUAL_MODELS {
            let spec = id.spec();
            assert_eq!(spec.sha256.len(), 64, "{} has no digest", spec.file);
            assert!(spec.bytes > 0, "{} has no size", spec.file);
            assert!(spec.upstream.starts_with("https://"), "{}", spec.file);
        }
    }

    #[test]
    fn a_partial_set_is_not_ready() {
        let dir = std::env::temp_dir().join(format!("concat-visualmodel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(folder(&dir)).unwrap();
        assert!(!ready(&dir));
        // A file of the right size is the whole test the row needs: the
        // digest is checked when it arrives, not every time it is read.
        let id = ModelId::Dinov2;
        let path = folder(&dir).join(id.spec().file);
        std::fs::write(&path, vec![0u8; id.spec().bytes as usize]).unwrap();
        assert!(installed(&dir, id));
        assert!(!ready(&dir));
        assert_eq!(missing(&dir).len(), 3);
        remove(&dir).unwrap();
        assert!(!installed(&dir, id));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
