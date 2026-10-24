// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The machine a run is on, as the first lines of its log say it.
//!
//! The questions every crash report starts with - which distribution,
//! which CPU, does it have AVX2, which build is this - each took a round trip
//! with the reporter (#313, #318). The log now answers them before it is
//! asked. Nothing here is sent anywhere; it is written into the run's own
//! log file like every other line.

/// One line about the build: the version, the target, and the commit CI
/// built it from - "local" for a build from a working tree.
pub fn build() -> String {
    format!(
        "Concat {} · {}-{} · commit {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        option_env!("GITHUB_SHA").map_or("local", |sha| &sha[..sha.len().min(12)]),
    )
}

/// One line about the system: its name and version.
pub fn system() -> String {
    os::describe().unwrap_or_else(|| std::env::consts::OS.to_owned())
}

/// One line about the CPU: its name where the system says it, how many
/// threads it runs, and which of the instruction sets the libraries Concat
/// links care about it has - the ones a build for a newer CPU than the
/// one it is on dies over.
pub fn cpu() -> String {
    let mut line = cpu_name().unwrap_or_else(|| std::env::consts::ARCH.to_owned());
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    line.push_str(&format!(" · {threads} threads"));
    let (has, lacks) = features();
    if !has.is_empty() {
        line.push_str(&format!(" · has {}", has.join(" ")));
    }
    if !lacks.is_empty() {
        line.push_str(&format!(" · lacks {}", lacks.join(" ")));
    }
    line
}

/// The instruction sets worth naming, split into those the CPU has and
/// those it does not.
fn features() -> (Vec<&'static str>, Vec<&'static str>) {
    #[allow(unused_mut)]
    let mut found: Vec<(&'static str, bool)> = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        found.extend([
            ("sse4.2", std::arch::is_x86_feature_detected!("sse4.2")),
            ("avx", std::arch::is_x86_feature_detected!("avx")),
            ("avx2", std::arch::is_x86_feature_detected!("avx2")),
            ("fma", std::arch::is_x86_feature_detected!("fma")),
            ("bmi2", std::arch::is_x86_feature_detected!("bmi2")),
            ("f16c", std::arch::is_x86_feature_detected!("f16c")),
            ("avx512f", std::arch::is_x86_feature_detected!("avx512f")),
        ]);
    }
    #[cfg(target_arch = "aarch64")]
    {
        found.extend([
            ("neon", std::arch::is_aarch64_feature_detected!("neon")),
            ("fp16", std::arch::is_aarch64_feature_detected!("fp16")),
            (
                "dotprod",
                std::arch::is_aarch64_feature_detected!("dotprod"),
            ),
            ("i8mm", std::arch::is_aarch64_feature_detected!("i8mm")),
            ("sve", std::arch::is_aarch64_feature_detected!("sve")),
        ]);
    }
    let has = found
        .iter()
        .filter(|(_, on)| *on)
        .map(|(n, _)| *n)
        .collect();
    let lacks = found
        .iter()
        .filter(|(_, on)| !*on)
        .map(|(n, _)| *n)
        .collect();
    (has, lacks)
}

/// The CPU's marketing name.
fn cpu_name() -> Option<String> {
    #[cfg(target_arch = "x86_64")]
    {
        x86_brand()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

/// The brand string CPUID leaves 0x80000002-4 spell out.
#[cfg(target_arch = "x86_64")]
fn x86_brand() -> Option<String> {
    use std::arch::x86_64::__cpuid;
    // SAFETY: CPUID is on every x86-64 CPU.
    let top = unsafe { __cpuid(0x8000_0000) }.eax;
    if top < 0x8000_0004 {
        return None;
    }
    let mut bytes = Vec::with_capacity(48);
    for leaf in 0x8000_0002u32..=0x8000_0004 {
        // SAFETY: as above, and the leaf is within the range just read.
        let r = unsafe { __cpuid(leaf) };
        for word in [r.eax, r.ebx, r.ecx, r.edx] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    let name = String::from_utf8_lossy(&bytes);
    let name = name.trim_matches(char::from(0)).trim();
    (!name.is_empty()).then(|| name.to_owned())
}

mod os {
    /// "Linux Mint 22.3 · kernel 7.0.0-10-generic".
    pub(super) fn describe() -> Option<String> {
        let release = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find_map(|line| line.strip_prefix("PRETTY_NAME="))
                    .map(|name| name.trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| "Linux".to_owned());
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
        Some(match kernel {
            Some(kernel) => format!("{release} · kernel {}", kernel.trim()),
            None => release,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_say_something() {
        assert!(build().starts_with(&format!("Concat {}", env!("CARGO_PKG_VERSION"))));
        assert!(!system().is_empty());
        assert!(cpu().contains("threads"));
    }
}
