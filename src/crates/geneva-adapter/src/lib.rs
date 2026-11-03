// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Motion made elsewhere, read here.
//!
//! Two formats carry motion between tools, and neither of them is ours:
//! Lottie (the bodymovin JSON After Effects exports) is what a designer
//! hands over, and GSAP (the timeline a web build animates with) is what a
//! motion developer already knows how to write. [`concat_motion`] is the
//! format the engines bake. This crate is the honest bridge between them.
//!
//! Two adapters, one rule: **nothing converts silently**. A Lottie feature
//! the comp vocabulary has no answer for - a text layer, a matte, a trim
//! path - is skipped with a note that says what and where, an approximation
//! (a group paint split per shape, an elastic ease read as a spring) is
//! applied with a note that says what was done instead, and the notes ride
//! along beside the comp in an [`Import`] so the caller - an agent, most
//! likely - can read them to the human. The alternative, guessing and
//! calling the result a conversion, is how a logo comes back wrong and
//! nobody can say why.
//!
//! - [`lottie`] turns a bodymovin document into a comp: solids, images,
//!   shape layers (rectangles, ellipses, paths), grouped and nested
//!   transforms, keyframed properties with their bezier and hold eases,
//!   layer parenting and in/out points. Item 03 of the Geneva roadmap.
//! - [`gsap`] applies a GSAP-style timeline to a comp's nodes: tweens on
//!   `x`, `y`, `scale`, `rotation`, `opacity` and `blur`, with the ease
//!   families the named-curve map covers, `delay`, `stagger`, `repeat` and
//!   `yoyo` - written onto the nodes' tracks, keyed as fractions of each
//!   node's span the way every comp track is.
//!
//! What the adapters deliberately are not:
//!
//! - **Not a player.** A `.lottie` file plays as an asset through the
//!   Lottie engine already; the adapter is for when the motion should
//!   become *document* - editable, lintable, fillable by a slot, baked by
//!   whichever engine the project prefers.
//! - **Not lossless.** The report is the contract: if a note says
//!   something was skipped, the comp is smaller than its source, and the
//!   caller decides whether that is acceptable. A conversion with no notes
//!   converted everything the document asked for.
//! - **Not a second format.** The output is a [`concat_motion::comp::Comp`]
//!   that validates and lints by the same rules a hand-written one does -
//!   the adapters hold themselves to the library's own bar, and the tests
//!   hold them to it on every fixture.

pub mod gsap;
pub mod lottie;

use concat_motion::comp::Comp;

/// A conversion and its conscience.
///
/// The comp is the deliverable; the notes are what makes it honest. Each
/// note is one sentence a caller can read aloud, prefixed with what kind of
/// note it is:
///
/// - `skipped: …` - a feature of the source that has no counterpart in the
///   comp vocabulary, left out entirely (a Lottie text layer, a matte).
/// - `approximated: …` - a feature carried over by a stated approximation
///   rather than exactly (an elastic ease read as the library's spring).
/// - `clamped: …` - a value outside what the comp allows, brought inside
///   with the reason (a layer that starts before the comp does).
///
/// A conversion with no notes lost nothing the source document said.
#[derive(Clone, PartialEq, Debug)]
pub struct Import {
    /// The comp the source became.
    pub comp: Comp,
    /// What the conversion skipped, approximated or clamped, in the order
    /// the document raised it.
    pub notes: Vec<String>,
}

impl Import {
    /// A conversion that lost nothing.
    pub fn clean(comp: Comp) -> Import {
        Import {
            comp,
            notes: Vec::new(),
        }
    }

    /// A note, added to the report.
    pub fn note(&mut self, said: impl Into<String>) {
        self.notes.push(said.into());
    }

    /// How many notes report a skip rather than an approximation or a
    /// clamp - the number a caller most wants to know before trusting a
    /// conversion, because a skip means the comp draws less than the
    /// source did.
    pub fn skipped(&self) -> usize {
        self.notes
            .iter()
            .filter(|note| note.starts_with("skipped: "))
            .count()
    }
}

/// Why an adapter refused to convert at all.
///
/// Notes are for features; errors are for documents. A Lottie file with no
/// layers is not a conversion that lost something, it is nothing to
/// convert, and a GSAP timeline naming a node the comp does not have is not
/// an approximation, it is a typo - and both say so rather than producing
/// an empty comp and calling it done.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AdapterError {
    /// The document is not the format the adapter reads, and the field it
    /// could not do without names which one.
    #[error("not a {format} document: {said}")]
    NotTheFormat {
        /// Which format was expected.
        format: &'static str,
        /// What was missing or wrong.
        said: String,
    },
    /// The conversion ran, and the comp it built does not validate - which
    /// is a bug in the adapter, reported as such rather than shipped as
    /// a document that will refuse to bake.
    #[error("the adapter built a comp that does not validate: {problems:?}")]
    Invalid {
        /// The validation problems, as [`Comp::validate`] reported them.
        problems: Vec<String>,
    },
}

/// Read a JSON field as `f64`, taking a one-element array as its number -
/// bodymovin wraps single values in arrays more often than not, and a
/// reader that cannot do both is a reader that breaks on real files.
pub(crate) fn number_or_first(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::Array(items) => items.first().and_then(number_or_first),
        _ => None,
    }
}

/// Read a JSON field as an array of `f64`, accepting a bare number as a
/// one-element array - the mirror of [`number_or_first`], for the fields
/// that are usually vectors but sometimes are not.
pub(crate) fn numbers(value: &serde_json::Value) -> Vec<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64().into_iter().collect(),
        serde_json::Value::Array(items) => items.iter().filter_map(number_or_first).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_kept_in_the_order_it_was_raised() {
        let mut import = Import::clean(fixtures::empty_comp());
        import.note("skipped: the text layer");
        import.note("approximated: the elastic ease");
        assert_eq!(import.notes.len(), 2);
        assert_eq!(import.skipped(), 1);
    }

    #[test]
    fn an_empty_report_skipped_nothing() {
        let import = Import::clean(fixtures::empty_comp());
        assert_eq!(import.skipped(), 0);
        assert!(import.notes.is_empty());
    }

    /// The smallest comp that validates: one frame, one second, nothing in
    /// it. Every fixture starts from a real document so the tests assert
    /// against what the adapters actually build.
    pub(crate) mod fixtures {
        use concat_motion::comp::{Comp, Rgba};
        use std::collections::BTreeMap;

        pub fn empty_comp() -> Comp {
            Comp {
                format: 1,
                id: "fixture".into(),
                name: "Fixture".into(),
                width: 320,
                height: 180,
                fps: 30.0,
                duration: 1.0,
                background: Rgba::new(0.0, 0.0, 0.0, 0.0),
                assets: BTreeMap::new(),
                curves: BTreeMap::new(),
                nodes: Vec::new(),
                comps: BTreeMap::new(),
                slots: Vec::new(),
            }
        }
    }
}
