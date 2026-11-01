// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The pulse: audio-driven keyframes, baked into the document.
//!
//! The engine computes every frame from its timestamp, so a keyframe is
//! just a value at a time - and the values at times are exactly what a
//! beat grid, an SFX transient and an emotion read already are. This
//! module is the arithmetic between them: a beat lands, and the chart
//! grows on it; a transient lands, and the caption pulses on it; the
//! act changes, and the easing curve changes with it.
//!
//! The beats themselves are measured elsewhere - `suggest_cuts` reads
//! them from the soundtrack, the recogniser times the words, the sounds
//! library carries each sound's `syncAt`. What was missing was the bake:
//! turning `[0.98, 1.94, 2.91]` into keyframes a comp carries, so the
//! video has a pulse without a single line of JavaScript.
//!
//! ```text
//! beats [0.98, 1.94, 2.91] + scale 0.05, release 0.32
//!   -> keys [(0.98, 1.05) spring-pop, (1.30, 1.0),
//!            (1.94, 1.05) spring-pop, (2.26, 1.0),
//!            (2.91, 1.05) spring-pop, (3.23, 1.0)]
//! ```

use serde::{Deserialize, Serialize};

use crate::comp::{Comp, Node, Point};
use crate::curve::Curve;
use crate::route::BeatType;
use crate::track::{Key, Track, Value};

/// The beat grid a pulse is driven by: the times, in seconds from the
/// scene's own start, that the audio lands its accents. Measured, not
/// guessed - `suggest_cuts` reads them from the soundtrack.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct BeatGrid {
    /// The beat times, in seconds, in order (a grid out of order is
    /// sorted where it is read; nothing here trusts the caller's order).
    pub beats: Vec<f64>,
}

impl BeatGrid {
    /// A grid from measured times.
    pub fn new(beats: Vec<f64>) -> BeatGrid {
        BeatGrid { beats }
    }
}

/// What a pulse drives.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Driven {
    /// The node's scale: `0.05` grows five percent on the beat and
    /// settles - the chart that grows on the downbeat.
    Scale,
    /// The node's opacity: `0.08` brightens on the transient - the
    /// caption that pulses when the sound lands.
    Opacity,
    /// The node's position: `1.0` pops up by 30 px at 1080p, scaled to
    /// the comp's own height - the same pop is the same fraction of the
    /// frame at 4K and at 540p - the whole card, taking the beat.
    Position,
}

/// One pulse: a beat grid, what it drives, and how the bump decays.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Pulse {
    /// The grid, as measured times in seconds.
    pub beats: Vec<f64>,
    /// What the beat drives, and by how much.
    pub driven: Driven,
    /// The bump's size: a scale delta, an opacity delta, or - for a
    /// position pop - px at 1080p, scaled to the comp's own height.
    pub amount: f64,
    /// How long the bump takes to settle, in seconds - the tail that
    /// makes a hit read as a hit rather than a flicker.
    pub release: f64,
    /// How the bump decays. The library's spring-pop is the drum read;
    /// `linear` is the mechanical one.
    pub curve: Curve,
    /// The nodes the pulse drives, by id; empty is every node the comp
    /// carries - a scene-wide pulse is usually what a beat grid means.
    pub nodes: Vec<String>,
}

impl Default for Pulse {
    fn default() -> Self {
        Pulse {
            beats: Vec::new(),
            driven: Driven::Scale,
            amount: 0.05,
            release: 0.32,
            curve: Curve::SPRING_POP,
            nodes: Vec::new(),
        }
    }
}

impl Pulse {
    /// A scale pulse - the chart that grows on the downbeat.
    pub fn scale(beats: Vec<f64>, amount: f64) -> Pulse {
        Pulse {
            beats,
            driven: Driven::Scale,
            amount,
            ..Pulse::default()
        }
    }

