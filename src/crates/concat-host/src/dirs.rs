// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Where the app keeps what belongs to this machine rather than to a
//! project: the recents list, remembered settings, downloaded models.
//!
//! Two directories, named by the app identifier and following the XDG
//! split: settings in `~/.config` and the models and caches in
//! `~/.local/share` (or wherever `XDG_CONFIG_HOME` and `XDG_DATA_HOME`
//! point).
//!
//! Or one directory, chosen by the person running the app: a folder named
//! `portable` beside the executable holds both, and nothing is written
//! anywhere else on the machine. That is what makes the zip a portable
//! build - unpack it on a stick, make the folder, and the settings, recents
//! and models travel with it and leave with it.

use std::path::{Path, PathBuf};

/// The app identifier, which is also the folder name everywhere.
pub const IDENTIFIER: &str = "app.concat.editor";

/// The app's own directories on this machine.
#[derive(Clone, Debug)]
pub struct AppDirs {
    /// Small state: recents, remembered settings, the template library.
    pub config: PathBuf,
    /// Large state: downloaded models.
    pub data: PathBuf,
}

impl AppDirs {
    /// The platform's directories for this app, created lazily by whoever
    /// writes into them - or the `portable` folder beside the executable,
    /// when there is one.
    pub fn locate() -> Result<AppDirs, String> {
        if let Some(portable) = portable_root() {
            return Ok(AppDirs::under(&portable));
        }
        // Linux follows the XDG split.
        let base = |variable: &str| {
            std::env::var_os(variable)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let config_base = match base("XDG_CONFIG_HOME") {
            Some(path) => path,
            None => home()?.join(".config"),
        };
        let data_base = match base("XDG_DATA_HOME") {
            Some(path) => path,
            None => home()?.join(".local").join("share"),
        };
        Ok(AppDirs {
            config: config_base.join(IDENTIFIER),
            data: data_base.join(IDENTIFIER),
        })
    }

    /// Both directories under one root. For tests, and for anyone running
    /// a portable build out of a folder.
    pub fn under(root: &Path) -> AppDirs {
        AppDirs {
            config: root.to_path_buf(),
            data: root.to_path_buf(),
        }
    }

    /// Where what the user made and cannot download again is kept - the
    /// Audio page's own sounds, the voices they cloned: `data`.
    pub fn library(&self) -> PathBuf {
        self.data.clone()
    }
}

/// The `portable` folder beside the executable, when someone has made one.
/// A folder rather than a marker file, because it *is* where everything
/// then goes, and its presence is the whole of the switch: no flag, no
/// setting, nothing to remember. The desktop's never has one unless a
/// person put it there.
fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let folder = exe.parent()?.join("portable");
    folder.is_dir().then_some(folder)
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "could not locate the home directory".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directories_are_named_by_the_identifier() {
        let dirs = AppDirs::locate().expect("locates");
        assert!(dirs.config.ends_with(IDENTIFIER));
        assert!(dirs.data.ends_with(IDENTIFIER));
        assert!(dirs.config.is_absolute());
    }
}
