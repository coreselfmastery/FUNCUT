// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Engines: how a comp becomes frames.
//!
//! Three implementations, one trait, and the timeline never knows which one
//! drew a comp:
//!
//! - [`EngineKind::Native`] - the project's own compositor. No sidecar, no
//!   extra install, and the default for everything the native path can draw.
//! - [`EngineKind::Geneva`] - the browserless HTML/CSS engine, spoken to over
//!   its JSON timeline and its `--format json` report. It is what makes
//!   design-grade markup comps possible without putting Chromium in the
//!   installer, and it is the default engine for comps that carry markup.
//! - [`EngineKind::Lottie`] - a Lottie or dotLottie file rasterised to RGBA
//!   frames. The importer for motion graphics made elsewhere.
//!
//! Choosing is a preference, not a dependency: a project records the engine
//! and its version in the bake manifest, so re-opening an old project on a
//! machine without that engine fails loudly at bake time with the frames still
//! in the cache, rather than quietly drawing something else.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::comp::Comp;

/// Which engine draws a comp.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineKind {
    /// The project's own compositor: `concat-render` plus `concat-text`.
    Native,
    /// The browserless HTML/CSS engine, as a pinned sidecar binary.
    Geneva,
    /// A Lottie or dotLottie player.
    Lottie,
}

impl EngineKind {
    /// Every engine, in the order the editor lists them.
    pub const ALL: [EngineKind; 3] = [EngineKind::Geneva, EngineKind::Native, EngineKind::Lottie];

    /// The name used in documents, settings and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Native => "native",
            EngineKind::Geneva => "geneva",
            EngineKind::Lottie => "lottie",
        }
    }

    /// Whether this engine is the one that draws markup.
    pub fn draws_markup(self) -> bool {
        matches!(self, EngineKind::Geneva)
    }
}

/// What this install prefers, and where to find the sidecar.
///
/// Geneva is the default: it is the engine that draws a comp as written,
/// including the CSS a template or an agent authors, and it costs one pinned
/// binary rather than a browser. The native engine takes over per comp when a
/// project asks it to, or when the sidecar is not installed.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct EnginePreference {
    /// The engine to use for a comp that does not name one.
    pub default: EngineKind,
    /// Where the sidecar lives, when it is not on `PATH`.
    pub geneva: Option<PathBuf>,
    /// Where bakes are cached. `None` means the project's own `cache/comps`.
    pub cache: Option<PathBuf>,
}

impl Default for EnginePreference {
    fn default() -> Self {
        EnginePreference {
            default: EngineKind::Geneva,
            geneva: None,
            cache: None,
        }
    }
}

impl EnginePreference {
    /// The engine to draw `comp` with, given what this install can actually
    /// run and what the comp is made of.
    ///
    /// A comp of nothing but shapes, images and text does not need the
    /// sidecar, so on a machine without one it falls back to the native
    /// engine - the comps it can draw - rather than failing to open the
    /// project. A comp with markup does not get that fallback: the native
    /// engine cannot draw markup, so keeping Geneva makes the bake fail
    /// loudly with the sidecar's install instructions in the message
    /// rather than quietly drawing something else. Asking for an engine
    /// that is not installed is not an error here: it becomes one at bake
    /// time, with the comp's id in the message.
    pub fn engine_for(&self, comp: &Comp, geneva_available: bool) -> EngineKind {
        if self.default == EngineKind::Geneva && !geneva_available && !comp.needs_markup() {
            return EngineKind::Native;
        }
        self.default
    }

    /// The engine to draw `comp` with, given the sidecar's state *right
    /// now* - which is [`engine_for`] plus the circuit breaker: a sidecar
    /// that has failed three times in a minute is treated exactly like a
    /// missing one for every comp the native engine can draw, and the
    /// note a caller is asked to log says so, because a fallback nobody
    /// can see is a fallback nobody can trust.
    ///
    /// A markup comp keeps Geneva even on a tripped breaker, for the same
    /// reason it keeps it on a missing sidecar: the native engine cannot
    /// draw markup, and the honest answer is the refusal - now a fast
    /// one, answered by the breaker, rather than a minute spent learning
    /// the sidecar is not answering.
    pub fn choose(&self, comp: &Comp, sidecar: Sidecar) -> Choice {
        match sidecar {
            Sidecar::Ready => Choice {
                engine: self.engine_for(comp, true),
                note: None,
            },
            Sidecar::Missing => Choice {
                engine: self.engine_for(comp, false),
                note: (!comp.needs_markup() && self.default == EngineKind::Geneva).then(|| {
                    "the geneva sidecar is not installed; drawing this comp on the native \
                     engine"
                        .to_owned()
                }),
            },
            Sidecar::Failing { failures } => Choice {
                engine: self.engine_for(comp, false),
                note: (!comp.needs_markup() && self.default == EngineKind::Geneva).then(|| {
                    format!(
                        "geneva failed {failures} times in the last minute; drawing this \
                         comp on the native engine while its circuit breaker is open"
                    )
                }),
            },
        }
    }
}