    /// An opacity pulse - the caption that pulses on the SFX transient.
    pub fn opacity(beats: Vec<f64>, amount: f64) -> Pulse {
        Pulse {
            beats,
            driven: Driven::Opacity,
            amount,
            ..Pulse::default()
        }
    }
}

/// The curve an act's motion travels - the emotion read's own answer to
/// "how should this feel", as one line of the easing library. Calm for
/// the hook, wound for the tension, punchy for the payoff.
///
/// | Act | Curve | The read |
/// | --- | --- | --- |
/// | hook | `ease-out` | calm, confident - read before it is watched |
/// | setup | `emphasis` | the settle the presets share |
/// | tension | `ease-in` | winding up |
/// | turn | `ease-in-out` | the shift travels through |
/// | reveal | `spring-pop` | the payoff lands and overshoots once |
/// | proof | `emphasis` | steady, the receipts |
/// | cta | `spring-pop` | the ask pops |
pub fn curve_for_act(beat: BeatType) -> Curve {
    match beat {
        BeatType::Hook => Curve::EASE_OUT,
        BeatType::Setup => Curve::EMPHASIS,
        BeatType::Tension => Curve::EASE_IN,
        BeatType::Turn => Curve::EASE_IN_OUT,
        BeatType::Reveal => Curve::SPRING_POP,
        BeatType::Proof => Curve::EMPHASIS,
        BeatType::Cta => Curve::SPRING_POP,
    }
}

/// Where a sound must start for its peak to land on the moment it is
/// for: the sounds library carries each sound's `syncAt` - seconds in
/// where its transient peaks - and an entrance aligned to the *peak*
/// is the entrance minus the sync, not the entrance itself. A sound
/// with no measured peak starts exactly on the moment.
pub fn sound_starts_at(moment: f64, sync_at: Option<f64>) -> f64 {
    (moment - sync_at.unwrap_or(0.0).max(0.0)).max(0.0)
}

/// Bake a pulse into a comp: every beat the comp's nodes carry as a bump
/// and a settle, on the driven property, merged with whatever the nodes
/// already animate there. The beats are read in the scene's own clock
/// and spent in each node's own fractions, so a comp pulsed here keeps
/// its pulse wherever it is placed.
///
/// Returns how many of the grid's beats landed inside at least one
/// node's own window. A grid that lands none - the signature of a
/// project-time grid fed to a scene that reads its own clock - is the
/// caller's to say, not this bake's to swallow.
pub(crate) fn apply(comp: &mut Comp, pulse: &Pulse) -> usize {
    if pulse.beats.is_empty() || pulse.amount <= 0.0 {
        return 0;
    }
    // The grid in order, whatever order it arrived in: a track whose
    // keys do not strictly increase is refused by the engine.
    let mut beats = pulse.beats.clone();
    beats.retain(|beat: &f64| beat.is_finite() && *beat >= 0.0);
    beats.sort_by(|a: &f64, b: &f64| a.total_cmp(b));
    if beats.is_empty() {
        return 0;
    }
    // Whether each beat reaches any node at all, so a grid that lands
    // nowhere can be named rather than silently dropped.
    let mut reached = vec![false; beats.len()];
    // A position pop is sized by the frame it happens in: the 30 px a
    // unit of `amount` moves at 1080p is scaled to the comp's own
    // height, so the same pulse pops the same fraction of the frame at
    // 4K and at 540p - a fixed pixel count would shrink on the big frame
    // and shout on the small one.
    let pop_unit = 30.0 * f64::from(comp.height) / 1080.0;
    for node in &mut comp.nodes {
        if !pulse.nodes.is_empty() && !pulse.nodes.contains(&node.id) {
            continue;
        }
        let span = if node.duration > 0.0 {
            node.duration
        } else {
            (comp.duration - node.start).max(0.0)
        };
        if span <= 1e-9 {
            continue;
        }
        for (index, beat) in beats.iter().enumerate() {
            let at = (beat - node.start) / span;
            if (0.0..=1.0).contains(&at) {
                reached[index] = true;
            }
        }
        match pulse.driven {
            Driven::Scale => bump_number(
                node,
                &beats,
                span,
                "scale",
                node.transform.scale,
                pulse.amount,
                pulse,
            ),
            Driven::Opacity => bump_number(
                node,
                &beats,
                span,
                "opacity",
                node.opacity,
                pulse.amount,
                pulse,
            ),
            Driven::Position => {
                // A position pulse rides around the node's own place; a
                // node with an authored position track keeps what it
                // wrote, because two position opinions is one too many.
                if node.tracks.contains_key("position") {
                    continue;
                }
                if let Some(base) = node.transform.position {
                    let pop = Point::new(base.x, base.y - pulse.amount * pop_unit);
                    bump_point(node, &beats, span, base, pop, pulse);
                }
            }
        }
    }
    reached.into_iter().filter(|landed| *landed).count()
}

