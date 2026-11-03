// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The lint: five families of rules, every finding carrying a stable code.
//!
//! The motion principles ([`crate::discipline`], now [`sentences`] here) were
//! the first family: an opinion about curves, said as a sentence. What a comp
//! author - a designer, a template or an agent - also needs is the same said
//! about the *plumbing*: a document whose id is missing, a scene that starts
//! inside the one before it with nothing to blend them, an asset the cache
//! cannot file, a stylesheet that asks the network for a byte. Five families,
//! one contract:
//!
//! | Family | Codes | What it reads |
//! | --- | --- | --- |
//! | Structure | `L1xx` | the document: ids, names, tracks, nesting |
//! | Timing | `L2xx` | spans and boundaries, in a comp and in a sequence |
//! | Assets | `L3xx` | the files a comp points at, against the disk |
//! | Motion | `L4xx` | the principles, as curves measured not read |
//! | Determinism | `L5xx` | the network, the clock and the unseeded |
//!
//! The code namespace is the lint's own: the sidecar owns `E###`/`W###`
//! ([`crate::geneva::fix_for`]), and a document-side finding says `L###`, so
//! the two can never collide or be read as one another's. The codes are
//! **stable**: `L400` means the same thing in every future version, a new
//! rule takes a new code, and no rule is ever renumbered - [`RULES`] is the
//! registry, ordered by code, and a test holds it to that.
//!
//! Like the principles before them, the lint is **advice, not refusal**:
//! [`crate::comp::Comp::validate`] refuses a document that cannot render, and
//! several rules below shadow a refusal on purpose - a comp that is about to
//! be refused also deserves to be *told*, in the words an agent can act on,
//! before the refusal lands. A finding rides a plan's warnings or a critic's
//! eye; it never stops a render.
//!
//! Three doors, because the families read different things:
//!
//! - [`audit`]: structure, comp-level timing, motion and determinism - no
//!   disk, no reel, just the document;
//! - [`audit_assets`]: the asset family, which has to look at the files;
//! - [`audit_sequence`]: the sequence-level timing family, which has to look
//!   at the reel the scenes make together.

use std::collections::BTreeMap;
use std::path::Path;

use crate::comp::{AssetKind, Comp, NodeKind};
use crate::curve::Curve;
use crate::sequence::Sequence;

/// The family a rule belongs to. The codes say it too (`L2xx` is timing);
/// this says it in the words a report groups by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// The document: ids, names, tracks, nesting.
    Structure,
    /// Spans and boundaries, in a comp and in a sequence.
    Timing,
    /// The files a comp points at, against the disk.
    Assets,
    /// The motion principles, as curves measured not read.
    Motion,
    /// The network, the clock and the unseeded.
    Determinism,
}

impl Family {
    /// The family's own word, as a report groups by.
    pub const fn label(self) -> &'static str {
        match self {
            Family::Structure => "structure",
            Family::Timing => "timing",
            Family::Assets => "assets",
            Family::Motion => "motion",
            Family::Determinism => "determinism",
        }
    }
}

/// One rule in the registry: a stable code, the family it belongs to, and
/// the name it goes by. The registry is ordered by code and complete - a
/// rule that fires without an entry here is a rule whose code nobody can
/// look up, and a code without a rule is a promise nothing keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    /// The stable code, e.g. `L400`. Never renumbered, never reused.
    pub code: &'static str,
    /// The family the finding groups under.
    pub family: Family,
    /// The name the rule goes by, kebab-case.
    pub name: &'static str,
}

/// Every rule the lint carries, ordered by code. [`rule_count`] is this
/// list's length, and a test holds the two together with the codes'
/// uniqueness and order - the stability the codes promise is a property of
/// this table, said out loud where a change to it has to argue with it.
pub const RULES: &[Rule] = &[
    // Structure: the document itself.
    Rule {
        code: "L100",
        family: Family::Structure,
        name: "missing-comp-id",
    },
    Rule {
        code: "L101",
        family: Family::Structure,
        name: "duplicate-layer-names",
    },
    Rule {
        code: "L102",
        family: Family::Structure,
        name: "empty-tracks",
    },
    Rule {
        code: "L103",
        family: Family::Structure,
        name: "nested-comp-cycle",
    },
    // Timing: spans and boundaries.
    Rule {
        code: "L200",
        family: Family::Timing,
        name: "keyframes-outside-span",
    },
    Rule {
        code: "L201",
        family: Family::Timing,
        name: "negative-starts",
    },
    Rule {
        code: "L202",
        family: Family::Timing,
        name: "overlap-without-transition",
    },
    Rule {
        code: "L203",
        family: Family::Timing,
        name: "transition-outlasts-scene",
    },
    // Assets: the files a comp points at.
    Rule {
        code: "L300",
        family: Family::Assets,
        name: "missing-files",
    },
    Rule {
        code: "L301",
        family: Family::Assets,
        name: "unhashed-assets",
    },
    Rule {
        code: "L302",
        family: Family::Assets,
        name: "unsupported-codecs",
    },
    // Motion: the principles.
    Rule {
        code: "L400",
        family: Family::Motion,
        name: "entrance-creeps-in",
    },
    Rule {
        code: "L401",
        family: Family::Motion,
        name: "entrance-on-a-line",
    },
    Rule {
        code: "L402",
        family: Family::Motion,
        name: "exit-hangs-in-the-doorway",
    },
    Rule {
        code: "L403",
        family: Family::Motion,
        name: "exit-on-a-line",
    },
    Rule {
        code: "L404",
        family: Family::Motion,
        name: "bounce-without-the-scene-asking",
    },
    Rule {
        code: "L405",
        family: Family::Motion,
        name: "bounce-on-the-way-out",
    },
    Rule {
        code: "L406",
        family: Family::Motion,
        name: "too-many-simultaneous-animations",
    },
    // Determinism: the network, the clock and the unseeded.
    Rule {
        code: "L500",
        family: Family::Determinism,
        name: "mid-render-fetch",
    },
    Rule {
        code: "L501",
        family: Family::Determinism,
        name: "wall-clock",
    },
    Rule {
        code: "L502",
        family: Family::Determinism,
        name: "unseeded-random",
    },
    Rule {
        code: "L503",
        family: Family::Determinism,
        name: "remote-asset-src",
    },
];

/// How many rules the lint carries: [`RULES`]'s own length, said as a
/// number so a report can carry it beside the findings.
pub const fn rule_count() -> usize {
    RULES.len()
}

