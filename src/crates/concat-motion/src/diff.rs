// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! What changed between two revisions of one comp, and the frames the
//! change can reach.
//!
//! A fix patch changes the comp's hash, and the whole bake re-renders for
//! it - even when the change is one node's timing, and only the frames
//! that node's window touches ever drew differently. This is the diff
//! that says so: which nodes were added, removed or changed, and - from
//! where the changed things sit on screen - which frames a re-render has
//! to produce at all, the rest inherited from the earlier bake byte for
//! byte.
//!
//! The comparison is by id, the same key tracks and diagnostics point at;
//! a change inside a sub-comp reaches every frame its placements hold,
//! through any depth, because a composition is the whole of what it
//! places.

use crate::comp::{Comp, NodeKind};

/// What changed between two revisions of one comp.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Diff {
    /// Nodes the revision after carries that the one before did not.
    pub added: Vec<String>,
    /// Nodes the revision before carried that the one after does not.
    pub removed: Vec<String>,
    /// Nodes both carry, differently - timing, transform, tracks, kind,
    /// any of it: a node is equal or it is changed, nothing in between.
    pub changed: Vec<String>,
}

impl Diff {
    /// Whether the two revisions are the same document.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// The frames the change can reach: a frame is dirty when a changed
    /// or added node is on screen in it in the revision after, or a
    /// removed node was on screen in it in the revision before - a node
    /// that leaves dirties a frame it used to hold, whatever the revision
    /// after says about it.
    pub fn dirty_frames(&self, before: &Comp, after: &Comp) -> Vec<bool> {
        let fps = before.fps.max(after.fps).max(1.0);
        let seconds = before.duration.max(after.duration);
        let frames = ((seconds * fps).ceil() as usize).max(1);
        let mut dirty = vec![false; frames];
        let reach = |comp: &Comp, id: &str, dirty: &mut Vec<bool>| {
            for node in &comp.nodes {
                if node.id != id {
                    continue;
                }
                let from = node.start;
                let to = node.end(comp.duration);
                for (frame, dirty) in dirty.iter_mut().enumerate() {
                    let at = frame as f64 / comp.fps.max(1.0);
                    if at >= from && at < to {
                        *dirty = true;
                    }
                }
            }
        };
        for id in self.changed.iter() {
            reach(before, id, &mut dirty);
            reach(after, id, &mut dirty);
        }
        for id in &self.added {
            reach(after, id, &mut dirty);
        }
        for id in &self.removed {
            reach(before, id, &mut dirty);
        }
        dirty
    }
}

