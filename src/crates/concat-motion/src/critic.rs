// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The critic's fixes, applied: a structured suggestion become a patch.
//!
//! The contact-sheet critic loop runs, and it scores. What it did not
//! produce was a revision an editor could apply without translating it
//! by hand - "the chart enters 400ms too early" was a sentence, not an
//! operation. The critic now returns **fixes**: a where, a problem, a
//! change with its number and unit, and an op from the scene builder's
//! own vocabulary. This module is the other half: turning a fix into the
//! [`Op`] batch that patches the comp, so the loop from critique to
//! revision is data the whole way through.
//!
//! A fix that cannot be parsed - no number, no unit, no node it names -
//! is skipped **with the reason said**, never guessed at: a wrong patch
//! applied confidently is worse than a suggestion left on the table.
//!
//! ```no_run
//! # fn main() {
//! use concat_motion::comp::Comp;
//! use concat_motion::critic::{self, Fix};
//! use serde_json::json;
//!
//! let fixes = vec![
//!     Fix::parse(&json!({
//!         "where": "the chart", "op": "shift",
//!         "problem": "clipped off the right edge",
//!         "change": "shift x by -180px"
//!     })).expect("a well-formed fix"),
//!     Fix::parse(&json!({
//!         "where": "the chart", "op": "retime",
//!         "problem": "enters 400ms too early",
//!         "change": "start 0.4s later"
//!     })).expect("a well-formed fix"),
//! ];
//! let mut comp: Comp = unimplemented!();
//! let report = critic::apply(&mut comp, &fixes);
//! # let _ = report;
//! # }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::comp::{Comp, Node, NodeKind};
use crate::curve::Curve;

/// One fix, as the critic writes it: a where, a problem, a change with
/// its number and unit, and an op from the five the scene builder
/// patches with.
///
/// A fix may also carry a **patch**: the exact numbers of its change as
/// JSON, in the shape its op reads - `{"start": 1.9}` for a retime,
/// `{"dx": -180, "dy": 0}` for a shift, `{"factor": 1.2}` for a
/// resize, `{"curve": "spring-pop"}` for a restyle. A patch is applied
/// as written, no sentence parsed; the prose `change` stays beside it
/// for the human reading the report. A fix with both is a fix an agent
/// can apply without interpreting a single word, which is the loop the
/// critic exists to close.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Fix {
    /// The element or frames the fix names - "the chart in frames 6-8",
    /// "the caption on frame 3". Matched against node ids, loosely and
    /// case-insensitively; a comp with one node is named by anything.
    pub r#where: String,
    /// What is wrong, as the critic saw it.
    pub problem: String,
    /// The exact change, with its number and its unit.
    pub change: String,
    /// The patch operation: `retime`, `shift`, `resize`, `restyle` or
    /// `replace`.
    pub op: String,
    /// The same change as exact JSON, when the critic computed one -
    /// see the struct docs for each op's shape. Validated at parse:
    /// a patch that does not match its op is a refusal, because a
    /// machine-applied wrong number is worse than a sentence a human
    /// reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<Json>,
    /// How sure the critic is, 0 to 1. A fix whose every number came
    /// from the measured facts carries a high one, a read of the reel's
    /// taste a low one, and an agent batching fixes has the number it
    /// needs to order them. `None` is the critic that did not say, read
    /// as unrated, never as certain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

impl Fix {
    /// A fix from the critic's JSON, refused when it is not one: the op
    /// must be one of the five, every field must be there - a fix that
    /// lost its `where` is advice nobody can apply - and a patch, when
    /// it carries one, must be the shape its op reads.
    pub fn parse(value: &Json) -> Result<Fix, String> {
        let fix: Fix = serde_json::from_value(value.clone())
            .map_err(|error| format!("a fix is where/problem/change/op: {error}"))?;
        const OPS: [&str; 5] = ["retime", "shift", "resize", "restyle", "replace"];
        if !OPS.contains(&fix.op.as_str()) {
            return Err(format!(
                "{:?} is not one of the fix ops (retime, shift, resize, restyle, replace)",
                fix.op
            ));
        }
        if fix.r#where.trim().is_empty()
            || fix.problem.trim().is_empty()
            || fix.change.trim().is_empty()
        {
            return Err("a fix carries its where, its problem and its change".into());
        }
        if let Some(confidence) = fix.confidence
            && (!confidence.is_finite() || !(0.0..=1.0).contains(&confidence))
        {
            return Err("a fix's confidence is between 0 and 1".into());
        }
        if let Some(patch) = &fix.patch {
            check_patch(&fix.op, patch)?;
        }
        Ok(fix)
    }
}