/// One finding: a stable code, a pointer into the document, a sentence,
/// and - when there is one - the fix. The code is what an agent keys on;
/// the sentence is what a person reads; the pointer is what a patch needs.
#[derive(Clone, PartialEq, Debug)]
pub struct Finding {
    /// The stable code, e.g. `L400` - an entry in [`RULES`].
    pub code: &'static str,
    /// Where in the document: a JSON pointer, e.g. `/nodes/3/tracks/opacity`.
    pub path: String,
    /// What is wrong, in one sentence.
    pub message: String,
    /// What to do about it, when the rule knows.
    pub help: Option<String>,
}

impl Finding {
    /// A finding, before its help.
    fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Finding {
        Finding {
            code,
            path: path.into(),
            message: message.into(),
            help: None,
        }
    }

    /// The same finding, with its fix said.
    fn with_help(mut self, help: impl Into<String>) -> Finding {
        self.help = Some(help.into());
        self
    }
}

impl std::fmt::Display for Finding {
    /// `[L400] /nodes/3/tracks/opacity: node "card" enters opacity ...` -
    /// the code, the place, the sentence, in the one line a log carries.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.path, self.message)
    }
}

/// The motion principles, audited while the comp is still the author's,
/// as the sentences the plan's warnings have always carried. The
/// principles themselves live here now; [`crate::discipline`] re-exports
/// this as `audit`, so the warnings - and every reader of them - see the
/// same sentences as before, byte for byte.
pub fn sentences(comp: &Comp, playful: bool) -> Vec<String> {
    audit(comp, playful)
        .into_iter()
        .filter(|finding| finding.code.starts_with("L4"))
        .map(|finding| finding.message)
        .collect()
}

/// Audit a comp against every family that reads the document alone:
/// structure, comp-level timing, motion and determinism. The asset family
/// needs the disk ([`audit_assets`]) and the sequence-level timing family
/// needs the reel ([`audit_sequence`]); this is the audit a plan, a critic
/// or an agent runs on a comp it holds.
pub fn audit(comp: &Comp, playful: bool) -> Vec<Finding> {
    let mut found = Vec::new();
    structure(comp, &mut found);
    comp_timing(comp, &mut found);
    motion(comp, playful, &mut found);
    determinism(comp, &mut found);
    found
}

/// Audit a comp's assets against the folder they resolve from: the files
/// the document names, on the disk that holds them. A source dir that is
/// not there is the caller's problem to say; this reads what it can and
/// says what it finds.
pub fn audit_assets(comp: &Comp, source_dir: &Path) -> Vec<Finding> {
    let mut found = Vec::new();
    assets(comp, source_dir, &mut found);
    found
}

/// Audit a sequence's timing: the boundaries its scenes make together,
/// which no single comp can see. The comp-level timing rules run under
/// [`audit`]; these are the ones that need the reel.
pub fn audit_sequence(sequence: &Sequence) -> Vec<Finding> {
    let mut found = Vec::new();
    sequence_timing(sequence, &mut found);
    found
}

// ── Structure ─────────────────────────────────────────────────────────

/// The document's own shape: an id to be named by, ids that name once,
/// tracks that say something, and nesting that terminates.
fn structure(comp: &Comp, found: &mut Vec<Finding>) {
    if comp.id.trim().is_empty() {
        found.push(
            Finding::new(
                "L100",
                "/id",
                "the comp has no id: a template, a slot or a clip refers to a comp by name, \
                 and this one cannot be named",
            )
            .with_help(
                "give the document the id its callers will name - the presets carry \
                 `funcut.<comp>.<variant>` for exactly this",
            ),
        );
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, node) in comp.nodes.iter().enumerate() {
        if let Some(first) = seen.insert(node.id.as_str(), i) {
            found.push(
                Finding::new(
                    "L101",
                    format!("/nodes/{i}/id"),
                    format!(
                        "id {:?} is already used by node {first}; ids are what tracks, slots \
                         and diagnostics point at, so they must name one thing each",
                        node.id
                    ),
                )
                .with_help("validate() refuses this document too - rename one of the two"),
            );
        }
        for (name, track) in &node.tracks {
            if track.is_empty() {
                found.push(
                    Finding::new(
                        "L102",
                        format!("/nodes/{i}/tracks/{name}"),
                        "a track with no keys animates nothing",
                    )
                    .with_help("validate() refuses this document too - drop the track, or key it"),
                );
            }
        }
        if let NodeKind::Composition { comp: nested } = &node.kind
            && nested == &comp.id
        {
            found.push(
                Finding::new(
                    "L103",
                    format!("/nodes/{i}/comp"),
                    format!(
                        "the composition contains itself: node {i} places {:?}, which is this \
                         comp, and a scene cannot be built from itself",
                        comp.id
                    ),
                )
                .with_help(
                    "the sidecar refuses a cycle through any depth as E207; this lint sees the \
                     one document, so a cycle through another comp is the bake's to name",
                ),
            );
        }
    }
}

// ── Timing: the comp's own spans ──────────────────────────────────────

/// What a comp says about time on its own: keys placed inside their span,
/// nodes that start inside the comp.
fn comp_timing(comp: &Comp, found: &mut Vec<Finding>) {
    for (i, node) in comp.nodes.iter().enumerate() {
        if node.start < 0.0 {
            found.push(
                Finding::new(
                    "L201",
                    format!("/nodes/{i}/start"),
                    format!(
                        "node {:?} starts at {:.2}s, before the comp begins; a node cannot \
                         start before the comp",
                        node.id, node.start
                    ),
                )
                .with_help("validate() refuses this document too - start it at 0 or later"),
            );
        }
        for (name, track) in &node.tracks {
            if track.keys.iter().any(|key| !(0.0..=1.0).contains(&key.at)) {
                found.push(
                    Finding::new(
                        "L200",
                        format!("/nodes/{i}/tracks/{name}"),
                        format!(
                            "node {:?}'s {name} carries a key outside its span: keys are \
                             placed as fractions of the node, 0 to 1",
                            node.id
                        ),
                    )
                    .with_help(
                        "a key at 1.5s of a 1s node is 1.5, not 1500; validate() refuses this \
                         document too",
                    ),
                );
            }
        }
    }
}

