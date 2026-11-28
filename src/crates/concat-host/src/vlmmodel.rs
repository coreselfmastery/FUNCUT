// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The local vision critic's two models: the GGUF a llama.cpp server reads
//! and the projector it pairs with, fetched into the same
//! `vlm/models/<profile>` folder `vlm.py`'s own setup reads - so a set
//! downloaded here is the set the critic scores with, without either side
//! being told about the other. The digests are the pins `vlm.py` has always
//! enforced, spent by the same fetch chain every other model takes: mirror,
//! upstream, mirror-of-upstream, each attempt resumable and each arrival
//! verified before it lands.
//!
//! One row per profile in the sheet, not one per file: a model and its
//! projector are what the critic spends together. The llama.cpp runtime the
//! server runs on is a few megabytes behind the same pin and stays with the
//! setup call that starts it - this table is the half a gigabyte that never
//! fit inside one bounded call.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// One file of a profile: its name in the profile's folder, the size the
/// row shows (the HF repository's own rounded figure - the fetch reports
/// real bytes as they arrive), and the sha256 pin the arrival must match.
struct FileSpec {
    file: &'static str,
    mb: u64,
    sha256: &'static str,
}

/// One critic profile: the HuggingFace repository both files live in and
/// the two files themselves. The ids are the spellings `local_vlm`'s
/// `profile` field takes, so the row a caller downloads is the profile the
/// critic starts.
pub struct Profile {
    /// The profile's id: `local_vlm {profile}` and `models.download`'s
    /// `id` under `kind: "vlm"`.
    pub id: &'static str,
    /// What the sheet's label says, the same words the settings rows use.
    pub label: &'static str,
    /// The repository both files resolve from.
    repo: &'static str,
    model: FileSpec,
    mmproj: FileSpec,
}

/// The set's id: the one row a whole-registry download spends, the default
/// profile every cold machine's critic starts from.
pub const SET_ID: &str = "vlm-critic";

/// Every profile, sheet order: the every-PC default first.
pub const PROFILES: [Profile; 3] = [
    Profile {
        id: "mini",
        label: "Vision critic (mini, 0.5B)",
        repo: "ggml-org/SmolVLM2-500M-Video-Instruct-GGUF",
        model: FileSpec {
            file: "SmolVLM2-500M-Video-Instruct-Q8_0.gguf",
            mb: 437,
            sha256: "6f67b8036b2469fcd71728702720c6b51aebd759b78137a8120733b4d66438bc",
        },
        mmproj: FileSpec {
            file: "mmproj-SmolVLM2-500M-Video-Instruct-Q8_0.gguf",
            mb: 109,
            sha256: "921dc7e259f308e5b027111fa185efcbf33db13f6e35749ddf7f5cdb60ef520b",
        },
    },
    Profile {
        id: "image",
        label: "Vision critic (image, 0.5B)",
        repo: "ggml-org/SmolVLM-500M-Instruct-GGUF",
        model: FileSpec {
            file: "SmolVLM-500M-Instruct-Q8_0.gguf",
            mb: 437,
            sha256: "9d4612de6a42214499e301494a3ecc2be0abdd9de44e663bda63f1152fad1bf4",
        },
        mmproj: FileSpec {
            file: "mmproj-SmolVLM-500M-Instruct-Q8_0.gguf",
            mb: 109,
            sha256: "d1eb8b6b23979205fdf63703ed10f788131a3f812c7b1f72e0119d5d81295150",
        },
    },
    Profile {
        id: "minicpm",
        label: "Vision critic (MiniCPM, 2.8B)",
        repo: "openbmb/MiniCPM-V-2-gguf",
        model: FileSpec {
            file: "ggml-model-Q2_K.gguf",
            mb: 1298,
            sha256: "d6c3584306a32ec56b6ca6cce21c05fd01db629ee4360fce665baaece15d2792",
        },
        mmproj: FileSpec {
            file: "mmproj-model-f16.gguf",
            mb: 867,
            sha256: "79611c59b5ad5b0547256602e3fb546a3041bcf6db5058091b6bcaa31f3a1c95",
        },
    },
];

/// The profile an id names, when it names one.
pub fn profile_by_id(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|profile| profile.id == id)
}

/// The profile a whole-registry download spends: the every-PC default.
pub fn default_profile() -> &'static Profile {
    &PROFILES[0]
}

/// Where a profile's files live: the app's own data folder, under the
/// `vlm/models/<profile>` path `vlm.py`'s setup reads - the folder contract
/// both sides share, so bytes that land here are bytes the critic finds.
pub fn folder(data: &Path, profile: &Profile) -> PathBuf {
    data.join("vlm").join("models").join(profile.id)
}

/// The bytes a full profile takes, for the row's size before anything is
/// fetched: the repository's own rounded figures, the same numbers the
/// setup call's reply has always carried.
pub fn bytes(profile: &Profile) -> u64 {
    (profile.model.mb + profile.mmproj.mb) * 1024 * 1024
}

