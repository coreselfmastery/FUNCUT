// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! How much memory the machine has, for sizing the caches to it.
//!
//! One question, asked once: the frame cache is a share of it (see
//! `concat_media::ReaderPool::frame_budget`). `None` where the platform
//! will not say, and the caches fall back to the sizes they always had.
//!
//! The reading itself lives one crate down, in `concat_media::memory`,
//! so the export and the command line - which hold pools of their own -
//! size them for the same machine the window does; this is the host's
//! thin handle on it, for the caches this crate holds: the title
//! paintings and the audio mappings.

/// The machine's physical memory, in bytes.
pub fn total() -> Option<u64> {
    concat_media::memory::total()
}

/// Whether a machine with `total_memory` bytes of RAM counts as a small
/// one - six gigabytes or less, the class of a budget laptop. The pure
/// half of [`small_machine`], for a cache that sizes itself for a
/// machine it was told about rather than the one it is on.
pub fn small_machine_for(total_memory: Option<u64>) -> bool {
    concat_media::memory::small(total_memory)
}

/// Whether this machine is a small one; see [`small_machine_for`]. The
/// caches here step down on one.
pub fn small_machine() -> bool {
    concat_media::memory::small_machine()
}

#[cfg(test)]
mod tests {
    /// Every machine the tests run on has some memory, and says how much.
    #[test]
    fn the_machine_says_how_much_memory_it_has() {
        let total = super::total().expect("this platform answers");
        assert!(total >= 512 * 1024 * 1024, "{total} bytes");
    }
}