/// What a sequence says about time that no comp can see: scenes that
/// overlap with nothing to blend them, and cuts that outlast what they
/// cut from.
fn sequence_timing(sequence: &Sequence, found: &mut Vec<Finding>) {
    // Where each scene starts and ends, resolved the way the planner
    // places them: a scene without an `at` follows the one before it,
    // and a scene without a `hold` runs its comp's own length.
    let mut placed: Vec<(f64, f64)> = Vec::with_capacity(sequence.scenes.len());
    let mut cursor = 0.0;
    for scene in &sequence.scenes {
        let start = scene.at.unwrap_or(cursor);
        let length = scene.hold.unwrap_or(scene.comp.duration).max(0.0);
        placed.push((start, start + length));
        cursor = start + length;
    }
    for boundary in 0..placed.len().saturating_sub(1) {
        let (_, first_end) = placed[boundary];
        let (next_start, _) = placed[boundary + 1];
        // The transition keyed by the boundary's outgoing scene owns the
        // shared window; without one, two scenes on screen at once are
        // talking over each other.
        if next_start < first_end - 1e-9 && !sequence.transitions.contains_key(&boundary) {
            found.push(
                Finding::new(
                    "L202",
                    format!("/scenes/{}", boundary + 1),
                    format!(
                        "scene {} starts {:.2}s before scene {boundary} ends, and no transition \
                         blends them: the shared window is where a cut lives, and a cut nobody \
                         asked for is two scenes talking over each other",
                        boundary + 1,
                        first_end - next_start
                    ),
                )
                .with_help(
                    "move the scene past the one before it, or give the boundary a transition \
                     that owns the window they share",
                ),
            );
        }
    }
    for (outgoing, cut) in &sequence.transitions {
        let Some(&(start, end)) = placed.get(*outgoing) else {
            continue;
        };
        let length = end - start;
        if cut.seconds() > length + 1e-9 {
            found.push(
                Finding::new(
                    "L203",
                    format!("/transitions/{outgoing}"),
                    format!(
                        "the cut after scene {outgoing} runs {:.2}s, longer than the scene's \
                         own {:.2}s: the transition outlasts what it is cutting from",
                        cut.seconds(),
                        length
                    ),
                )
                .with_help(
                    "shorten the cut or lengthen the scene; the sidecar refuses this as E306",
                ),
            );
        }
    }
}

// ── Assets ────────────────────────────────────────────────────────────

/// The files a comp points at: that they are there, that the cache can
/// file them, and that the engine's own table names their kind.
fn assets(comp: &Comp, source_dir: &Path, found: &mut Vec<Finding>) {
    for (id, asset) in &comp.assets {
        let off_machine = asset.src.starts_with("http:")
            || asset.src.starts_with("https://")
            || asset.src.starts_with("//");
        // A src that points off-machine is the determinism family's to
        // say (L503); the disk cannot be asked about it either way.
        if off_machine {
            continue;
        }
        if !source_dir.join(&asset.src).is_file() {
            found.push(
                Finding::new(
                    "L300",
                    format!("/assets/{id}"),
                    format!(
                        "asset {id:?} points at {:?}, which is not there; the bake will look \
                         for it beside the comp and be refused as E501",
                        asset.src
                    ),
                )
                .with_help("stage the file where the comp can reach it, or fix the path"),
            );
        }
        let extension = Path::new(&asset.src)
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase());
        let inferred = extension.as_deref().and_then(crate::geneva::extension_kind);
        if asset.kind.is_none() && inferred.is_none() {
            found.push(
                Finding::new(
                    "L301",
                    format!("/assets/{id}"),
                    format!(
                        "asset {id:?} carries no `kind`, and the extension of {:?} names none \
                         the engine reads: the cache cannot file it under a family, so its \
                         content never lands in a key",
                        asset.src
                    ),
                )
                .with_help(
                    "give the asset a `kind`, or a file name whose extension names one; the \
                     sidecar refuses this as E203",
                ),
            );
        }
        if let Some(kind) = asset.kind
            && matches!(kind, AssetKind::Video | AssetKind::Audio)
            && !extension
                .as_deref()
                .is_some_and(|ext| crate::geneva::extension_kind(ext) == Some(kind))
        {
            let (family, table) = match kind {
                AssetKind::Video => ("a video", "mp4, mov, mkv, webm"),
                AssetKind::Audio => ("a sound", "wav, mp3, flac, ogg, m4a"),
                _ => unreachable!("the match above narrowed the kind"),
            };
            found.push(
                Finding::new(
                    "L302",
                    format!("/assets/{id}"),
                    format!(
                        "asset {id:?} is {family} whose {:?} the engine's own table does \
                         not name ({table}); a kind the table cannot vouch for is a codec \
                         the bake may not decode",
                        asset.src
                    ),
                )
                .with_help(
                    "re-encode the file into one of the table's containers, or expect the \
                         bake to refuse it as E501",
                ),
            );
        }
    }
}

// ── Motion: the principles ────────────────────────────────────────────

/// The family a curve's motion belongs to - the shape the principle cares
/// about, not the syntax it was written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CurveFamily {
    /// Fast out of the gate, a long settle: arrivals.
    Arrive,
    /// Slow to start, accelerating away: departures.
    Leave,
    /// Symmetric: either side's honest companion.
    Both,
    /// Steady travel: drifts, counters, plates.
    Linear,
    /// Held then jumped: mechanical, and exempt.
    Steps,
    /// Bounces past its landing: punctuation, for playful moments.
    Overshoot,
}

/// A spring's damping ratio: below one, it overshoots. The line between
/// "a spring" and "a bounce" is drawn where physics draws it.
fn damping_ratio(stiffness: f64, damping: f64, mass: f64) -> f64 {
    let omega = (stiffness / mass.max(1e-9)).sqrt();
    damping / (2.0 * mass.max(1e-9) * omega)
}

/// Which family a curve travels in, by *measuring* it rather than by
/// reading its syntax: the value at a third of the way in and at a third
/// from the end say together where a curve spends its energy, which is
/// the whole question. `apply` is the crate's own solver, so the lint and
/// the renderer can never disagree about what `ease-out` means.
fn curve_family(curve: &Curve) -> CurveFamily {
    match curve {
        Curve::Linear => CurveFamily::Linear,
        Curve::Steps { .. } => CurveFamily::Steps,
        Curve::Spring(spring) => {
            // 0.85, not 1.0: a damping ratio just under one overshoots by
            // a fraction of a percent, which no eye can see, while the
            // library's own settle spring sits at 0.997 - the line between
            // "a spring" and "a bounce" is drawn where the bounce becomes
            // visible, not where physics stops calling it overdamped.
            if damping_ratio(spring.stiffness, spring.damping, spring.mass) < 0.85 {
                CurveFamily::Overshoot
            } else {
                CurveFamily::Arrive
            }
        }
        Curve::Bezier { y1, y2, .. } => {
            if *y1 > 1.01 || *y2 > 1.01 || *y1 < -0.01 || *y2 < -0.01 {
                return CurveFamily::Overshoot;
            }
            shape_of(curve)
        }
        Curve::Points { stops } => {
            if stops
                .iter()
                .any(|(_, output)| *output > 1.01 || *output < -0.01)
            {
                return CurveFamily::Overshoot;
            }
            shape_of(curve)
        }
    }
}