/// A numeric bump on a property: every beat a key up by `amount` (the
/// curve the pulse names) and a key back at the rest, merged with the
/// track's own keys - the author's fade and the beat's pulse are both
/// kept, sorted by time, the beat winning its own moment.
///
/// The rest each beat bumps from is the authored motion's own value at
/// that moment, sampled from the track as it stands - a pulse keyed to
/// the node's static rest would fight an authored zoom or fade rather
/// than ride it: a chart growing through an authored zoom would shrink
/// on the beat instead, a caption fading out would flash bright on it.
/// The settle returns to the authored value at its own moment the same
/// way, so the beat is a bump *on* the motion and not a jump back to
/// where the motion started.
fn bump_number(
    node: &mut Node,
    beats: &[f64],
    span: f64,
    property: &str,
    rest: f64,
    amount: f64,
    pulse: &Pulse,
) {
    let existing = node.tracks.get(property);
    let local = |at: f64| -> f64 {
        existing
            .and_then(|track| track.sample(at))
            .and_then(|value| value.as_number())
            .filter(|value| value.is_finite())
            .unwrap_or(rest)
    };
    let mut keys: Vec<Key> = Vec::new();
    for beat in beats {
        let at = (beat - node.start) / span;
        if !(0.0..=1.0).contains(&at) {
            // A beat outside the node's own window does not reach it.
            continue;
        }
        let from = local(at);
        let bump = match property {
            // Opacity is clamped to what a frame can carry: a node already
            // at full opacity brightens nowhere, so its pulse dips - a
            // blink reads as a pulse reads - and a dimmed node brightens.
            "opacity" => {
                let up = from + amount;
                if up <= 1.0 {
                    up
                } else {
                    (from - amount).max(0.0)
                }
            }
            _ => from + amount,
        };
        if (bump - from).abs() <= 1e-9 {
            continue;
        }
        keys.push(Key::eased(at, Value::number(bump), pulse.curve.clone()));
        // The settle lands inside the node even when the release runs
        // past its end: clamped to the node's last moment, the property
        // settles as far as it can rather than staying bumped for
        // whatever of the node is left.
        let back = at + pulse.release / span;
        keys.push(Key::new(back.min(1.0), Value::number(local(back))));
    }
    merge(node, property, keys, Value::number(rest));
}

/// A position pop: every beat a key at the popped place and a key back
/// at the node's own.
fn bump_point(node: &mut Node, beats: &[f64], span: f64, rest: Point, pop: Point, pulse: &Pulse) {
    let mut keys: Vec<Key> = Vec::new();
    for beat in beats {
        let at = (beat - node.start) / span;
        if !(0.0..=1.0).contains(&at) {
            continue;
        }
        keys.push(Key::eased(
            at,
            Value::Point { x: pop.x, y: pop.y },
            pulse.curve.clone(),
        ));
        // As in `bump_number`: a release that outruns the node settles
        // as far as it reaches, not never.
        let back = at + pulse.release / span;
        keys.push(Key::new(
            back.min(1.0),
            Value::Point {
                x: rest.x,
                y: rest.y,
            },
        ));
    }
    merge(
        node,
        "position",
        keys,
        Value::Point {
            x: rest.x,
            y: rest.y,
        },
    );
}