/// What state the Geneva sidecar is in, for [`EnginePreference::choose`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sidecar {
    /// Installed, versioned, and its breaker closed.
    Ready,
    /// Not installed, or not the binary it should be.
    Missing,
    /// Installed but failing: its circuit breaker is open, after three
    /// failures in a minute.
    Failing {
        /// How many failures tripped it.
        failures: usize,
    },
}

/// An engine choice, with the sentence to log when the choice is a
/// fallback: a fallback nobody can see is a fallback nobody can trust.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Choice {
    /// The engine to draw with.
    pub engine: EngineKind,
    /// Why, when the choice is news: `None` for the ordinary case.
    pub note: Option<String>,
}

/// How a comp is to be baked.
#[derive(Clone, PartialEq, Debug)]
pub struct BakeRequest {
    /// Frame width; the comp's authoring width when `None`.
    pub width: Option<u32>,
    /// Frame height.
    pub height: Option<u32>,
    /// Frame rate; the comp's own when `None`.
    pub fps: Option<f64>,
    /// Keep the alpha channel: an overlay for a layer above.
    pub alpha: bool,
    /// Encoder quality for engines that encode, 0 to 51, lower is better.
    pub crf: u8,
    /// Only these frames, for a parallel bake across machines.
    pub frames: Option<(u32, u32)>,
    /// Where the comp's asset paths resolve from - the folder the comp
    /// document lives in. Assets are copied from here into the bake, because
    /// an engine is never given a path that leaves its own directory.
    pub source_dir: std::path::PathBuf,
}

impl Default for BakeRequest {
    fn default() -> Self {
        BakeRequest {
            width: None,
            height: None,
            fps: None,
            alpha: true,
            crf: 18,
            frames: None,
            source_dir: std::path::PathBuf::new(),
        }
    }
}

impl BakeRequest {
    /// The width this bake will produce.
    pub fn width(&self, comp: &Comp) -> u32 {
        self.width.unwrap_or(comp.width)
    }

    /// The height this bake will produce.
    pub fn height(&self, comp: &Comp) -> u32 {
        self.height.unwrap_or(comp.height)
    }

    /// The frame rate this bake will produce.
    pub fn fps(&self, comp: &Comp) -> f64 {
        self.fps.unwrap_or(comp.fps)
    }
}

/// A finished bake: what is on disk, and enough about it to trust the cache.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Baked {
    /// The directory holding `0001.png` upwards.
    pub dir: PathBuf,
    /// How many frames were written.
    pub frames: usize,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Frame rate.
    pub fps: f64,
    /// The comp's content hash, which is also the cache key.
    pub hash: u64,
    /// Which engine drew it.
    pub engine: EngineKind,
    /// Which version of that engine, so a cache entry can be invalidated when
    /// the engine changes.
    pub engine_version: String,
    /// Whether the frames carry alpha.
    pub alpha: bool,
    /// Set in memory when this bake answered from the cache instead of a
    /// fresh render; never persisted, so a manifest always reads as a
    /// description of the bake it was written for.
    #[serde(skip)]
    pub cached: bool,
}

impl Baked {
    /// The file name of frame `index`, 1-based, as the sequence writes it.
    pub fn frame_name(index: u32) -> String {
        format!("{index:04}.png")
    }

    /// The path of frame `index`, 1-based.
    pub fn frame_path(&self, index: u32) -> PathBuf {
        self.dir.join(Baked::frame_name(index))
    }
}

/// Whether an engine can run here.
#[derive(Clone, PartialEq, Debug)]
pub struct Probe {
    /// Whether it can.
    pub available: bool,
    /// Its version, when it reports one.
    pub version: Option<String>,
    /// What was found, for the settings page and for a bug report.
    pub detail: String,
}

/// One way of drawing a comp.
pub trait CompEngine {
    /// Which engine this is.
    fn kind(&self) -> EngineKind;

    /// Whether it can run on this machine, and what version it is.
    fn probe(&self) -> Probe;

    /// Draw every frame of `comp` into `dir`, and say what was written.
    ///
    /// The frames are PNGs with alpha, which is the one shape every engine
    /// and every downstream consumer agrees on: the compositor samples them
    /// like any other picture, and the sidecar encodes them to a video file
    /// when a project wants one. Alpha is the PNG sequence's to keep: the
    /// sidecar's video codecs - H.264, VP9, ProRes - carry none, which the
    /// agent CLI's `--encode` says as a note on every transparent comp.
    fn bake(&self, comp: &Comp, request: &BakeRequest, dir: &Path) -> Result<Baked, MotionError>;