/// Where a non-overshooting curve spends its energy: early, late, or
/// evenly. A curve that is a third of the way in at 55 percent of its
/// value is arriving; one holding 18 percent at the same moment is
/// leaving; anything between is symmetric.
fn shape_of(curve: &Curve) -> CurveFamily {
    let early = curve.apply(0.3);
    let late = curve.apply(0.7);
    if early + late > 1.2 {
        CurveFamily::Arrive
    } else if early + late < 0.8 {
        CurveFamily::Leave
    } else {
        CurveFamily::Both
    }
}

/// Whether a track's value is "hidden" - the 5 percent band where a rise
/// out of it is an entrance and a fall into it is an exit. Opacity is the
/// obvious one; the band keeps the lint from reading a dip as a door.
const HIDDEN: f64 = 0.05;

/// The entrance of a track, if it has one: the first segment whose value
/// rises out of the hidden band. Returns `(from, to, curve)` of the
/// segment, as fractions of the node's span - the window the
/// simultaneity rule counts and the at the sentence names.
fn entrance(track: &crate::track::Track) -> Option<(f64, f64, Option<&Curve>)> {
    let mut keys: Vec<&crate::track::Key> = track.keys.iter().collect();
    keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    for pair in keys.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let (Some(a), Some(b)) = (from.value.as_number(), to.value.as_number()) else {
            continue;
        };
        if a <= HIDDEN && b > a + HIDDEN {
            return Some((from.at, to.at, from.curve.as_ref()));
        }
    }
    None
}

/// The exit of a track, if it has one: the last segment whose value falls
/// into the hidden band. Exits are read from the end because an exit is a
/// promise about how the story ends.
fn exit(track: &crate::track::Track) -> Option<(f64, f64, Option<&Curve>)> {
    let mut keys: Vec<&crate::track::Key> = track.keys.iter().collect();
    keys.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    for pair in keys.windows(2).rev() {
        let (from, to) = (pair[0], pair[1]);
        let (Some(a), Some(b)) = (from.value.as_number(), to.value.as_number()) else {
            continue;
        };
        if b <= HIDDEN && a > b + HIDDEN {
            return Some((from.at, to.at, from.curve.as_ref()));
        }
    }
    None
}

/// The easing library's own words, for the note's suggestion.
fn suggested_for(family: CurveFamily) -> &'static str {
    match family {
        CurveFamily::Arrive | CurveFamily::Overshoot => "ease-in",
        CurveFamily::Leave | CurveFamily::Both | CurveFamily::Steps | CurveFamily::Linear => {
            "ease-out, emphasis or a spring"
        }
    }
}

/// How many nodes may enter at one instant before the lint says the
/// crowd. Five is the number the eye keeps a shape in - a sixth entrance
/// inside the same moment is a pile-on, whatever the design meant.
const SIMULTANEOUS_ENTRANCES: usize = 5;

/// One node's audit: its entrances, its exits, its curves, and a
/// persistent pointer (`/nodes/3/tracks/opacity`) that says where each
/// finding lives in the document an agent would edit.
fn motion(comp: &Comp, playful: bool, found: &mut Vec<Finding>) {
    // The entrance windows, in seconds on the comp's own clock, for the
    // simultaneity rule: a node "enters" over its earliest entrance
    // segment, whatever property rises.
    let mut windows: Vec<(f64, f64)> = Vec::new();
    for (i, node) in comp.nodes.iter().enumerate() {
        for (name, track) in &node.tracks {
            // Entrances: rise out of the hidden band.
            if let Some((at, to, curve)) = entrance(track) {
                let curve = curve.cloned().unwrap_or(Curve::Linear);
                let span = node.end(comp.duration) - node.start;
                windows.push((node.start + at * span, node.start + to * span));
                match curve_family(&curve) {
                    CurveFamily::Arrive | CurveFamily::Both | CurveFamily::Steps => {}
                    CurveFamily::Overshoot if playful => {}
                    CurveFamily::Overshoot => found.push(
                        Finding::new(
                            "L404",
                            format!("/nodes/{i}/tracks/{name}"),
                            format!(
                                "node {:?} enters {name} at {:.0}% with a bouncing curve \
                                 ({}); overshoot is punctuation for playful moments, and this \
                                 scene is not wearing one - ease-out, emphasis or an \
                                 over-damped spring lands the same read",
                                node.id,
                                at * 100.0,
                                curve_name(&curve)
                            ),
                        )
                        .with_help(
                            "an over-damped spring (the library's SPRING) lands without the \
                             bounce; SPRING_POP is the curve to take off",
                        ),
                    ),
                    CurveFamily::Leave => found.push(
                        Finding::new(
                            "L400",
                            format!("/nodes/{i}/tracks/{name}"),
                            format!(
                                "node {:?} enters {name} at {:.0}% easing in; an arrival \
                                 should spend its energy early and settle - {} (an entrance \
                                 that creeps in is the most robotic read a comp can make)",
                                node.id,
                                at * 100.0,
                                suggested_for(CurveFamily::Leave)
                            ),
                        )
                        .with_help(
                            "ease-out, emphasis or a spring; the library's EASE_OUT \
                                    is the canonical arrival",
                        ),
                    ),
                    CurveFamily::Linear => found.push(
                        Finding::new(
                            "L401",
                            format!("/nodes/{i}/tracks/{name}"),
                            format!(
                                "node {:?} enters {name} at {:.0}% on a straight line; \
                                 linear is what makes motion look cheap - {}, or steps() \
                                 when the reveal means to be mechanical",
                                node.id,
                                at * 100.0,
                                suggested_for(CurveFamily::Linear)
                            ),
                        )
                        .with_help(
                            "steps() is exempt when the reveal means to be mechanical \
                                    (the typewriter); linear is not",
                        ),
                    ),
                }
            }
            // Exits: fall into the hidden band.
            if let Some((at, _, curve)) = exit(track) {
                let curve = curve.cloned().unwrap_or(Curve::Linear);
                match curve_family(&curve) {
                    CurveFamily::Leave | CurveFamily::Both | CurveFamily::Steps => {}
                    CurveFamily::Overshoot => found.push(
                        Finding::new(
                            "L405",
                            format!("/nodes/{i}/tracks/{name}"),
                            format!(
                                "node {:?} leaves {name} at {:.0}% on a curve that bounces \
                                 ({}); a value on its way to nothing should accelerate away, \
                                 not wave - ease-in is the exit's own curve",
                                node.id,
                                at * 100.0,
                                curve_name(&curve)
                            ),
                        )
                        .with_help("ease-in is the exit's own curve, whatever the mood"),
                    ),
                    CurveFamily::Arrive => found.push(
                        Finding::new(
                            "L402",
                            format!("/nodes/{i}/tracks/{name}"),
                            format!(
                                "node {:?} leaves {name} at {:.0}% easing out; an exit that \
                                 settles hangs in the doorway - {}",
                                node.id,
                                at * 100.0,
                                suggested_for(CurveFamily::Arrive)
                            ),
                        )
                        .with_help("an exit should accelerate away: ease-in"),
                    ),
                    CurveFamily::Linear => found.push(
                        Finding::new(
                            "L403",
                            format!("/nodes/{i}/tracks/{name}"),
                            format!(
                                "node {:?} leaves {name} at {:.0}% on a straight line; a \
                                 linear fade to nothing reads as a switch, not a leave - {}",
                                node.id,
                                at * 100.0,
                                suggested_for(CurveFamily::Arrive)
                            ),
                        )
                        .with_help("ease-in out, or steps for a mechanical cut"),
                    ),
                }
            }
        }
    }
    // The crowd: more than `SIMULTANEOUS_ENTRANCES` nodes entering at one
    // instant. Counted by sweep - each window's start tested against every
    // window it sits inside - and said once, at the worst instant.
    let mut worst: Option<(f64, usize)> = None;
    for &(start, _) in &windows {
        let count = windows
            .iter()
            .filter(|&&(from, to)| from <= start + 1e-9 && start < to + 1e-9)
            .count();
        if count > SIMULTANEOUS_ENTRANCES && worst.is_none_or(|(_, known)| count > known) {
            worst = Some((start, count));
        }
    }
    if let Some((at, count)) = worst {
        found.push(
            Finding::new(
                "L406",
                "/nodes",
                format!(
                    "{count} nodes enter inside the same instant ({at:.2}s): the eye keeps a \
                     shape in five, and a sixth entrance inside one moment is a pile-on the \
                     design never meant",
                ),
            )
            .with_help(
                "stagger the entrances - authoring's SetTrackPoints keys, or a per-node delay, \
                 spread the moment into a sequence the eye can follow",
            ),
        );
    }
}