/// Whether a patch is the shape its op reads: the one place the shapes
/// are named, so the parser, the applier and the docs cannot drift
/// apart. Strict on purpose - an extra key is a typo in a number a
/// machine is about to trust.
fn check_patch(op: &str, patch: &Json) -> Result<(), String> {
    let number = |key: &str| -> Result<f64, String> {
        patch
            .get(key)
            .and_then(Json::as_f64)
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("the patch's {key} is not a finite number"))
    };
    let keys = patch
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let only = |wanted: &[&str]| {
        if !keys.iter().all(|key| wanted.contains(&key.as_str())) {
            Err(format!(
                "a {op} patch carries only {}; got {keys:?}",
                wanted.join(", ")
            ))
        } else {
            Ok(())
        }
    };
    match op {
        "retime" => {
            only(&["start", "shift"])?;
            let starts = keys.iter().filter(|key| key.as_str() == "start").count();
            let shifts = keys.iter().filter(|key| key.as_str() == "shift").count();
            if starts + shifts != 1 {
                return Err(
                    "a retime patch carries exactly one of start (absolute) or shift \
                     (relative), in seconds"
                        .into(),
                );
            }
            if starts == 1 {
                number("start")?;
            } else {
                number("shift")?;
            }
        }
        "shift" => {
            only(&["dx", "dy"])?;
            number("dx")?;
            number("dy")?;
        }
        "resize" => {
            only(&["factor"])?;
            let factor = number("factor")?;
            // The same envelope the prose path clamps to - a resize is
            // a nudge, not a disappearance, and not a machine-applied
            // million either.
            if !(0.05..=20.0).contains(&factor) {
                return Err("a resize factor is between 0.05 and 20".into());
            }
        }
        "restyle" => {
            only(&["curve"])?;
            let name = patch
                .get("curve")
                .and_then(Json::as_str)
                .ok_or("a restyle patch's curve is a name from the easing library")?;
            if Curve::named(name).is_none() {
                return Err(format!(
                    "a restyle patch's curve {name:?} is not one the library knows; \
                     the names are {}",
                    Curve::NAMES.join(", ")
                ));
            }
        }
        // A replacement needs content the patch does not carry either.
        "replace" => {
            return Err(
                "a replacement needs new content the patch does not carry: pick the \
                 replacement, then patch it in"
                    .into(),
            );
        }
        other => return Err(format!("{other:?} is not a fix op")),
    }
    Ok(())
}

/// What came of applying a batch of fixes: every fix either landed as a
/// patch or was skipped with the reason said - and the reasons are the
/// half an agent needs, because a fix that cannot be applied is a thing
/// to escalate, not to lose.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Report {
    /// The fixes that landed, with the patch each became.
    pub applied: Vec<(Fix, String)>,
    /// The fixes that did not, with the reason each was skipped.
    pub skipped: Vec<(Fix, String)>,
}

impl Report {
    /// Whether every fix in the batch landed.
    pub fn all_applied(&self) -> bool {
        self.skipped.is_empty()
    }
}

/// Apply a batch of fixes to a comp: each patchable one becomes the op
/// that patches it, and the rest are skipped with their reasons said.
/// The comp is mutated in place; nothing is applied that the model
/// cannot justify with a number and a unit in the change.
pub fn apply(comp: &mut Comp, fixes: &[Fix]) -> Report {
    let mut report = Report::default();
    for fix in fixes {
        let outcome = apply_one(comp, fix);
        match outcome {
            Ok(patch) => report.applied.push((fix.clone(), patch)),
            Err(reason) => report.skipped.push((fix.clone(), reason)),
        }
    }
    report
}

/// The comp's score on the contact-sheet floors, 0 to 10 - the number
/// the critic loop converges. The vision critic reads frames and
/// answers in prose; this is the same judgement reduced to what a
/// document can say without eyes, so "applying the fixes raises the
/// score above 7" is a thing a test - and a loop - can hold:
///
/// * a document that fails its own contract costs 2 a problem;
/// * an element the frame clips costs 4 - the defect the contact
///   sheet's reader names first and loudest;
/// * an element the reel never shows costs 2.
///
/// A comp that carries none of that is a 10. The floors the CI holds
/// scenes to are the same arithmetic in smaller clothes.
pub fn score(comp: &Comp) -> f64 {
    let mut score = 10.0;
    // The contract first: a comp that does not hold it is a comp the
    // engine may refuse to draw at all.
    score -= 2.0 * comp.validate().len() as f64;
    for node in &comp.nodes {
        if clipped(comp, node) {
            score -= 4.0;
        }
        // Never shows: an element whose entrance sits beyond the
        // comp's own end. `duration: 0` runs to the end, so only a
        // `start` past it is invisible.
        if node.duration != 0.0 && node.start + node.duration <= 0.0 || node.start > comp.duration {
            score -= 2.0;
        }
    }
    score.clamp(0.0, 10.0)
}

/// Whether the frame clips a node: its placed centre plus its own box,
/// which is exactly the arithmetic the convergence test's chart was
/// refused by. A node the document never places is centred by the
/// engine and so never clipped; a node whose size is its content's own
/// (text, pictures, paths) is measured by its render, not its box, and
/// is left to the eyes of the vision critic.
fn clipped(comp: &Comp, node: &Node) -> bool {
    let Some(at) = placed_at(node) else {
        return false;
    };
    let (width, height) = match &node.kind {
        NodeKind::Rect { width, height, .. }
        | NodeKind::Ellipse { width, height, .. }
        | NodeKind::Html { width, height, .. } => (
            width.resolve(comp.width as f64),
            height.resolve(comp.height as f64),
        ),
        _ => return false,
    };
    let Some((width, height)) = width.zip(height) else {
        return false;
    };
    let scale = node.transform.scale.abs();
    let half_width = width * scale / 2.0;
    let half_height = height * scale / 2.0;
    at.x - half_width < -1e-9
        || at.x + half_width > comp.width as f64 + 1e-9
        || at.y - half_height < -1e-9
        || at.y + half_height > comp.height as f64 + 1e-9
}