/// Merge pulse keys with a track's own: sorted by time, and a pulse key
/// that lands on an authored one wins the moment (a beat is a fact, a
/// keyframe an opinion). A track with no keys of its own begins at the
/// rest value, so the first bump has somewhere to come from.
fn merge(node: &mut Node, property: &str, keys: Vec<Key>, rest: Value) {
    if keys.is_empty() {
        return;
    }
    let existing = node.tracks.remove(property).unwrap_or_default();
    let mut all: Vec<Key> = existing.keys.clone();
    all.extend(keys);
    all.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    // The dedup that keeps the times strictly increasing: on a collision
    // the pulse's key - pushed after the authored one, so sitting after
    // it in the sort's stable order - wins the moment, and the authored
    // key is the one dropped. The track never carries two keys at one
    // time.
    let mut merged: Vec<Key> = Vec::with_capacity(all.len());
    for key in all {
        if let Some(last) = merged.last()
            && (key.at - last.at).abs() <= 1e-9
        {
            merged.pop();
        }
        merged.push(key);
    }
    // A track that starts mid-span needs its rest first: a key at 0.9
    // with nothing before it animates nothing until it lands. The rest
    // it needs is the one the track was already holding - an authored
    // track holds its own first value from the start, and the pulse
    // must not swap that for the node's static one.
    let hold = existing.sample(0.0).unwrap_or(rest);
    if merged.first().is_some_and(|first| first.at > 1e-9) {
        merged.insert(0, Key::new(0.0, hold));
    }
    node.tracks.insert(property.into(), Track::new(merged));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, NodeKind, Rgba};

    fn node_comp(node: Node) -> Comp {
        Comp {
            format: FORMAT,
            id: "test.pulse".into(),
            name: "Pulse".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 4.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: std::collections::BTreeMap::new(),
            curves: std::collections::BTreeMap::new(),
            nodes: vec![node],
            slots: Vec::new(),
        }
    }

    fn placed_node(id: &str) -> Node {
        Node {
            id: id.into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: crate::comp::Transform {
                position: Some(Point::new(960.0, 540.0)),
                ..Default::default()
            },
            opacity: 1.0,
            blend: crate::comp::Blend::Normal,
            mask: None,
            effects: Vec::new(),
            tracks: std::collections::BTreeMap::new(),
            kind: NodeKind::Rect {
                width: crate::comp::Length::px(200.0),
                height: crate::comp::Length::px(80.0),
                fill: None,
                radius: 0.0,
            },
        }
    }

    /// The done-when, chart half: a beat grid of three downbeats, and
    /// the chart grows on each - a bump at the beat, a settle after,
    /// every key strictly increasing, the comp validating clean.
    #[test]
    fn a_chart_grows_on_the_downbeat() {
        let mut comp = node_comp(placed_node("chart"));
        let pulse = Pulse::scale(vec![0.98, 1.94, 2.91], 0.05);
        apply(&mut comp, &pulse);
        let track = comp.nodes[0].tracks.get("scale").expect("the pulse landed");
        let keys = &track.keys;
        assert_eq!(keys.len(), 7, "{keys:?}"); // rest + 3 bumps + 3 settles
        assert_eq!(keys[0].at, 0.0);
        assert_eq!(keys[0].value, Value::number(1.0));
        assert!((keys[1].at - 0.98 / 4.0).abs() < 1e-9);
        assert_eq!(keys[1].value, Value::number(1.05));
        assert_eq!(keys[1].curve, Some(Curve::SPRING_POP));
        assert!((keys[2].at - (0.98 + 0.32) / 4.0).abs() < 1e-9);
        assert_eq!(keys[2].value, Value::number(1.0));
        // Strictly increasing, as the engine demands.
        for pair in keys.windows(2) {
            assert!(pair[0].at < pair[1].at, "{keys:?}");
        }
        assert!(comp.validate().is_empty(), "{:?}", comp.validate());
    }

    /// The done-when, caption half: the SFX transient times drive an
    /// opacity pulse, and the authored fade-in survives the merge.
    #[test]
    fn a_caption_pulses_on_the_sfx_transient_and_keeps_its_fade() {
        let mut node = placed_node("caption");
        node.tracks.insert(
            "opacity".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::new(0.03, Value::number(1.0)),
            ]),
        );
        let mut comp = node_comp(node);
        let pulse = Pulse::opacity(vec![1.2, 2.4], 0.08);
        apply(&mut comp, &pulse);
        let keys = &comp.nodes[0].tracks["opacity"].keys;
        // The authored fade survives whole...
        assert_eq!(keys[0].at, 0.0);
        assert_eq!(keys[0].value, Value::number(0.0));
        assert_eq!(keys[1].at, 0.03);
        assert_eq!(keys[1].value, Value::number(1.0));
        // ...and the transients land as bumps after it. A node already
        // at full opacity brightens nowhere, so its pulse dips - the
        // blink that reads as a pulse.
        assert!((keys[2].at - 1.2 / 4.0).abs() < 1e-9);
        assert_eq!(keys[2].value, Value::number(0.92));
        assert!((keys[3].at - (1.2 + 0.32) / 4.0).abs() < 1e-9, "{keys:?}");
        assert_eq!(keys[3].value, Value::number(1.0));
        for pair in keys.windows(2) {
            assert!(pair[0].at < pair[1].at, "{keys:?}");
        }
    }

    /// The bump rides the authored motion, not the static rest: a chart
    /// growing through an authored zoom grows *more* on the beat, rather
    /// than shrinking to what the node's untouched scale would say.
    #[test]
    fn a_beat_bumps_the_authored_motion_where_it_stands() {
        let mut node = placed_node("chart");
        node.tracks.insert(
            "scale".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(1.0)),
                Key::new(1.0, Value::number(1.4)),
            ]),
        );
        let mut comp = node_comp(node);
        // A beat at 2s of a 4s span: the authored zoom stands at 1.2
        // there, and the bump is 5 percent on top of that.
        apply(&mut comp, &Pulse::scale(vec![2.0], 0.05));
        let keys = &comp.nodes[0].tracks["scale"].keys;
        let bump = keys
            .iter()
            .find(|key| (key.at - 0.5).abs() < 1e-9)
            .expect("the beat's key");
        assert_eq!(bump.value, Value::number(1.25), "{keys:?}");
        // And the settle returns to the authored zoom's own value at its
        // moment - 1.232 at 0.58 - not to the 1.0 the node rests at.
        let settle = keys
            .iter()
            .find(|key| (key.at - 0.58).abs() < 1e-9)
            .expect("the settle");
        assert_eq!(settle.value, Value::number(1.232), "{keys:?}");
        for pair in keys.windows(2) {
            assert!(pair[0].at < pair[1].at, "{keys:?}");
        }
    }

    /// A dimmed caption brightens on the beat by the amount, from the
    /// dimness it is actually in - a pulse keyed to the node's static
    /// full opacity would have blinked it dark instead.
    #[test]
    fn a_dimmed_caption_brightens_from_where_it_is() {
        let mut node = placed_node("caption");
        node.tracks.insert(
            "opacity".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.5)),
                Key::new(1.0, Value::number(0.5)),
            ]),
        );
        let mut comp = node_comp(node);
        apply(&mut comp, &Pulse::opacity(vec![1.0], 0.08));
        let keys = &comp.nodes[0].tracks["opacity"].keys;
        let bump = keys
            .iter()
            .find(|key| (key.at - 0.25).abs() < 1e-9)
            .expect("the beat's key");
        assert_eq!(bump.value, Value::number(0.58), "{keys:?}");
    }

    /// An authored track that starts mid-span holds its own first value
    /// from the start; a pulse landing before it does not swap that hold
    /// for the node's static rest.
    #[test]
    fn an_authored_track_keeps_its_own_hold_before_a_pulse() {
        let mut node = placed_node("card");
        node.tracks.insert(
            "opacity".into(),
            Track::new(vec![Key::new(0.9, Value::number(0.4))]),
        );
        let mut comp = node_comp(node);
        // The beat at 1s lands at 0.25, before the authored 0.9 key.
        apply(&mut comp, &Pulse::opacity(vec![1.0], 0.08));
        let keys = &comp.nodes[0].tracks["opacity"].keys;
        assert_eq!(keys[0].at, 0.0, "{keys:?}");
        assert_eq!(keys[0].value, Value::number(0.4), "{keys:?}");
        // And the beat itself still bumps from the 0.4 the track holds.
        let bumped = keys[1].value.as_number().expect("a number");
        assert!((bumped - 0.48).abs() < 1e-9, "{keys:?}");
    }

    /// A beat outside a node's own window does not reach it; a pulse
    /// naming nodes drives only the nodes it names.
    #[test]
    fn beats_reach_only_the_nodes_and_windows_they_reach() {
        let mut comp = node_comp(placed_node("a"));
        let mut late = placed_node("b");
        late.start = 2.0;
        comp.nodes.push(late);
        let mut named = placed_node("c");
        named.id = "c".into();
        comp.nodes.push(named);
        let mut pulse = Pulse::scale(vec![0.5, 3.0], 0.05);
        pulse.nodes = vec!["a".into(), "b".into(), "c".into()];
        apply(&mut comp, &pulse);
        // Node a carries both beats.
        let a = comp.nodes[0].tracks.get("scale").expect("a pulses");
        assert_eq!(a.keys.len(), 5, "{:?}", a.keys);
        // Node b starts at 2s: the 0.5s beat is outside its window, and
        // only the 3.0s one lands.
        let b = comp.nodes[1].tracks.get("scale").expect("b pulses");
        assert_eq!(b.keys.len(), 3, "{:?}", b.keys);
        assert!(
            (b.keys[1].at - (3.0 - 2.0) / 2.0).abs() < 1e-9,
            "{:?}",
            b.keys
        );
        // And a pulse naming no nodes drives all of them.
        let mut comp = node_comp(placed_node("a"));
        comp.nodes.push(placed_node("b"));
        apply(&mut comp, &Pulse::scale(vec![1.0], 0.05));
        assert!(
            comp.nodes
                .iter()
                .all(|node| node.tracks.contains_key("scale"))
        );
    }

    /// The act chooses the easing curve: calm for the hook, punchy for
    /// the payoff - the emotion read's own answer, as one line of the
    /// easing library.
    #[test]
    fn the_act_chooses_the_curve() {
        assert_eq!(curve_for_act(BeatType::Hook), Curve::EASE_OUT);
        assert_eq!(curve_for_act(BeatType::Setup), Curve::EMPHASIS);
        assert_eq!(curve_for_act(BeatType::Tension), Curve::EASE_IN);
        assert_eq!(curve_for_act(BeatType::Turn), Curve::EASE_IN_OUT);
        assert_eq!(curve_for_act(BeatType::Reveal), Curve::SPRING_POP);
        assert_eq!(curve_for_act(BeatType::Proof), Curve::EMPHASIS);
        assert_eq!(curve_for_act(BeatType::Cta), Curve::SPRING_POP);
    }

    /// An SFX aligned to a layer entrance starts before the entrance by
    /// its own measured peak, so the peak lands on the moment.
    #[test]
    fn a_sound_starts_early_by_its_own_peak() {
        assert_eq!(sound_starts_at(2.0, Some(0.092)), 1.908);
        assert_eq!(sound_starts_at(2.0, None), 2.0);
        assert_eq!(
            sound_starts_at(0.05, Some(0.092)),
            0.0,
            "never before the reel"
        );
    }
}