/// The library's name for a curve, for a note that suggests in the same
/// words the easing library answers to. A spring's parameters are the
/// identity when no preset name matches, because "the spring at 260/15"
/// is still a name an author can find.
fn curve_name(curve: &Curve) -> String {
    for name in Curve::NAMES {
        if let Some(named) = Curve::named(name)
            && &named == curve
        {
            return name.to_string();
        }
    }
    match curve {
        Curve::Spring(spring) => format!(
            "spring {}/{}/{}",
            spring.stiffness as u32, spring.damping as u32, spring.mass as u32
        ),
        _ => "the curve it carries".to_string(),
    }
}

// ── Determinism ───────────────────────────────────────────────────────

/// The determinism contract, read the way [`crate::comp::Comp::validate`]
/// reads it and said the way advice is said: a fetch, a clock, an
/// unseeded random or an asset that points off-machine makes the render's
/// bytes the machine's mood, not the document's.
fn determinism(comp: &Comp, found: &mut Vec<Finding>) {
    for (i, node) in comp.nodes.iter().enumerate() {
        let NodeKind::Html { markup, css, .. } = &node.kind else {
            continue;
        };
        // The same union validate() refuses with: the stylesheet, the
        // style blocks inside the markup, and the markup outside them.
        // Refused there, advised here - a comp about to be refused also
        // deserves to be told, in the words an agent can act on.
        let hits: Vec<(bool, crate::css::NetworkRef)> = crate::css::network_refs(css)
            .into_iter()
            .map(|hit| (true, hit))
            .chain(
                style_blocks(markup)
                    .flat_map(crate::css::network_refs)
                    .map(|hit| (false, hit)),
            )
            .chain(
                crate::css::network_refs(&markup_outside_styles(markup))
                    .into_iter()
                    .map(|hit| (false, hit)),
            )
            .collect();
        for (from_css, hit) in hits {
            let code = match hit.kind {
                "a mid-render fetch" => "L500",
                "the wall clock" => "L501",
                "an unseeded random" => "L502",
                // A stylesheet import is refused by validate(); the lint
                // leaves it to the refusal, which says it as well.
                _ => continue,
            };
            found.push(
                Finding::new(
                    code,
                    format!("/nodes/{i}/{}", if from_css { "css" } else { "markup" }),
                    format!(
                        "node {:?} carries {} ({:?}), and the determinism contract refuses \
                         it: the same comp must be the same bytes on every machine, at any \
                         time, online or not",
                        node.id, hit.kind, hit.text
                    ),
                )
                .with_help(
                    "stage the file as an asset the comp names by id, inline it as a data: \
                     URL, or bake the numbers into the document - set-noise-track carries a \
                     seed for exactly this; validate() refuses this document too",
                ),
            );
        }
    }
    for (id, asset) in &comp.assets {
        let off_machine = asset.src.starts_with("http:")
            || asset.src.starts_with("https://")
            || asset.src.starts_with("//");
        if off_machine {
            found.push(
                Finding::new(
                    "L503",
                    format!("/assets/{id}"),
                    format!(
                        "asset {id:?} points off-machine ({:?}), and a font or a picture the \
                         render fetches is bytes the network's mood chooses, not the \
                         document's",
                        asset.src
                    ),
                )
                .with_help(
                    "stage the file beside the comp and name it by id - a remote font is the \
                     classic case: carry it as a `font` asset and every machine draws the \
                     same face",
                ),
            );
        }
    }
}

/// The `<style>` blocks of a markup string, as stylesheet text: the comp's
/// own `css` field is applied *after* these, but the engine reads both, so
/// a network reference hiding in either is the same broken promise.
fn style_blocks(markup: &str) -> impl Iterator<Item = &str> {
    let mut rest = markup;
    std::iter::from_fn(move || {
        let open = rest.find("<style")?;
        let after_tag = rest[open..].find('>')? + open + 1;
        let body = &rest[after_tag..];
        let close = body.find("</style>")?;
        let css = &body[..close];
        rest = &body[close + "</style>".len()..];
        Some(css)
    })
}

