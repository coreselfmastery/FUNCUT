// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The library's Motion page (#13): the project's motion comps, listed
//! and placed.
//!
//! A comp is a document in the project's `comps/`, its bake a PNG
//! sequence under `cache/comps/` keyed by the document's hash. The page
//! lists what is there - the name, the frame, the length, and whether a
//! bake is on disk to place - and Add puts a motion clip at the playhead
//! through the same undoable command path every other clip takes
//! (`Command::AddMotionClip`): it can be undone, it saves with the
//! project, it reopens with it, and the monitor and the export render it
//! from the bake. The command is the validator as well - a comp id that
//! is not a safe file name, or a digest that is not a cache key, is
//! refused there, with the reason on a toast, not silently.

use serde_json::Value;

use crate::studio::Studio;

/// Everything that can happen to the Motion page.
#[derive(Clone, Debug)]
pub enum MotionMsg {
    /// A library pane turned to the page or away from it: the listing
    /// refreshes on the way in, because comps come and go while the
    /// page is closed.
    Shown(bool),
    /// The Refresh button.
    Refresh,
    /// A row's Add: a motion clip at the playhead. `hash` is the bake
    /// key and `comp` the comp's id, both as the listing read them off
    /// the disk.
    Add { hash: String, comp: String },
}

/// One comp in the listing.
#[derive(Clone, Debug)]
pub struct CompRow {
    /// The comp's id, as its document names itself.
    pub id: String,
    /// What the row says: the document's name, or its id when it has
    /// none.
    pub name: String,
    /// The bake key: the file's stem, the cache directory's prefix.
    pub hash: String,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Length in seconds.
    pub duration: f64,
    /// A bake is on disk under the project's cache: what a motion clip
    /// placed from this row renders.
    pub baked: bool,
}

/// The Motion page's state.
#[derive(Default)]
pub struct MotionPane {
    /// Whether a library pane is showing the page.
    pub shown: bool,
    /// The project's comps, by name.
    pub comps: Vec<CompRow>,
}

impl MotionPane {
    pub fn update(&mut self, msg: MotionMsg, studio: &mut Studio) {
        match msg {
            MotionMsg::Shown(on) => {
                self.shown = on;
                if on {
                    self.refresh(studio);
                }
            }
            MotionMsg::Refresh => self.refresh(studio),
            MotionMsg::Add { hash, comp } => {
                let start = f64::from(studio.playhead);
                let created =
                    studio.apply(concat_project::Command::AddMotionClip { start, comp, hash });
                if let Some(id) = created {
                    studio.selection = vec![id];
                }
            }
        }
    }

    /// Reads the project's `comps/` and the bakes under its `cache/`:
    /// what the page lists. A comp whose document cannot be read is
    /// left out rather than listed as something it may not be.
    fn refresh(&mut self, studio: &Studio) {
        self.comps.clear();
        let Some(session) = studio.session.as_ref() else {
            return;
        };
        let project = std::path::Path::new(session.path());
        let Some(entries) = std::fs::read_dir(project.join("comps")).ok() else {
            return;
        };
        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            })
            .collect();
        files.sort();
        for file in files {
            let Some(row) = comp_row(project, &file) else {
                continue;
            };
            self.comps.push(row);
        }
        self.comps.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

/// One comp file as a listing row: the document's own facts, and whether
/// a bake is on disk for it. `None` for a file that is not a comp
/// document the page can speak for.
fn comp_row(project: &std::path::Path, file: &std::path::Path) -> Option<CompRow> {
    let text = std::fs::read_to_string(file).ok()?;
    let document: Value = serde_json::from_str(&text).ok()?;
    // The comp's own fields, as `concat-motion`'s document writes them;
    // read loose, because the document is the motion engine's to extend.
    let id = document.get("id")?.as_str()?.to_owned();
    let name = document
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&id)
        .to_owned();
    let width = document.get("width").and_then(Value::as_u64)? as u32;
    let height = document.get("height").and_then(Value::as_u64)? as u32;
    let duration = document
        .get("duration")
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .unwrap_or(1.0);
    let hash = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if hash.len() != 16 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        // Not a bake-keyed name: the motion tools write `<hash>.json`,
        // and a clip placed from any other file could not find its bake.
        return None;
    }
    Some(CompRow {
        baked: baked(project, &hash),
        id,
        name,
        hash,
        width,
        height,
        duration,
    })
}

/// Whether a bake for `hash` is under the project's cache: a directory
/// whose name starts with the hash and dashes on, holding a manifest.
/// The deep validation - the frames the manifest promises against the
/// PNGs on disk - is the export's, when a clip is actually rendered;
/// this is the listing's lighter question, asked every refresh.
fn baked(project: &std::path::Path, hash: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(project.join("cache").join("comps")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_dir()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.len() > hash.len()
                        && name[..hash.len()].eq_ignore_ascii_case(hash)
                        && name.as_bytes()[hash.len()] == b'-'
                })
            && path.join("manifest.json").is_file()
    })
}