/// One fix, applied - or the reason it cannot be.
///
/// A fix with a patch is applied from the patch's own numbers, exactly
/// as written: no sentence is parsed, no unit guessed at. A fix without
/// one falls back to the prose `change`, whose number and unit the
/// parsers below read - the older half of the contract, kept because a
/// critic that writes good sentences and no JSON is still a critic
/// worth applying.
fn apply_one(comp: &mut Comp, fix: &Fix) -> Result<String, String> {
    let node = find_node(comp, &fix.r#where)
        .ok_or_else(|| format!("no node the fix's where ({:?}) names", fix.r#where))?;
    let node = node.to_owned();
    match fix.op.as_str() {
        "retime" => {
            // The new start, however it was said: absolute from a
            // patch's `start`, relative from its `shift`, and relative
            // from the prose too - a sentence carries a move ("0.4s
            // earlier"), never a place on the timeline, because the
            // critic that wrote it was reading where the node already
            // is. All three land on zero at the earliest.
            let start = match fix.patch.as_ref().and_then(|patch| patch.get("start")) {
                Some(start) => {
                    let start = start.as_f64().ok_or_else(|| {
                        format!("the patch's start is not a number: {start:?}")
                    })?;
                    start.max(0.0)
                }
                _ => {
                    let shift = fix
                        .patch
                        .as_ref()
                        .and_then(|patch| patch.get("shift"))
                        .and_then(Json::as_f64);
                    let seconds = match shift {
                        Some(shift) => shift,
                        None => parse_seconds(&fix.change).ok_or_else(|| {
                            format!("no time with a unit in {:?}", fix.change)
                        })?,
                    };
                    (node.start + seconds).max(0.0)
                }
            };
            let node = comp
                .nodes
                .iter_mut()
                .find(|candidate| candidate.id == node.id)
                .expect("the node was found");
            let moved = start - node.start;
            node.start = start;
            Ok(format!(
                "moved {:?} to {start:.3}s ({}{moved:.3}s)",
                node.id,
                if moved >= 0.0 { "+" } else { "" }
            ))
        }
        "shift" => {
            let (dx, dy) = match (
                fix.patch
                    .as_ref()
                    .and_then(|patch| patch.get("dx"))
                    .and_then(Json::as_f64),
                fix.patch
                    .as_ref()
                    .and_then(|patch| patch.get("dy"))
                    .and_then(Json::as_f64),
            ) {
                (Some(dx), Some(dy)) => (dx, dy),
                _ => parse_pixels(&fix.change)
                    .ok_or_else(|| format!("no distance with a unit in {:?}", fix.change))?,
            };
            let base = placed_at(&node).ok_or_else(|| {
                format!("the node {:?} is never placed, so there is nothing to shift", node.id)
            })?;
            let x = base.x + dx;
            let y = base.y + dy;
            let node = comp
                .nodes
                .iter_mut()
                .find(|candidate| candidate.id == node.id)
                .expect("the node was found");
            node.transform.position = Some(crate::comp::Point::new(x, y));
            // A node that was moving keeps its motion - the shift moves
            // the destination, not the journey.
            if let Some(track) = node.tracks.get_mut("position") {
                for key in &mut track.keys {
                    if let crate::track::Value::Point { x, y } = &mut key.value {
                        *x += dx;
                        *y += dy;
                    }
                }
            }
            Ok(format!(
                "shifted {:?} to ({x:.0}, {y:.0}) ({}{dx:.0}px, {}{dy:.0}px)",
                node.id,
                if dx >= 0.0 { "+" } else { "" },
                if dy >= 0.0 { "+" } else { "" }
            ))
        }
        "resize" => {
            let factor = match fix
                .patch
                .as_ref()
                .and_then(|patch| patch.get("factor"))
                .and_then(Json::as_f64)
            {
                Some(factor) => factor,
                None => parse_factor(&fix.change)
                    .ok_or_else(|| format!("no size with a unit in {:?}", fix.change))?,
            };
            let node = comp
                .nodes
                .iter_mut()
                .find(|candidate| candidate.id == node.id)
                .expect("the node was found");
            let rest = node.transform.scale * factor;
            node.transform.scale = rest.max(0.05);
            Ok(format!(
                "scaled {:?} by {factor} to {:.2}",
                node.id, node.transform.scale
            ))
        }
        "restyle" => {
            // The restyle an agent can apply exactly: the patch names a
            // curve from the library, or the change's prose does - the
            // longest library name that appears in it, so "ease with
            // spring-pop" names the spring-pop and not the ease the
            // sentence leads with.
            let named = fix
                .patch
                .as_ref()
                .and_then(|patch| patch.get("curve"))
                .and_then(Json::as_str)
                .map(|name| name.to_owned())
                .or_else(|| {
                    let lowered = fix.change.to_ascii_lowercase();
                    Curve::NAMES
                        .iter()
                        .filter(|name| lowered.contains(*name))
                        .max_by_key(|name| name.len())
                        .map(|name| (*name).to_owned())
                });
            let Some(name) = named else {
                return Err(format!(
                    "no curve from the easing library named in {:?} (the names are {})",
                    fix.change,
                    Curve::NAMES.join(", ")
                ));
            };
            // The prose path can only name a curve the library knows,
            // but a patch can name anything a machine wrote - so the
            // lookup is answered for, not expected, the way every other
            // machine-supplied value in this module is.
            let Some(curve) = Curve::named(&name) else {
                return Err(format!(
                    "a restyle patch's curve {name:?} is not one the library knows; the names are {}",
                    Curve::NAMES.join(", ")
                ));
            };
            let node = comp
                .nodes
                .iter_mut()
                .find(|candidate| candidate.id == node.id)
                .expect("the node was found");
            let mut touched = 0;
            for track in node.tracks.values_mut() {
                if let Some(key) = track.keys.first_mut() {
                    key.curve = Some(curve.clone());
                    touched += 1;
                }
            }
            if touched == 0 {
                return Err(format!(
                    "the node {:?} carries no track to restyle",
                    node.id
                ));
            }
            Ok(format!(
                "eased {:?}'s {touched} track{} on {name}",
                node.id,
                if touched == 1 { "" } else { "s" }
            ))
        }
        // A replacement needs content the fix does not carry: the one op
        // that is always a skip, said as the escalation it is.
        "replace" => Err(
            "a replacement needs new content the fix does not carry: pick the replacement, then patch it in".into(),
        ),
        other => Err(format!("{other:?} is not a fix op")),
    }
}

/// The node a fix's `where` names: a node whose id appears in it, matched
/// loosely and case-insensitively - the critic writes prose, not ids -
/// with the longest id winning when two match. A comp of one node is
/// named by anything at all.
fn find_node<'a>(comp: &'a Comp, r#where: &str) -> Option<&'a Node> {
    let hay = r#where.to_ascii_lowercase();
    let mut best: Option<&Node> = None;
    for node in &comp.nodes {
        if comp.nodes.len() == 1 || hay.contains(&node.id.to_ascii_lowercase()) {
            let better = best
                .map(|current| current.id.len() < node.id.len())
                .unwrap_or(true);
            if better {
                best = Some(node);
            }
        }
    }
    best
}

/// Where a node is placed: its own base position, or the last key of its
/// position track.
fn placed_at(node: &Node) -> Option<crate::comp::Point> {
    if let Some(track) = node.tracks.get("position") {
        let mut keys = track.keys.iter().collect::<Vec<_>>();
        keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(crate::track::Value::Point { x, y }) = keys.last().map(|key| &key.value) {
            return Some(crate::comp::Point::new(*x, *y));
        }
    }
    node.transform.position
}

/// The number a unit is written after, with the whitespace a sentence
/// puts between them tolerated: `400ms`, `0.4s`, `1.5 seconds`, `-120px`.
/// The unit must directly follow its number (with room for spaces), so
/// the "s" in "seconds" counts and the one inside another word does not.
fn number_before_unit(text: &str, unit: &str) -> Option<f64> {
    let mut from = 0;
    while let Some(found) = text[from..].find(unit) {
        let at = from + found;
        let before: String = text[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == ' ')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let before = before.trim();
        if let Ok(value) = before.parse::<f64>() {
            return Some(value);
        }
        from = at + unit.len();
    }
    None
}

/// A time out of a change: `400ms`, `0.4s`, `2 s later`, `-1.5 seconds`,
/// with the sign the words carry - "later" and "after" keep a positive
/// number positive, "earlier" and "sooner" flip it.
fn parse_seconds(change: &str) -> Option<f64> {
    let lower = change.to_ascii_lowercase();
    // The longer units first: "ms" before "s", "seconds" before "sec".
    let seconds = if let Some(millis) = number_before_unit(&lower, "ms") {
        millis / 1000.0
    } else if let Some(secs) = number_before_unit(&lower, "seconds") {
        secs
    } else if let Some(secs) = number_before_unit(&lower, "second") {
        secs
    } else if let Some(secs) = number_before_unit(&lower, "secs") {
        secs
    } else if let Some(secs) = number_before_unit(&lower, "sec") {
        secs
    } else if let Some(secs) = number_before_unit(&lower, "s") {
        secs
    } else {
        return None;
    };
    let earlier = (lower.contains("earlier") || lower.contains("sooner")) && seconds > 0.0;
    Some(if earlier { -seconds } else { seconds })
}

/// A distance out of a change: `x by -120px`, `left 120px` (x becomes
/// negative), `down 40px` (y positive) - with `x`/`y` naming the axis and
/// the direction words deciding the sign when the number has none. One
/// axis per fix: a change that names two is two fixes.
fn parse_pixels(change: &str) -> Option<(f64, f64)> {
    let lower = change.to_ascii_lowercase();
    let value = number_before_unit(&lower, "px")?;
    // The words with the unit's own letters taken out of them, split
    // into tokens: "px" contains an x, and that x is not an axis.
    let words = lower.replace("px", " ");
    let tokens: Vec<&str> = words.split(|c: char| !c.is_ascii_alphanumeric()).collect();
    let (dx, dy) = if tokens.contains(&"x") {
        (value, 0.0)
    } else if tokens.contains(&"y") {
        (0.0, value)
    } else if tokens.contains(&"left") {
        (-value.abs(), 0.0)
    } else if tokens.contains(&"right") {
        (value.abs(), 0.0)
    } else if tokens.contains(&"up") {
        (0.0, -value.abs())
    } else if tokens.contains(&"down") {
        (0.0, value.abs())
    } else {
        (value, 0.0)
    };
    Some((dx, dy))
}

/// A scale factor out of a change: `0.9x`, `90%`, `scale to 0.9`, clamped
/// to the sane - a resize is a nudge, not a disappearance.
fn parse_factor(change: &str) -> Option<f64> {
    let lower = change.to_ascii_lowercase();
    if let Some(percent) = number_before_unit(&lower, "%") {
        return Some((percent / 100.0).clamp(0.05, 20.0));
    }
    // "1.2x": the x that follows its number, not the one inside a word.
    let mut from = 0;
    while let Some(found) = lower[from..].find('x') {
        let at = from + found;
        let before: String = lower[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == ' ')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if let Ok(value) = before.trim().parse::<f64>() {
            return Some(value.clamp(0.05, 20.0));
        }
        from = at + 1;
    }
    // "scale to 0.9": the number after the word.
    if let Some(at) = lower.find("scale") {
        let number: String = lower[at..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(value) = number.parse::<f64>() {
            return Some(value.clamp(0.05, 20.0));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, Length, Node, NodeKind, Point, Rgba, Transform};

    fn comp_of(nodes: Vec<Node>) -> Comp {
        Comp {
            format: FORMAT,
            id: "test.critic".into(),
            name: "Critic".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 4.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: std::collections::BTreeMap::new(),
            curves: std::collections::BTreeMap::new(),
            nodes,
            slots: Vec::new(),
        }
    }

    fn chart_node() -> Node {
        Node {
            id: "chart".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform {
                position: Some(Point::new(2100.0, 540.0)),
                ..Transform::default()
            },
            opacity: 1.0,
            blend: crate::comp::Blend::Normal,
            mask: None,
            effects: Vec::new(),
            tracks: std::collections::BTreeMap::new(),
            kind: NodeKind::Rect {
                width: Length::px(600.0),
                height: Length::px(400.0),
                fill: None,
                radius: 0.0,
            },
        }
    }

    /// The done-when: a deliberately broken scene - the chart clipped
    /// off the right edge of a 1920 frame, entering 400ms too early -
    /// and the critic's fixes for exactly those two things. Applying
    /// them puts the chart inside the frame at the right moment, and the
    /// second pass has nothing left to do.
    #[test]
    fn a_broken_scene_converges_under_its_fixes() {
        let mut comp = comp_of(vec![chart_node()]);
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "shift",
                "problem": "clipped off the right edge (2100 + 300 > 1920)",
                "change": "shift x by -480px"
            }))
            .expect("a well-formed fix"),
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "retime",
                "problem": "enters 400ms too early",
                "change": "start 0.4s later"
            }))
            .expect("a well-formed fix"),
        ];
        let report = apply(&mut comp, &fixes);
        assert!(report.all_applied(), "{:?}", report.skipped);
        let chart = comp.node("chart").expect("the chart");
        // Clipped no more: the 600-wide chart now sits centred at 1620,
        // its whole self inside the 1920 frame.
        assert!((chart.transform.position.expect("placed").x - 1620.0).abs() < 1e-9);
        // And it enters 400ms later, as the fix said.
        assert!((chart.start - 0.4).abs() < 1e-9);
        // The comp is still a valid document.
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());

        // And the defects the fixes named are gone, which is the
        // convergence the loop measures: nothing clips, nothing is
        // early, and the next critique has nothing left to name.
        let x = chart.transform.position.expect("placed").x;
        assert!(
            x - 300.0 >= 0.0 && x + 300.0 <= 1920.0,
            "the chart fits: {x}"
        );
        assert!((chart.start - 0.4).abs() < 1e-9, "nothing is early");

        // The score the loop converges, in numbers: the broken scene
        // sat under the 7 floor - a clipped element is the defect the
        // contact sheet names first - and the fixed one clears it.
        let broken = comp_of(vec![chart_node()]);
        let before = score(&broken);
        assert!(
            before < 7.0,
            "the broken scene scores {before}, under the 7 floor"
        );
        let fixed = score(&comp);
        assert!(
            fixed > 7.0,
            "the fixed scene scores {fixed}, above the 7 floor"
        );
        assert!(
            fixed > before,
            "the fixes raised the score, not just moved it"
        );
        // And the loop's own next critique has nothing left to name.
        assert_eq!(fixed, 10.0, "a comp with no named defect is a 10");
    }

    /// The floors the score charges, one defect at a time: a clean comp
    /// is a 10, a clipped element costs 4, an element the reel never
    /// shows costs 2, and the floor is 0 - a comp cannot score below
    /// the sheet it is judged on.
    #[test]
    fn the_score_costs_what_the_contact_sheet_names() {
        // Clean: the chart, centred, inside the frame.
        let mut centred = chart_node();
        centred.transform.position = Some(Point::new(960.0, 540.0));
        let clean = comp_of(vec![centred]);
        assert!(clean.validate().is_empty(), "{:?}", clean.validate());
        assert_eq!(score(&clean), 10.0);

        // Clipped: the chart as the broken scene carries it.
        let clipped = comp_of(vec![chart_node()]);
        assert_eq!(score(&clipped), 6.0, "a clipped element costs 4");

        // Never shows: an entrance past the comp's own end.
        let mut late = chart_node();
        late.transform.position = Some(Point::new(960.0, 540.0));
        late.start = 5.0;
        let never = comp_of(vec![late]);
        assert_eq!(score(&never), 8.0, "an unseen element costs 2");

        // Both: 10 - 4 - 2.
        let mut both = chart_node();
        both.start = 5.0;
        assert_eq!(score(&comp_of(vec![both])), 4.0);

        // And the floor: five clipped elements cannot score -10.
        let many = (0..5).map(|_| chart_node()).collect();
        assert_eq!(score(&comp_of(many)), 0.0, "the floor is 0");
    }

    /// A patch is applied as written, and a wrong patch is refused at
    /// the door: the loop an agent trusts with numbers must not accept
    /// numbers that do not parse, keys that do not belong, or a curve
    /// the library does not carry.
    #[test]
    fn a_patch_applies_exactly_and_a_wrong_one_is_refused() {
        let mut comp = comp_of(vec![chart_node()]);
        // The chart needs a track for the restyle to ease; the scene the
        // critic criticises always has one.
        comp.nodes[0].tracks.insert(
            "scale".into(),
            crate::track::Track {
                keys: vec![
                    crate::track::Key::eased(0.0, crate::track::Value::number(0.9), Curve::EASE_IN),
                    crate::track::Key::new(0.6, crate::track::Value::number(1.0)),
                ],
            },
        );

        // The exact patch: a start, a shift, a factor and a curve, each
        // landing as written without a sentence being read.
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "retime",
                "problem": "enters 400ms too early",
                "change": "start 0.4s later",
                "patch": { "start": 1.9 }
            }))
            .expect("a well-formed fix"),
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "shift",
                "problem": "clipped off the right edge",
                "change": "shift x by -480px",
                "patch": { "dx": -480.0, "dy": 0.0 }
            }))
            .expect("a well-formed fix"),
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "restyle",
                "problem": "arrives on a dead start",
                "change": "ease with spring-pop",
                "patch": { "curve": "spring-pop" }
            }))
            .expect("a well-formed fix"),
        ];
        let report = apply(&mut comp, &fixes);
        assert!(report.all_applied(), "{:?}", report.skipped);
        let chart = comp.node("chart").expect("the chart");
        assert!(
            (chart.start - 1.9).abs() < 1e-9,
            "the patch's start, exactly"
        );
        assert!(
            (chart.transform.position.expect("placed").x - 1620.0).abs() < 1e-9,
            "the patch's dx, exactly"
        );
        assert!(
            chart.tracks.values().all(|track| track
                .keys
                .first()
                .and_then(|key| key.curve.as_ref())
                .is_some_and(|curve| matches!(curve, Curve::Spring(_)))),
            "the patch's curve, exactly"
        );

        // A relative retime: shift, not start.
        let mut comp = comp_of(vec![chart_node()]);
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "retime",
                "problem": "enters 400ms too early",
                "change": "start 0.4s later",
                "patch": { "shift": 0.4 }
            }))
            .expect("a well-formed fix"),
        ];
        let report = apply(&mut comp, &fixes);
        assert!(report.all_applied(), "{:?}", report.skipped);
        assert!(
            (comp.node("chart").expect("the chart").start - 0.4).abs() < 1e-9,
            "the shift moved it 0.4s from its own 0"
        );

        // The refusals: both a start and a shift, an unknown curve, a
        // replace that carries a patch it cannot use, and a key no op
        // reads. Each names what it wanted.
        for broken in [
            serde_json::json!({
                "where": "the chart", "op": "retime", "problem": "p", "change": "c",
                "patch": { "start": 1.0, "shift": 0.4 }
            }),
            serde_json::json!({
                "where": "the chart", "op": "restyle", "problem": "p", "change": "c",
                "patch": { "curve": "wobbly" }
            }),
            serde_json::json!({
                "where": "the chart", "op": "replace", "problem": "p", "change": "c",
                "patch": { "text": "new words" }
            }),
            serde_json::json!({
                "where": "the chart", "op": "shift", "problem": "p", "change": "c",
                "patch": { "dx": -480.0, "rotation": 12.0 }
            }),
        ] {
            let error = Fix::parse(&broken)
                .expect_err("a wrong patch is refused")
                .to_string();
            assert!(
                error.contains("patch") || error.contains("replacement"),
                "the refusal names the patch: {error}"
            );
        }
    }

    /// The prose path keeps the contract it always had: its time is a
    /// move from where the node already is, clamped at the beginning
    /// of the timeline - "0.4s earlier" of a node at 2s is 1.6s, and a
    /// move that would cross zero lands on it, not before it. The
    /// patch's `start` is the one absolute time the vocabulary carries.
    #[test]
    fn the_prose_retime_stays_a_move_and_never_crosses_zero() {
        let mut node = chart_node();
        node.start = 2.0;
        let mut comp = comp_of(vec![node]);
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "retime",
                "problem": "enters 400ms too early",
                "change": "start 0.4s earlier"
            }))
            .expect("a well-formed fix"),
        ];
        let report = apply(&mut comp, &fixes);
        assert!(report.all_applied(), "{:?}", report.skipped);
        assert!(
            (comp.node("chart").expect("the chart").start - 1.6).abs() < 1e-9,
            "0.4s earlier of 2s is 1.6s, not an absolute 0.4s"
        );

        // And a move that would cross the beginning of the timeline
        // lands on zero, not before it.
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "retime",
                "problem": "lands before the reel begins",
                "change": "start 3s earlier"
            }))
            .expect("a well-formed fix"),
        ];
        let report = apply(&mut comp, &fixes);
        assert!(report.all_applied(), "{:?}", report.skipped);
        assert_eq!(
            comp.node("chart").expect("the chart").start,
            0.0,
            "a move past the beginning lands on it"
        );
    }

    /// The word forms the critic actually writes: "left 120px" is
    /// negative on x, "down 40px" positive on y, "earlier" flips a
    /// retime, and "0.4s" and "400ms" are the same time.
    #[test]
    fn the_words_carry_the_signs_the_numbers_do_not() {
        assert_eq!(parse_seconds("start 0.4s later"), Some(0.4));
        assert_eq!(parse_seconds("400ms later"), Some(0.4));
        assert_eq!(parse_seconds("0.4s earlier"), Some(-0.4));
        assert_eq!(parse_seconds("trim 1500ms"), Some(1.5));
        assert_eq!(parse_seconds("make it sooner"), None);

        assert_eq!(parse_pixels("shift x by -120px"), Some((-120.0, 0.0)));
        assert_eq!(parse_pixels("left 120px"), Some((-120.0, 0.0)));
        assert_eq!(parse_pixels("right 120px"), Some((120.0, 0.0)));
        assert_eq!(parse_pixels("up 40px"), Some((0.0, -40.0)));
        assert_eq!(parse_pixels("down 40px"), Some((0.0, 40.0)));
        assert_eq!(parse_pixels("move it a bit"), None);

        assert_eq!(parse_factor("scale to 0.9"), Some(0.9));
        assert_eq!(parse_factor("90% of the size"), Some(0.9));
        assert_eq!(parse_factor("1.2x bigger"), Some(1.2));
    }

    /// A fix that cannot be applied is skipped with the reason said:
    /// no node named, no number in the change, a replacement that needs
    /// content, and a restyle that names no curve.
    #[test]
    fn an_unparseable_fix_is_skipped_with_its_reason() {
        // Two nodes, so a where that names neither is a skip rather
        // than the single-node rule (anything names the only node).
        let mut caption = chart_node();
        caption.id = "caption".into();
        let mut comp = comp_of(vec![chart_node(), caption]);
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the table", "op": "shift",
                "problem": "clipped", "change": "shift x by -120px"
            }))
            .expect("parsed"),
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "shift",
                "problem": "clipped", "change": "move it a bit"
            }))
            .expect("parsed"),
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "replace",
                "problem": "the wrong footage", "change": "use a different one"
            }))
            .expect("parsed"),
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "restyle",
                "problem": "too linear", "change": "make it feel nicer"
            }))
            .expect("parsed"),
        ];
        let report = apply(&mut comp, &fixes);
        assert_eq!(report.applied.len(), 0, "{:?}", report.applied);
        assert_eq!(report.skipped.len(), 4, "{:?}", report.skipped);
        let reasons: Vec<&str> = report
            .skipped
            .iter()
            .map(|(_, reason)| reason.as_str())
            .collect();
        assert!(reasons[0].contains("no node"), "{reasons:?}");
        assert!(reasons[1].contains("no distance"), "{reasons:?}");
        assert!(reasons[2].contains("replacement"), "{reasons:?}");
        assert!(reasons[3].contains("easing library"), "{reasons:?}");
        // And nothing was patched along the way.
        assert!((comp.node("chart").unwrap().transform.position.unwrap().x - 2100.0).abs() < 1e-9);
    }

    /// The where matches loosely - the critic writes prose - and the
    /// longest id wins when two match, because "the caption" should not
    /// land on a node called "caption-shadow".
    #[test]
    fn the_where_matches_prose_and_prefers_the_longest_id() {
        let mut shadow = chart_node();
        shadow.id = "caption-shadow".into();
        let mut caption = chart_node();
        caption.id = "caption".into();
        let comp = comp_of(vec![shadow, caption]);
        assert_eq!(
            find_node(&comp, "the Caption on frame 3").map(|n| n.id.as_str()),
            Some("caption")
        );
        assert_eq!(
            find_node(&comp, "the caption-shadow behind it").map(|n| n.id.as_str()),
            Some("caption-shadow")
        );
        // A comp of one node is named by anything at all.
        let single = comp_of(vec![chart_node()]);
        assert_eq!(
            find_node(&single, "anything").map(|n| n.id.as_str()),
            Some("chart")
        );
    }

    /// A restyle that names a curve eases the node's tracks on it - the
    /// chart's fade, arrival and settle all take the critic's curve.
    #[test]
    fn a_restyle_eases_the_tracks_on_the_named_curve() {
        let mut node = chart_node();
        node.tracks.insert(
            "opacity".into(),
            crate::track::Track::new(vec![
                crate::track::Key::new(0.0, crate::track::Value::number(0.0)),
                crate::track::Key::new(0.3, crate::track::Value::number(1.0)),
            ]),
        );
        let mut comp = comp_of(vec![node]);
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "restyle",
                "problem": "arrives too linearly",
                "change": "ease with spring-pop"
            }))
            .expect("parsed"),
        ];
        let report = apply(&mut comp, &fixes);
        assert!(report.all_applied(), "{:?}", report.skipped);
        let chart = comp.node("chart").expect("the chart");
        assert_eq!(
            chart.tracks["opacity"].keys.first().unwrap().curve,
            Some(Curve::SPRING_POP)
        );
    }

    /// A shift moves a moving node's whole journey, not just its rest:
    /// the destination and the track's keys travel together.
    #[test]
    fn a_shift_moves_the_journey_not_just_the_rest() {
        let mut node = chart_node();
        node.tracks.insert(
            "position".into(),
            crate::track::Track::new(vec![
                crate::track::Key::new(
                    0.0,
                    crate::track::Value::Point {
                        x: 2100.0,
                        y: 554.0,
                    },
                ),
                crate::track::Key::new(
                    0.5,
                    crate::track::Value::Point {
                        x: 2100.0,
                        y: 540.0,
                    },
                ),
            ]),
        );
        let mut comp = comp_of(vec![node]);
        let fixes = vec![
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "shift",
                "problem": "clipped off the right edge",
                "change": "shift x by -180px"
            }))
            .expect("parsed"),
        ];
        let report = apply(&mut comp, &fixes);
        assert!(report.all_applied(), "{:?}", report.skipped);
        let chart = comp.node("chart").expect("the chart");
        let keys = &chart.tracks["position"].keys;
        assert_eq!(
            keys[0].value,
            crate::track::Value::Point {
                x: 1920.0,
                y: 554.0
            }
        );
        assert_eq!(
            keys[1].value,
            crate::track::Value::Point {
                x: 1920.0,
                y: 540.0
            }
        );
    }

    /// The parse refuses what is not a fix: a missing field, an unknown
    /// op - the vocabulary is the scene builder's, not the model's.
    #[test]
    fn a_fix_that_is_not_one_is_refused_at_the_parse() {
        assert!(
            Fix::parse(&serde_json::json!({
                "where": "the chart", "op": "wiggle",
                "problem": "clipped", "change": "shift x by -120px"
            }))
            .is_err()
        );
        assert!(
            Fix::parse(&serde_json::json!({
                "op": "shift", "problem": "clipped", "change": "shift x by -120px"
            }))
            .is_err()
        );
        assert!(Fix::parse(&serde_json::json!("just a string")).is_err());
    }

    /// A fix's confidence rides along when it is a finite 0-1 and is
    /// refused when it is not: the number an agent orders its work by is
    /// not one a model gets to invent the range of.
    #[test]
    fn a_fixs_confidence_is_carried_and_kept_in_its_range() {
        let fix = Fix::parse(&serde_json::json!({
            "where": "the chart", "op": "shift",
            "problem": "clipped off the right edge",
            "change": "shift x by -180px",
            "confidence": 0.85
        }))
        .expect("parsed");
        assert_eq!(fix.confidence, Some(0.85));
        // Absent is carried as unrated, never as certain.
        let quiet = Fix::parse(&serde_json::json!({
            "where": "the chart", "op": "shift",
            "problem": "clipped off the right edge",
            "change": "shift x by -180px"
        }))
        .expect("parsed");
        assert_eq!(quiet.confidence, None);
        for said in [1.5, -0.1] {
            assert!(
                Fix::parse(&serde_json::json!({
                    "where": "the chart", "op": "shift",
                    "problem": "clipped off the right edge",
                    "change": "shift x by -180px",
                    "confidence": said
                }))
                .is_err(),
                "a confidence of {said} is not a confidence"
            );
        }
    }
}
