// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Motion comps: the layer between a timeline and "make it move".
//!
//! A **comp** is a small scene graph stored as JSON in the project - nodes with
//! a span, a transform, tracks and one of a handful of kinds (group, shape,
//! path, text, markup, image, Lottie, nested comp) - plus the assets it points
//! at. Baking a comp produces an RGBA PNG sequence with alpha, which the
//! compositor samples like any other picture: it goes through the project's own
//! linear-light blending, effects and transitions, and out through its own
//! encoder. Nothing about a comp reaches around the render pipeline.
//!
//! Three engines draw comps, behind one trait ([`engine::CompEngine`]):
//!
//! | Engine | What it draws | Cost |
//! | --- | --- | --- |
//! | [`engine::EngineKind::Geneva`] | everything, markup included, browserless | one pinned binary |
//! | [`engine::EngineKind::Native`] | shapes, paths, text, images, vector/solid Lottie, nested comps, masks, mattes and supported effects | in-process tiny-skia + rasterlottie compositor |
//! | [`engine::EngineKind::Lottie`] | Lottie and dotLottie files | reserved; a player is not implemented |
//!
//! [`engine::EnginePreference::default`] is Geneva, because a comp written by a
//! designer, a template or an agent may contain markup. The native engine is
//! implemented in-process and is the fallback for supported non-markup comps
//! when Geneva is unavailable. It renders vector/solid Lottie and dotLottie
//! nodes through the pure-Rust rasterlottie player, including supported image
//! and text assets. Unsupported Lottie features are surfaced by its support
//! analyzer; HTML/markup and shader packages remain outside the native engine.
//! The Bodymovin adapter still imports a supported subset of JSON into comp nodes.
//!
//! # Where it plugs in
//!
//! - **Project**: a `Motion` clip points at `comps/<hash>.json`. The document
//!   is content-addressed, so two clips placing the same comp share one bake,
//!   and an edit changes the hash and misses the cache by itself.
//! - **Render**: `concat-export` asks for a comp's frames, `0001.png`
//!   upwards, and the cache answers when it can; the engine bakes the rest
//!   into `cache/comps/<hash>-<w>x<h>>@<fps>/`. The export loop never waits
//!   on a sidecar mid-frame: a bake is a preparation step, not a per-frame
//!   call.
//! - **Preview**: [`engine::CompEngine::frame`] draws one frame at a time for
//!   the stage - tens of milliseconds, so a scrub is responsive without
//!   baking.
//! - **Agent**: every operation here is data-in, data-out, so the MCP tools
//!   (`motion_presets`, `motion_inspect`, `motion_frame`, `motion_bake`,
//!   `captions_animate`, `motion_overlay`, `motion_reel`) are thin wrappers
//!   over this crate rather than a second implementation.
//!
//! # Example
//!
//! ```no_run
//! use concat_motion::{comp::Comp, engine::{BakeRequest, CompEngine, EnginePreference}, geneva::Geneva, presets};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let comp: Comp = presets::build("funcut.lower-third.glass").expect("a preset");
//! let problems = comp.validate();
//! assert!(problems.is_empty());
//!
//! let preference = EnginePreference::default();
//! let geneva = Geneva::discover(preference.geneva.as_deref()).expect("the sidecar");
//! let request = BakeRequest { source_dir: "comps".into(), ..BakeRequest::default() };
//! let baked = geneva.bake(&comp, &request, std::path::Path::new("cache/comps/abc"))?;
//! println!("{} frames of {}x{}", baked.frames, baked.width, baked.height);
//! # Ok(())
//! # }
//! ```

pub mod authoring;
pub mod bench;
pub mod cache;
pub mod camera;
pub mod captions;
pub mod comp;
pub mod continuity;
pub mod critic;
pub mod css;
pub mod curve;
pub mod discipline;
pub mod engine;
pub mod farm;
pub mod footage;
pub mod geneva;
pub mod grade;
pub mod graphics;
pub mod lint;
pub mod looks;
/// The native engine, behind the `raster` feature: the in-process
/// compositor a machine without the Geneva sidecar falls back to, and
/// the only engine that draws vector paths, shape gradients, track
/// mattes and adjustment layers.
#[cfg(feature = "raster")]
pub mod native;
pub mod noise;
pub mod particles;
pub mod plan;
pub mod presets;
pub mod progress;
pub mod pulse;
pub mod reel;
pub mod revision;
pub mod route;
pub mod sequence;
pub mod storyboard;
pub mod style;
pub mod track;
pub mod transition;
pub mod typography;

pub use authoring::{CompSummary, Op, apply};
pub use camera::{CameraMove, apply_camera};
pub use comp::{Comp, FORMAT, Node, NodeKind, Slot};
pub use continuity::{
    AssetManifest, ContinuityRule, Discontinuity, ManifestClip, Remedy, ShotScale, TravelDirection,
    check_continuity,
};
pub use critic::{Fix, Report, score};
pub use curve::{Curve, Spring, StepPosition};
pub use engine::{
    BakeRequest, Baked, Choice, CompEngine, EngineKind, EnginePreference, MotionError, Probe,
    Sidecar,
};
pub use footage::{FootagePlan, suggest_footage};
pub use geneva::{Breaker, CallLimits, Diagnostic, Geneva, fix_for};
pub use graphics::{Area, Chart, ChartKind, Counter, Datum, NumberFormat};
pub use lint::{Finding, RULES, Rule, rule_count};
pub use looks::Look;
// The plan's footage Slot lives at `concat_motion::plan::Slot`: the root
// name belongs to the comp's template Slot, and two Slots on one root is
// an ambiguity, not a convenience.
pub use noise::Noise;
pub use particles::{Emission, ParticlePatch, ParticleSystem, Shape, Turbulence};
pub use plan::{Entry, Exit, SlotJob, SlotRole, Treatment, broll_share};
pub use pulse::{BeatGrid, Driven, Pulse, curve_for_act, sound_starts_at};
pub use reel::{ReelReport, SceneOutcome, render_reel, scene_key};
pub use revision::{
    GateDecision, MAX_RETRIES, SHIP_SCORE, SlotRevision, attempt_path, decide, failing_slot_ids,
};
pub use route::{BeatType, Decision, Routing, route};
pub use track::{Key, Track, Tracks, Value};
pub use transition::{Cut, Direction};

/// The library version of the preset set, bumped when a preset changes shape
/// so a project's saved copies can be offered an upgrade.
pub const PRESETS_VERSION: u32 = 1;
