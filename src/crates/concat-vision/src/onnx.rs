// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Whether this machine can run ONNX Runtime at all.
//!
//! The runtime is linked into the app, and it runs wherever the app does.
//! This part does no inference and needs no runtime: Settings asks it
//! too, to say up front what the AI features need.

/// Why the AI features - background removal, the smart brushes, Enhance
/// and Chatterbox - cannot run on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable {
    /// What the machine is missing, as a sentence for the interface.
    pub needs: String,
}

/// Why the AI features cannot run here, or `None` when nothing is known
/// against them. Nothing is, on a machine the runtime runs on.
pub fn unavailable() -> Option<&'static Unavailable> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_known_against_the_runtime() {
        assert!(unavailable().is_none());
    }
}