/// Whether one file is on disk and more than a stub: the presence test
/// `vlm.py`'s own download makes, so a file this side calls installed is a
/// file that side walks past. The digest was checked when the file
/// arrived; re-reading half a gigabyte to re-prove it helps no one.
fn installed(data: &Path, profile: &Profile, file: &FileSpec) -> bool {
    folder(data, profile)
        .join(file.file)
        .metadata()
        .map(|meta| meta.len() > 1_000_000)
        .unwrap_or(false)
}

/// The bytes still to fetch.
pub fn missing_bytes(data: &Path, profile: &Profile) -> u64 {
    [&profile.model, &profile.mmproj]
        .into_iter()
        .filter(|file| !installed(data, profile, file))
        .map(|file| file.mb * 1024 * 1024)
        .sum()
}

/// Whether the critic will find a working pair.
pub fn ready(data: &Path, profile: &Profile) -> bool {
    installed(data, profile, &profile.model) && installed(data, profile, &profile.mmproj)
}

/// Fetch what is missing, the same walk the picture set takes: one file at
/// a time, each into its `.part` and renamed only after its digest proved
/// it, `progress` called with (received, total) across the pair.
pub fn download(
    data: &Path,
    profile: &Profile,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let wanted: Vec<&FileSpec> = [&profile.model, &profile.mmproj]
        .into_iter()
        .filter(|file| !installed(data, profile, file))
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let dir = folder(data, profile);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let total: u64 = wanted.iter().map(|file| file.mb * 1024 * 1024).sum();
    let mut done: u64 = 0;
    for file in wanted {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("vision critic models cancelled".to_owned());
        }
        let path = dir.join(file.file);
        let partial = path.with_extension("gguf.part");
        let base = done;
        progress(base, total);
        crate::models::fetch(
            file.file,
            &format!("https://huggingface.co/{}/resolve/main/{}", profile.repo, file.file),
            &partial,
            file.mb * 1024 * 1024,
            file.sha256,
            cancel,
            "vision critic models cancelled",
            &mut |received, _total| progress(base + received, total),
        )?;
        std::fs::rename(&partial, &path)
            .map_err(|error| format!("could not finish {}: {error}", path.display()))?;
        done += file.mb * 1024 * 1024;
        progress(done, total);
    }
    Ok(())
}

/// Remove a profile's files. A model the user fetched is theirs to delete,
/// and the critic falls back to the measured-pixel critique when it is
/// gone.
pub fn remove(data: &Path, profile: &Profile) -> Result<(), String> {
    for file in [&profile.model, &profile.mmproj] {
        let path = folder(data, profile).join(file.file);
        if path.is_file() {
            std::fs::remove_file(&path)
                .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_in_every_profile_carries_a_digest_and_a_source() {
        for profile in PROFILES {
            for file in [&profile.model, &profile.mmproj] {
                assert_eq!(file.sha256.len(), 64, "{} has no digest", file.file);
                assert!(file.mb > 0, "{} has no size", file.file);
                assert!(!profile.repo.is_empty(), "{} has no repository", file.file);
            }
        }
    }

    #[test]
    fn the_default_profile_is_the_every_pc_one() {
        assert_eq!(default_profile().id, "mini");
        assert_eq!(SET_ID, "vlm-critic");
    }

    #[test]
    fn the_folder_is_the_one_the_setup_reads() {
        let data = Path::new("/data");
        assert_eq!(
            folder(data, profile_by_id("image").unwrap()),
            Path::new("/data/vlm/models/image"),
            "vlm.py reads data_dir()/models/<profile>"
        );
    }

    #[test]
    fn a_partial_profile_is_not_ready_and_downloads_nothing_when_it_is() {
        let dir = std::env::temp_dir().join(format!("concat-vlmmodel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let profile = default_profile();
        assert!(!ready(&dir, profile));
        assert_eq!(missing_bytes(&dir, profile), bytes(profile));
        // A file over the stub line is present; a file under it is not.
        std::fs::create_dir_all(folder(&dir, profile)).unwrap();
        std::fs::write(
            folder(&dir, profile).join(profile.model.file),
            vec![0u8; 1_100_000],
        )
        .unwrap();
        assert!(installed(&dir, profile, &profile.model));
        assert!(!ready(&dir, profile));
        assert_eq!(missing_bytes(&dir, profile), profile.mmproj.mb * 1024 * 1024);
        remove(&dir, profile).unwrap();
        assert!(!installed(&dir, profile, &profile.model));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_download_is_done_without_touching_the_network() {
        let dir = std::env::temp_dir().join(format!("concat-vlmmodel-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let profile = default_profile();
        std::fs::create_dir_all(folder(&dir, profile)).unwrap();
        for file in [&profile.model, &profile.mmproj] {
            std::fs::write(
                folder(&dir, profile).join(file.file),
                vec![0u8; 1_100_000],
            )
            .unwrap();
        }
        assert!(ready(&dir, profile));
        let cancel = AtomicBool::new(false);
        // No sources to walk: the wanted list is empty, so this returns
        // without ever naming a URL.
        download(&dir, profile, &cancel, &mut |_, _| {}).expect("nothing to fetch");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
