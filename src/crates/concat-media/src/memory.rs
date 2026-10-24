// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! How much memory the machine has, for sizing the caches to it.
//!
//! One question, asked once: the frame cache is a share of it (see
//! [`ReaderPool::frame_budget`]), and the other caches that sit beside
//! that one - the stills the pool holds, the title paintings, the warm
//! readers - step down together on a small machine, where RAM shared
//! with the desktop and a browser is the difference between scrubbing
//! and swapping. `None` where the platform will not say, and the caches
//! fall back to the sizes they always had.
//!
//! The detection lives here, the lowest crate that sizes anything, so
//! every caller can ask: the window's scheduler, an export (whose
//! sought-clip pool is a whole second pool) and the command line each
//! hold caches of their own, and none of them should size those for a
//! workstation when the machine is not one.

/// The machine's physical memory, in bytes.
pub fn total() -> Option<u64> {
    static TOTAL: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *TOTAL.get_or_init(read)
}

/// The count of physical pages times their size, which the system answers
/// through `sysconf`.
fn read() -> Option<u64> {
    // SAFETY: `sysconf` reads a system constant and has no preconditions;
    // a negative answer means "not known" and is handled below.
    let (pages, size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    (pages > 0 && size > 0).then(|| pages as u64 * size as u64)
}

/// The memory at and under which a machine counts as small: six gigabytes,
/// the class of a budget laptop, whose RAM is shared with a whole desktop
/// and whatever else is open. Pure, so the step-downs that hang off it can
/// be tested without a machine; `None` never says small, and the caches
/// take their usual sizes.
pub fn small(total: Option<u64>) -> bool {
    const SIX_GIGABYTES: u64 = 6 * 1024 * 1024 * 1024;
    total.is_some_and(|total| total <= SIX_GIGABYTES)
}

/// Whether this machine is a small one; see [`small`].
pub fn small_machine() -> bool {
    small(total())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every machine the tests run on has some memory, and says how much.
    #[test]
    fn the_machine_says_how_much_memory_it_has() {
        let total = super::total().expect("this platform answers");
        assert!(total >= 512 * 1024 * 1024, "{total} bytes");
    }

    /// The step-down line sits where it is meant to, on both sides.
    #[test]
    fn small_is_the_budget_laptop_and_not_the_workstation() {
        assert!(small(Some(4 * 1024 * 1024 * 1024)));
        assert!(small(Some(6 * 1024 * 1024 * 1024)));
        assert!(!small(Some(8 * 1024 * 1024 * 1024)));
        assert!(!small(None));
    }
}