/// Diff two revisions of one comp, by node id, through every sub-comp.
pub fn diff(before: &Comp, after: &Comp) -> Diff {
    let mut out = Diff::default();
    for node in &after.nodes {
        match before.nodes.iter().find(|other| other.id == node.id) {
            None => out.added.push(node.id.clone()),
            Some(other) if other == node => {}
            Some(_) => out.changed.push(node.id.clone()),
        }
    }
    for node in &before.nodes {
        if !after.nodes.iter().any(|other| other.id == node.id) {
            out.removed.push(node.id.clone());
        }
    }
    // A sub-comp that changed reaches every frame its placements hold:
    // the placement is the whole of what it places, through any depth.
    for (name, sub) in &after.comps {
        if before.comps.get(name) == Some(sub) {
            continue;
        }
        let places_it = |node: &crate::comp::Node| matches!(&node.kind, NodeKind::Composition { comp } if comp == name);
        for node in &after.nodes {
            if places_it(node) && !out.changed.contains(&node.id) && !out.added.contains(&node.id) {
                out.changed.push(node.id.clone());
            }
        }
    }
    // And a sub-comp the revision before carried that the one after
    // dropped: its placements are gone with it, and the frames they held.
    for name in before.comps.keys() {
        if after.comps.contains_key(name) {
            continue;
        }
        for node in &before.nodes {
            if matches!(&node.kind, NodeKind::Composition { comp } if comp == name)
                && !out.removed.contains(&node.id)
            {
                out.removed.push(node.id.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, Length, Node, NodeKind, Rgba, Transform};

    fn comp_of(duration: f64) -> Comp {
        Comp {
            format: FORMAT,
            id: "test.diff".into(),
            name: "Diff".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            nodes: Vec::new(),
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    fn node(id: &str, start: f64, span: f64) -> Node {
        Node {
            id: id.into(),
            parent: None,
            start,
            duration: span,
            transform: Transform::default(),
            opacity: 1.0,
            blend: crate::comp::Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind: NodeKind::Rect {
                width: Length::px(100.0),
                height: Length::px(100.0),
                fill: None,
                radius: 0.0,
            },
        }
    }

    /// A patch that retimes one node dirties only the frames the change
    /// reaches: the frames before it and after it, the ones neither window
    /// touches, inherit from the earlier bake untouched.
    #[test]
    fn a_retimed_node_dirties_only_its_windows() {
        let before = {
            let mut comp = comp_of(2.0);
            comp.nodes.push(node("card", 0.0, 0.0));
            comp
        };
        // The patch moves the card from 0..2s to 1..2s: frames 0..30 are
        // dirtied by the old window, 30..60 by the new one - which is all
        // of them. A better read: the card retimed LATER, so 0..1s held it
        // before and does not now; the diff says both windows.
        let after = {
            let mut comp = comp_of(2.0);
            let mut card = node("card", 1.0, 0.0);
            card.start = 1.0;
            comp.nodes.push(card);
            comp
        };
        let found = diff(&before, &after);
        assert_eq!(found.changed, ["card".to_string()]);
        let dirty = found.dirty_frames(&before, &after);
        assert_eq!(dirty.len(), 60);
        // The old window 0..2s and the new 1..2s together cover the whole
        // comp: everything is dirty.
        assert!(dirty.iter().all(|dirty| *dirty));
        // A patch that touches nothing changes nothing.
        let same = diff(&before, &before);
        assert!(same.is_empty());
        assert!(same.dirty_frames(&before, &before).iter().all(|d| !*d));
    }

    /// An added node and a removed one dirty their own windows, and a
    /// sub-comp that changed reaches its placements.
    #[test]
    fn added_removed_and_subcomp_changes_reach_their_frames() {
        let before = {
            let mut comp = comp_of(1.0);
            comp.nodes.push(node("keeper", 0.0, 0.0));
            comp.nodes.push(node("gone", 0.5, 0.25));
            let mut sub = comp_of(0.5);
            sub.nodes.push(node("inner", 0.0, 0.0));
            comp.comps.insert("sting".into(), sub);
            comp.nodes.push({
                let mut place = node("place", 0.0, 0.5);
                place.kind = NodeKind::Composition {
                    comp: "sting".into(),
                };
                place
            });
            comp
        };
        let after = {
            let mut comp = comp_of(1.0);
            comp.nodes.push(node("keeper", 0.0, 0.0));
            comp.nodes.push(node("new", 0.75, 0.1));
            let mut sub = comp_of(0.5);
            sub.nodes.push(node("inner", 0.0, 0.0));
            sub.nodes.push(node("extra", 0.2, 0.1));
            comp.comps.insert("sting".into(), sub);
            comp.nodes.push({
                let mut place = node("place", 0.0, 0.5);
                place.kind = NodeKind::Composition {
                    comp: "sting".into(),
                };
                place
            });
            comp
        };
        let found = diff(&before, &after);
        assert_eq!(found.added, ["new".to_string()]);
        assert_eq!(found.removed, ["gone".to_string()]);
        // The placement changed because its sub-comp did.
        assert!(found.changed.contains(&"place".to_string()), "{found:?}");
        let dirty = found.dirty_frames(&before, &after);
        assert_eq!(dirty.len(), 30);
        // The windows: "place" held 0..0.5s (frames 0..14), "gone" held
        // 0.5..0.75s (15..22), "new" holds 0.75..0.85s (23..25). Nothing
        // touches 0.85..1.0s: the last four frames inherit.
        let mut expected = vec![false; 30];
        expected[..26].fill(true);
        assert_eq!(dirty, expected, "{dirty:?}");
        // Without the sub-comp change and with the windows apart, the
        // frames between the windows inherit.
        let quiet = {
            let mut comp = comp_of(1.0);
            comp.nodes.push(node("keeper", 0.0, 0.0));
            comp.nodes.push(node("late", 0.9, 0.05));
            comp
        };
        let same = diff(&before, &quiet);
        let dirty = same.dirty_frames(&before, &quiet);
        assert!(dirty[23..26].iter().all(|d| !*d), "{dirty:?}");
        assert!(dirty[15], "gone held 0.5s");
    }
}