    /// Draw one frame at `at` seconds, for the stage's scrub and for a
    /// poster.
    ///
    /// `source_dir` is where the comp's asset paths resolve from - the
    /// folder the comp document lives in - which a frame needs as much as a
    /// bake does: without it a comp that points at a picture stages nothing
    /// and frames with a hole where the picture was.
    fn frame(
        &self,
        comp: &Comp,
        at: f64,
        width: Option<u32>,
        source_dir: &Path,
    ) -> Result<PathBuf, MotionError>;
}

/// What went wrong.
#[derive(Debug)]
pub enum MotionError {
    /// The document is wrong. The diagnostics point into it.
    Invalid {
        /// What is wrong, where, and what to do.
        diagnostics: Vec<crate::comp::Diagnostic>,
    },
    /// An engine that is not installed, or not where the setting says.
    Missing {
        /// Which engine.
        engine: EngineKind,
        /// What was looked for.
        detail: String,
    },
    /// An engine ran and refused.
    Engine {
        /// The engine's own diagnostic code, e.g. `E302`.
        code: Option<String>,
        /// What it said.
        message: String,
        /// Where in the document it stopped, as the engine names it: a
        /// JSON pointer into the timeline (`/layers/1/clips/1/start`),
        /// or a file and line for a markup problem.
        path: Option<String>,
        /// What it suggested.
        help: Option<String>,
    },
    /// An engine call outlived its limit and was killed. The process is
    /// gone and the call abandoned - nothing hangs, and the caller hears
    /// which call, and after how long.
    TimedOut {
        /// What the call was: `rendering`, `planning`, `probing`...
        what: String,
        /// The per-call limit that was reached, in seconds.
        limit: f64,
    },
    /// The circuit breaker around an engine is open: it failed too many
    /// times too close together, and further calls are refused for a
    /// while rather than spent on a sidecar that is not answering. The
    /// message says what to do - a comp the native compositor can draw
    /// is drawn by it; a markup comp or a reel waits for the breaker to
    /// close (or for the sidecar to be fixed).
    CircuitOpen {
        /// How many failures tripped it.
        failures: usize,
        /// The window they happened in, in seconds.
        window: f64,
    },
    /// A file could not be written or read.
    Io {
        /// What was being done.
        doing: String,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The comp asks for something this engine cannot draw.
    Unsupported {
        /// The node's id.
        node: String,
        /// What it is.
        what: String,
        /// Which engine refused.
        engine: EngineKind,
    },
}

impl std::fmt::Display for MotionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MotionError::Invalid { diagnostics } => {
                write!(f, "the comp is not valid: {} problem(s)", diagnostics.len())?;
                for diagnostic in diagnostics.iter().take(4) {
                    write!(f, "; {}: {}", diagnostic.path, diagnostic.message)?;
                }
                Ok(())
            }
            MotionError::Missing { engine, detail } => {
                write!(f, "{} is not available: {detail}", engine.as_str())
            }
            MotionError::Engine {
                code,
                message,
                path,
                help,
            } => {
                match code {
                    Some(code) => write!(f, "{code}: {message}")?,
                    None => write!(f, "{message}")?,
                }
                if let Some(path) = path {
                    write!(f, " (at {path})")?;
                }
                if let Some(help) = help {
                    write!(f, " ({help})")?;
                }
                Ok(())
            }
            MotionError::TimedOut { what, limit } => write!(
                f,
                "geneva did not finish {what} within {limit:.1}s and was killed - the call \
                 is abandoned, not hung"
            ),
            MotionError::CircuitOpen { failures, window } => write!(
                f,
                "geneva has failed {failures} times in the last {window:.0}s and its circuit \
                 breaker is open: calls are refused for a while instead of spent on a sidecar \
                 that is not answering. A comp without markup is drawn by the native \
                 engine meanwhile; a markup comp or a reel waits for the breaker to close"
            ),
            MotionError::Io {
                doing,
                path,
                source,
            } => {
                write!(f, "{doing} {}: {source}", path.display())
            }
            MotionError::Unsupported { node, what, engine } => {
                write!(f, "{} cannot draw {what} (node {node:?})", engine.as_str())
            }
        }
    }
}