/// The markup with its `<style>` blocks blanked to whitespace, so the
/// attribute scan reads the document *around* the styles - the styles
/// themselves are scanned as CSS, once, on their own.
fn markup_outside_styles(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    loop {
        let Some(open) = rest.find("<style") else {
            out.push_str(rest);
            return out;
        };
        let after_tag = rest[open..]
            .find('>')
            .map(|at| open + at + 1)
            .unwrap_or(open);
        out.push_str(&rest[..after_tag]);
        let body = &rest[after_tag..];
        let Some(close) = body.find("</style>") else {
            out.push_str(body);
            return out;
        };
        out.push_str(&" ".repeat(close));
        out.push_str("</style>");
        rest = &body[close + "</style>".len()..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Asset, Blend, Comp, Node, NodeKind, Rgba, Transform};
    use crate::curve::Curve;
    use crate::sequence::Sequence;
    use crate::track::{Key, Track, Value};

    /// A one-node comp whose track is given, so every finding below is
    /// the lint's and nothing else's.
    fn comp_with(track: Track) -> Comp {
        let mut node = Node {
            id: "card".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform::default(),
            opacity: 1.0,
            blend: Blend::default(),
            mask: None,
            effects: Vec::new(),
            tracks: crate::track::Tracks::default(),
            kind: NodeKind::Group {
                children: Vec::new(),
            },
        };
        node.tracks.insert("opacity".into(), track);
        Comp {
            format: crate::comp::FORMAT,
            id: "test".into(),
            name: "The test comp".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 2.0,
            background: Rgba::hex("#00000000"),
            assets: Default::default(),
            curves: Default::default(),
            nodes: vec![node],
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    /// The codes of a finding list, for the assertions that count rules.
    fn codes(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.code).collect()
    }

    // ── The registry ──────────────────────────────────────────────────

    /// The registry is the stability it promises: every code unique, the
    /// list ordered by code, and the count said out loud. A renumbered
    /// rule fails here before it fails in the wild.
    #[test]
    fn the_rules_are_counted_unique_and_in_order() {
        let mut seen = std::collections::BTreeSet::new();
        for rule in RULES {
            assert!(
                seen.insert(rule.code),
                "{:?} is in the registry twice",
                rule.code
            );
        }
        let mut ordered: Vec<_> = RULES.iter().map(|rule| rule.code).collect();
        ordered.sort();
        assert_eq!(
            RULES.iter().map(|rule| rule.code).collect::<Vec<_>>(),
            ordered,
            "the registry is ordered by code"
        );
        assert_eq!(rule_count(), RULES.len());
        assert_eq!(
            rule_count(),
            22,
            "the families carry 22 rules: 4 structure, \
                    4 timing, 3 assets, 7 motion, 4 determinism"
        );
    }

    // ── Structure ─────────────────────────────────────────────────────

    /// The structure family: a comp with no id and a duplicated node name
    /// is said with both codes; a clean comp with both said nothing.
    #[test]
    fn the_structure_family_fires_on_a_broken_document_and_stays_quiet_on_a_good_one() {
        let mut broken = comp_with(Track::new(vec![
            Key::new(0.0, Value::number(0.0)),
            Key::eased(0.4, Value::number(1.0), Curve::EASE_OUT),
        ]));
        broken.id = "   ".into();
        broken.nodes.push(broken.nodes[0].clone());
        let found = audit(&broken, false);
        assert!(
            codes(&found).contains(&"L100"),
            "the missing comp id is said: {found:?}"
        );
        assert!(
            codes(&found).contains(&"L101"),
            "the duplicate name is said: {found:?}"
        );

        let good = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
        ]));
        assert!(audit(&good, false).is_empty(), "{:?}", audit(&good, false));
    }

    /// An empty track is structure's third rule, and a comp that places
    /// itself is the fourth: a scene cannot be built from itself.
    #[test]
    fn an_empty_track_and_a_self_placed_comp_are_structure_s_own_sayings() {
        let mut empty = comp_with(Track::new(vec![]));
        empty.id = "test".into();
        let found = audit(&empty, false);
        assert!(codes(&found).contains(&"L102"), "{found:?}");

        let mut nested = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
        ]));
        nested.nodes.push(Node {
            id: "itself".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform::default(),
            opacity: 1.0,
            blend: Blend::default(),
            mask: None,
            effects: Vec::new(),
            tracks: crate::track::Tracks::default(),
            kind: NodeKind::Composition {
                comp: "test".into(),
            },
        });
        let found = audit(&nested, false);
        assert!(codes(&found).contains(&"L103"), "{found:?}");
    }

    // ── Timing ────────────────────────────────────────────────────────

    /// The comp-level timing family: a key outside its span and a node
    /// that starts before the comp are both said, with their pointers.
    #[test]
    fn the_timing_family_reads_the_comp_s_own_spans() {
        let mut out_of_span = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(1.4, Value::number(1.0)),
        ]));
        let found = audit(&out_of_span, false);
        assert!(codes(&found).contains(&"L200"), "{found:?}");
        assert!(
            found
                .iter()
                .any(|f| f.code == "L200" && f.path == "/nodes/0/tracks/opacity"),
            "{found:?}"
        );

        out_of_span.nodes[0].start = -0.5;
        let found = audit(&out_of_span, false);
        assert!(codes(&found).contains(&"L201"), "{found:?}");
    }

    /// The sequence-level timing family: an overlap with no transition to
    /// own it, and a cut that outlasts the scene before it, are both said;
    /// a sequence whose scenes follow each other is quiet.
    #[test]
    fn the_timing_family_reads_the_reel_the_scenes_make_together() {
        let scene = || {
            let mut comp = comp_with(Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
                Key::new(0.4, Value::number(1.0)),
            ]));
            comp.id = "scene".into();
            comp.duration = 2.0;
            crate::sequence::Scene {
                comp,
                ops: Vec::new(),
                at: None,
                hold: None,
                layers: Vec::new(),
                pulse: None,
                grade: None,
                look: None,
                style: None,
            }
        };
        // Overlap: the second scene starts inside the first, with no
        // transition between them to own the shared window.
        let mut overlapping = Sequence::new(1920, 1080, 30.0);
        overlapping.push(scene());
        let mut early = scene();
        early.at = Some(1.0);
        overlapping.push(early);
        let found = audit_sequence(&overlapping);
        assert!(
            codes(&found).contains(&"L202"),
            "the unblended overlap is said: {found:?}"
        );

        // A cut longer than the scene before it.
        let mut long_cut = Sequence::new(1920, 1080, 30.0);
        let mut held = scene();
        held.hold = Some(0.5);
        long_cut.push(held);
        long_cut.push(scene());
        long_cut.with_cut(
            0,
            crate::transition::Cut::WhipPan {
                direction: crate::transition::Direction::Left,
                seconds: 1.0,
                blur: 0.0,
            },
        );
        let found = audit_sequence(&long_cut);
        assert!(
            codes(&found).contains(&"L203"),
            "the cut that outlasts its scene is said: {found:?}"
        );

        // Good: scenes that follow each other, cut to length.
        let mut good = Sequence::new(1920, 1080, 30.0);
        good.push(scene());
        let mut follows = scene();
        follows.at = Some(2.0);
        good.push(follows);
        assert!(
            audit_sequence(&good).is_empty(),
            "{:?}",
            audit_sequence(&good)
        );
    }

    // ── Assets ────────────────────────────────────────────────────────

    /// The asset family: a missing file, a kind the cache cannot file and
    /// a codec the engine's table does not name are all said; real files
    /// with nameable kinds are quiet.
    #[test]
    fn the_asset_family_reads_the_disk_the_document_points_at() {
        let dir = tempfile::tempdir().expect("a scratch dir");
        std::fs::write(dir.path().join("plate.png"), b"not really a png").expect("written");

        let mut broken = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
        ]));
        broken.assets.insert(
            "plate".into(),
            Asset {
                src: "plate.png".into(),
                kind: None,
            },
        );
        broken.assets.insert(
            "ghost".into(),
            Asset {
                src: "ghost.png".into(),
                kind: None,
            },
        );
        broken.assets.insert(
            "mystery".into(),
            Asset {
                src: "mystery.xyz".into(),
                kind: None,
            },
        );
        broken.assets.insert(
            "film".into(),
            Asset {
                src: "film.avi".into(),
                kind: Some(AssetKind::Video),
            },
        );
        let found = audit_assets(&broken, dir.path());
        assert!(
            found
                .iter()
                .any(|f| f.code == "L300" && f.path == "/assets/ghost"),
            "the missing file is said: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|f| f.code == "L301" && f.path == "/assets/mystery"),
            "the unnameable kind is said: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|f| f.code == "L302" && f.path == "/assets/film"),
            "the foreign extension is said: {found:?}"
        );
        // The file that is there, with a kind its name names, is quiet.
        assert!(
            !found.iter().any(|f| f.path == "/assets/plate"),
            "the honest asset is not said: {found:?}"
        );
    }

    // ── Motion: the principles, as they always were ───────────────────

    /// The most common robotic read: a linear entrance. Flagged, with
    /// the node named.
    #[test]
    fn a_linear_entrance_is_said() {
        let notes = sentences(
            &comp_with(Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::new(0.4, Value::number(1.0)),
            ])),
            false,
        );
        assert_eq!(notes.len(), 1, "one finding, not a lecture: {notes:?}");
        assert!(
            notes[0].contains("\"card\""),
            "the node is named: {}",
            notes[0]
        );
        assert!(
            notes[0].contains("straight line"),
            "the curve is named: {}",
            notes[0]
        );
        let found = audit(
            &comp_with(Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::new(0.4, Value::number(1.0)),
            ])),
            false,
        );
        assert_eq!(codes(&found), vec!["L401"], "{found:?}");
    }

    /// Ease-in on an entrance: the creep. Flagged with the fix.
    #[test]
    fn an_entrance_that_creeps_in_is_said() {
        let found = audit(
            &comp_with(Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::EASE_IN),
                Key::new(0.4, Value::number(1.0)),
            ])),
            false,
        );
        assert_eq!(codes(&found), vec!["L400"], "{found:?}");
        assert!(
            found[0].message.contains("easing in"),
            "{}",
            found[0].message
        );
    }

    /// The arrivals the principles ask for: ease-out, emphasis, a spring
    /// that does not bounce. All clean.
    #[test]
    fn an_arrival_that_settles_is_clean() {
        for curve in [Curve::EASE_OUT, Curve::EMPHASIS, Curve::SPRING] {
            let found = audit(
                &comp_with(Track::new(vec![
                    Key::eased(0.0, Value::number(0.0), curve.clone()),
                    Key::new(0.4, Value::number(1.0)),
                ])),
                false,
            );
            assert!(found.is_empty(), "{curve:?} flagged: {found:?}");
        }
    }

    /// A mechanical reveal is a choice: steps() enters without a word
    /// said, whatever the mood.
    #[test]
    fn a_mechanical_reveal_is_a_choice_the_lint_respects() {
        let found = audit(
            &comp_with(Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::TYPEWRITER),
                Key::new(0.4, Value::number(1.0)),
            ])),
            false,
        );
        assert!(found.is_empty(), "steps flagged: {found:?}");
    }

    /// Overshoot is allowed exactly where the style allows it: flagged
    /// on a calm scene, clean on a playful one.
    #[test]
    fn a_bounce_is_punctuation_and_needs_the_scene_to_ask_for_it() {
        let bouncy = || {
            comp_with(Track::new(vec![
                Key::eased(0.0, Value::number(0.0), Curve::SPRING_POP),
                Key::new(0.4, Value::number(1.0)),
            ]))
        };
        let calm = audit(&bouncy(), false);
        assert_eq!(codes(&calm), vec!["L404"], "flagged when calm: {calm:?}");
        assert!(audit(&bouncy(), true).is_empty(), "clean when playful");
    }

    /// An exit that settles or creeps: both wrong, both said. ease-in is
    /// the exit's own curve and is clean.
    #[test]
    fn exits_leave_and_arrivals_arrive() {
        // The curve rides the segment's first key - the one that starts
        // it - so each exit below carries its curve on the key it shapes.
        let leaves = || {
            comp_with(Track::new(vec![
                Key::eased(0.6, Value::number(1.0), Curve::EASE_IN),
                Key::new(1.0, Value::number(0.0)),
            ]))
        };
        assert!(audit(&leaves(), false).is_empty(), "ease-in out is clean");
        let hangs = comp_with(Track::new(vec![
            Key::eased(0.6, Value::number(1.0), Curve::EASE_OUT),
            Key::new(1.0, Value::number(0.0)),
        ]));
        let found = audit(&hangs, false);
        assert_eq!(
            codes(&found),
            vec!["L402"],
            "an exit that settles is said: {found:?}"
        );
        assert!(
            found[0].message.contains("hangs in the doorway"),
            "{}",
            found[0].message
        );
        let switch = comp_with(Track::new(vec![
            Key::new(0.6, Value::number(1.0)),
            Key::new(1.0, Value::number(0.0)),
        ]));
        assert_eq!(
            codes(&audit(&switch, false)),
            vec!["L403"],
            "a linear exit is said"
        );
    }

    /// A bounce on the way out is flagged whatever the mood: there is no
    /// playful reading of overshooting an exit.
    #[test]
    fn an_overshooting_exit_is_never_what_a_bounce_is_for() {
        let out_on_a_bounce = comp_with(Track::new(vec![
            Key::eased(0.6, Value::number(1.0), Curve::bounce()),
            Key::new(1.0, Value::number(0.0)),
        ]));
        let found = audit(&out_on_a_bounce, true);
        assert_eq!(
            codes(&found),
            vec!["L405"],
            "said even when playful: {found:?}"
        );
        assert!(
            found[0].message.contains("bounces"),
            "the finding names the bounce: {}",
            found[0].message
        );
    }

    /// A drift is not an entrance: steady travel between two visible
    /// values is honest motion, and the lint stays out of it.
    #[test]
    fn a_drift_between_visible_values_is_not_the_lint_s_business() {
        let drifting = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
            Key::new(0.5, Value::number(1.0)),
            Key::new(1.0, Value::number(0.6)),
        ]));
        assert!(audit(&drifting, false).is_empty(), "drift flagged");
    }

    /// The crowd: six nodes entering inside one instant is said once,
    /// with the count and the instant; five staggered entrances are the
    /// choreography the limit exists to protect.
    #[test]
    fn a_pile_on_of_entrances_is_said_once_with_its_count() {
        let mut crowd = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
        ]));
        crowd.nodes.clear();
        for i in 0..6 {
            let mut node = Node {
                id: format!("card-{i}"),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform::default(),
                opacity: 1.0,
                blend: Blend::default(),
                mask: None,
                effects: Vec::new(),
                tracks: crate::track::Tracks::default(),
                kind: NodeKind::Group {
                    children: Vec::new(),
                },
            };
            node.tracks.insert(
                "opacity".into(),
                Track::new(vec![
                    Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
                    Key::new(0.4, Value::number(1.0)),
                ]),
            );
            crowd.nodes.push(node);
        }
        let found = audit(&crowd, false);
        assert_eq!(
            codes(&found),
            vec!["L406"],
            "the pile-on is said, once: {found:?}"
        );
        assert!(
            found[0].message.contains("6 nodes"),
            "the count is in the sentence: {}",
            found[0].message
        );

        // Staggered: the same six, spread over the comp's length.
        for (i, node) in crowd.nodes.iter_mut().enumerate() {
            node.start = i as f64 * 0.3;
        }
        assert!(
            audit(&crowd, false).is_empty(),
            "the stagger is a choreography, not a pile-on: {:?}",
            audit(&crowd, false)
        );
    }

    // ── Determinism ───────────────────────────────────────────────────

    /// The determinism family: a fetch, the clock, an unseeded random in
    /// the markup, and an asset that points off-machine, each said with
    /// its own code; the inline and the staged are quiet.
    #[test]
    fn the_determinism_family_refuses_the_network_the_clock_and_the_unseeded() {
        let mut wired = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
        ]));
        wired.nodes[0].kind = NodeKind::Html {
            markup: r#"<div class="ad"></div><script>if (Date.now() > 0) {}</script>"#.into(),
            css: ".ad { background: url(https://cdn.example.com/ad.png); }\
                  .ad::after { content: Math.random(); }"
                .into(),
            width: crate::comp::Length::Auto,
            height: crate::comp::Length::Auto,
        };
        wired.assets.insert(
            "brand".into(),
            Asset {
                src: "https://cdn.example.com/logo.woff2".into(),
                kind: Some(AssetKind::Font),
            },
        );
        let found = audit(&wired, false);
        assert!(
            codes(&found).contains(&"L500"),
            "the fetch is said: {found:?}"
        );
        assert!(
            codes(&found).contains(&"L501"),
            "the clock is said: {found:?}"
        );
        assert!(
            codes(&found).contains(&"L502"),
            "the unseeded random is said: {found:?}"
        );
        assert!(
            codes(&found).contains(&"L503"),
            "the off-machine asset is said: {found:?}"
        );

        // The inline and the staged: data: URLs and a file beside the
        // comp are the contract kept.
        let mut honest = comp_with(Track::new(vec![
            Key::eased(0.0, Value::number(0.0), Curve::EASE_OUT),
            Key::new(0.4, Value::number(1.0)),
        ]));
        honest.nodes[0].kind = NodeKind::Html {
            markup: r#"<div class="ad"></div>"#.into(),
            css: ".ad { background: url(data:image/png;base64,AAAA); }".into(),
            width: crate::comp::Length::Auto,
            height: crate::comp::Length::Auto,
        };
        honest.assets.insert(
            "brand".into(),
            Asset {
                src: "brand.woff2".into(),
                kind: Some(AssetKind::Font),
            },
        );
        assert!(
            audit(&honest, false).is_empty(),
            "{:?}",
            audit(&honest, false)
        );
    }

    // ── The whole lint ────────────────────────────────────────────────

    /// A comp with many problems returns every finding, not the first:
    /// one from each family the document alone can break.
    #[test]
    fn a_comp_with_many_problems_returns_every_finding_not_the_first() {
        let mut mess = comp_with(Track::new(vec![]));
        mess.id = "".into();
        mess.nodes.push(mess.nodes[0].clone());
        mess.nodes[0].start = -1.0;
        mess.nodes[0].tracks.insert(
            "scale".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::eased(1.5, Value::number(1.0), Curve::EASE_IN),
            ]),
        );
        mess.nodes[1].tracks.insert(
            "opacity".into(),
            Track::new(vec![
                Key::new(0.0, Value::number(0.0)),
                Key::new(0.4, Value::number(1.0)),
            ]),
        );
        let found = audit(&mess, false);
        let said: std::collections::BTreeSet<_> = codes(&found).into_iter().collect();
        for expected in ["L100", "L101", "L102", "L200", "L201", "L401"] {
            assert!(
                said.contains(expected),
                "{expected} is among the findings: {found:?}"
            );
        }
        assert!(found.len() >= 6, "every finding, not the first: {found:?}");
    }

    /// The library's own presets hold the principles and keep their
    /// plumbing: an audit that flags a preset is a lint arguing with a
    /// designer, and it had better be the designer who wins.
    #[test]
    fn every_preset_holds_the_principles() {
        for preset in crate::presets::ALL {
            let comp = crate::presets::build(preset.id).expect("a preset");
            let found = audit(&comp, false);
            assert!(
                found.is_empty(),
                "{} broke a principle: {found:?}",
                preset.id
            );
        }
    }

    /// The compatibility the plan's warnings were written against:
    /// `discipline::audit` returns the motion family's sentences, in the
    /// order they were found, exactly as it always did.
    #[test]
    fn discipline_still_speaks_in_sentences() {
        let comp = comp_with(Track::new(vec![
            Key::new(0.0, Value::number(0.0)),
            Key::new(0.4, Value::number(1.0)),
        ]));
        assert_eq!(
            crate::discipline::audit(&comp, false),
            vec![
                "node \"card\" enters opacity at 0% on a straight line; linear is what makes \
                 motion look cheap - ease-out, emphasis or a spring, or steps() when the \
                 reveal means to be mechanical"
                    .to_string()
            ]
        );
    }
}
