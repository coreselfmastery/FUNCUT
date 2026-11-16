// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The Rive importer: honest about what a `.riv` is. A Rive document is a
//! state machine over a vector scene, and the runtime that plays it is not
//! in this tree - so the import does not pretend: it documents the file
//! (its name, its bytes) and produces a comp that draws nothing but says
//! why, the same reserved-row honesty the Lottie engine carries.
//!
//! What a caller gets is a valid, empty comp - one invisible group, so the
//! document validates - plus the notes. The day a runtime lands, this is
//! the file it replaces.

use concat_motion::comp::{Comp, FORMAT, Node, NodeKind, Rgba, Transform};

use crate::{AdapterError, Import};

/// The character-class slug the other adapters share.
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for (i, ch) in name.char_indices() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if i > 0 && !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

/// Read a `.riv` document into a comp: the comp draws nothing, and the
/// notes say why, in the same words every reserved row uses.
pub fn import(bytes: &[u8], stem: &str) -> Result<Import, AdapterError> {
    let not_rive = |said: &str| AdapterError::NotTheFormat {
        format: "Rive",
        said: said.into(),
    };
    if bytes.is_empty() {
        return Err(not_rive("the file is empty"));
    }
    // Rive files are flatbuffer containers; the magic is the container's
    // own, and a file that does not start as one is not a `.riv` we read.
    if !bytes.starts_with(b"RIVE") {
        return Err(not_rive(
            "the file does not carry the Rive container's magic",
        ));
    }
    let name = slug(stem);
    let id = format!(
        "rive.{}",
        if name.is_empty() {
            "document"
        } else {
            name.as_str()
        }
    );
    let mut import = Import::clean(Comp {
        format: FORMAT,
        id: id.clone(),
        name: "Rive document".into(),
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration: 3.0,
        background: Rgba::new(0.0, 0.0, 0.0, 0.0),
        assets: Default::default(),
        curves: Default::default(),
        nodes: vec![Node {
            id: "board".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform::default(),
            opacity: 0.0,
            blend: concat_motion::comp::Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind: NodeKind::Group {
                children: Vec::new(),
            },
        }],
        comps: Default::default(),
        slots: Vec::new(),
    });
    import.note(format!(
        "kept: {id:?} is a Rive state machine ({} bytes); no runtime ships in this tree, \
         so the import documents it and the comp draws nothing",
        bytes.len()
    ));
    import.note(
        "skipped: the artboards, the state machines and the bones have no counterpart here; \
         the file is kept whole for the day a runtime lands",
    );
    Ok(import)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.riv` with the container's magic imports as an honest nothing:
    /// one invisible group, the notes saying why.
    #[test]
    fn a_rive_document_imports_as_an_honest_nothing() {
        let made = import(b"RIVE\x00\x01\x02state-machine-bytes", "Player").expect("converts");
        assert_eq!(made.comp.id, "rive.player");
        assert_eq!(made.comp.nodes.len(), 1);
        assert_eq!(made.comp.nodes[0].opacity, 0.0);
        assert_eq!(made.comp.nodes[0].id, "board");
        assert!(
            made.comp.validate().is_empty(),
            "{:?}",
            made.comp.validate()
        );
        assert!(
            made.notes
                .iter()
                .any(|note| note.contains("no runtime ships")),
            "{:?}",
            made.notes
        );
    }

    /// A file without the magic is refused, and so is an empty one.
    #[test]
    fn a_non_rive_file_is_refused() {
        assert!(import(b"", "empty").is_err());
        assert!(import(b"not a rive", "liar").is_err());
    }
}