impl std::error::Error for MotionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Blend, Comp, Length, Node, NodeKind, Paint, Rgba, Transform};

    fn comp_with(kind: NodeKind) -> Comp {
        Comp {
            format: crate::comp::FORMAT,
            id: "t".into(),
            name: String::new(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 2.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            nodes: vec![Node {
                id: "n".into(),
                parent: None,
                start: 0.0,
                duration: 0.0,
                transform: Transform::default(),
                opacity: 1.0,
                blend: Blend::Normal,
                mask: None,
                effects: Vec::new(),
                tracks: Default::default(),
                kind,
            }],
            slots: Vec::new(),
        }
    }

    #[test]
    fn geneva_is_the_default_engine() {
        assert_eq!(EnginePreference::default().default, EngineKind::Geneva);
    }

    #[test]
    fn a_markup_comp_stays_with_geneva_when_the_sidecar_is_missing() {
        let comp = comp_with(NodeKind::Html {
            markup: "<div>x</div>".into(),
            css: String::new(),
            width: Length::Auto,
            height: Length::Auto,
        });
        let preference = EnginePreference::default();
        assert_eq!(preference.engine_for(&comp, true), EngineKind::Geneva);
        // Native cannot draw markup, so the fallback would quietly draw
        // the wrong thing: the comp stays with Geneva and the bake says
        // the sidecar is missing, with the install instructions.
        assert_eq!(preference.engine_for(&comp, false), EngineKind::Geneva);
    }

    #[test]
    fn a_shape_comp_falls_back_to_native_when_the_sidecar_is_missing() {
        let comp = comp_with(NodeKind::Rect {
            width: Length::px(10.0),
            height: Length::px(10.0),
            fill: Some(Paint::Solid {
                color: Rgba::hex("#fff"),
            }),
            radius: 0.0,
        });
        let preference = EnginePreference::default();
        assert_eq!(preference.engine_for(&comp, true), EngineKind::Geneva);
        // A comp the native engine can draw falls back to it when the
        // sidecar is missing, as the crate docs promise.
        assert_eq!(preference.engine_for(&comp, false), EngineKind::Native);
    }

    #[test]
    fn a_request_falls_back_to_the_comps_own_geometry() {
        let comp = comp_with(NodeKind::Group {
            children: Vec::new(),
        });
        let request = BakeRequest::default();
        assert_eq!(request.width(&comp), 1920);
        assert_eq!(request.fps(&comp), 30.0);
    }

    #[test]
    fn frames_are_named_the_way_the_sequence_writes_them() {
        assert_eq!(Baked::frame_name(1), "0001.png");
        assert_eq!(Baked::frame_name(1200), "1200.png");
    }

    #[test]
    fn a_failing_sidecar_is_a_missing_one_for_every_comp_the_native_engine_draws() {
        let preference = EnginePreference::default();
        let shapes = comp_with(NodeKind::Rect {
            width: Length::px(10.0),
            height: Length::px(10.0),
            fill: Some(Paint::Solid {
                color: Rgba::hex("#fff"),
            }),
            radius: 0.0,
        });
        // Ready: Geneva, and nothing to log.
        let ready = preference.choose(&shapes, Sidecar::Ready);
        assert_eq!(ready.engine, EngineKind::Geneva);
        assert_eq!(ready.note, None);
        // Missing: native, with a note that says so.
        let missing = preference.choose(&shapes, Sidecar::Missing);
        assert_eq!(missing.engine, EngineKind::Native);
        assert!(missing.note.as_deref().unwrap_or_default().contains("not installed"));
        // Failing: native again, and the note names the failures - a
        // fallback nobody can see is a fallback nobody can trust.
        let failing = preference.choose(&shapes, Sidecar::Failing { failures: 3 });
        assert_eq!(failing.engine, EngineKind::Native);
        let note = failing.note.expect("the fallback is said");
        assert!(note.contains("failed 3 times"), "{note}");
        assert!(note.contains("native engine"), "{note}");
    }

    #[test]
    fn a_markup_comp_keeps_geneva_even_when_the_breaker_is_open() {
        let preference = EnginePreference::default();
        let markup = comp_with(NodeKind::Html {
            markup: "<div>x</div>".into(),
            css: String::new(),
            width: Length::Auto,
            height: Length::Auto,
        });
        // The native engine cannot draw markup, so the fallback would
        // quietly draw the wrong thing: the comp stays with Geneva and
        // the next call is answered by the breaker - fast, with the
        // fallback story - instead of by another minute of silence.
        let failing = preference.choose(&markup, Sidecar::Failing { failures: 3 });
        assert_eq!(failing.engine, EngineKind::Geneva);
        assert_eq!(failing.note, None);
        let missing = preference.choose(&markup, Sidecar::Missing);
        assert_eq!(missing.engine, EngineKind::Geneva);
        assert_eq!(missing.note, None);
    }
}
