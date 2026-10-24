// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! What Settings > About reports about this machine and this build.

use crate::i18n::t;

pub fn os_description() -> String {
    // The distribution's own name for itself, which is what a report
    // wants; the kernel version is the next question, not the first.
    let pretty = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|release| {
            release
                .lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
                .map(|value| value.trim_matches('"').to_owned())
        })
        .filter(|value| !value.is_empty());
    pretty.unwrap_or_else(|| "Linux".into())
}

/// What Settings > About shows under "System information", and what its copy
/// button puts on the clipboard.
///
/// One list, read twice: the rows on screen and the block that is copied are
/// built from the same pairs, so a fact cannot be on the page and missing
/// from the report. Gathered once — every line of it is fixed for the life of
/// the process — and handed over as a model that is never replaced.
pub fn system_facts() -> Vec<(String, String)> {
    let mut facts = vec![
        (
            t("sysinfo.application"),
            format!("Concat {}", env!("CARGO_PKG_VERSION")),
        ),
        (
            t("sysinfo.build"),
            format!("{} · {}", env!("BUILD_PROFILE"), env!("BUILD_TARGET")),
        ),
        // The renderer, the first question to ask about anything that looks
        // wrong on screen. One now, but a bug report outlives the build it
        // came from, and an older one may have drawn with FemtoVG.
        (t("sysinfo.renderer"), "Skia".into()),
        // The adapter the window and the monitor draw on, and whether it is
        // a GPU at all. A machine that fell back to WARP looks like any
        // other in the window, and the difference was the whole story of
        // https://github.com/jub0t/Concat/issues/135
        (
            t("sysinfo.graphics"),
            crate::gpu::adapter_description().unwrap_or("none").into(),
        ),
        (
            t("sysinfo.engine"),
            format!("concat-engine · FFmpeg {}", concat_media::linked_version()),
        ),
        (t("sysinfo.operatingSystem"), os_description()),
        (
            t("sysinfo.processor"),
            format!(
                "{} · {} threads",
                std::env::consts::ARCH,
                std::thread::available_parallelism().map_or(0, |count| count.get())
            ),
        ),
        (t("sysinfo.toolchain"), env!("BUILD_RUSTC").into()),
    ];
    facts
}
