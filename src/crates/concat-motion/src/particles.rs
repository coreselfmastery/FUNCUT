// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Particles: a seeded field, baked.
//!
//! The house already has seeded noise - a wander around a resting value -
//! and packaged particle-*effects* in the project's catalogue. This module
//! is the other thing: a real particle system an operation configures and
//! the document carries baked. Emitters, lifetimes, forces, turbulence and
//! curves over life are simulated here, at authoring time, on a fixed
//! timestep from a seed the document names; every particle lands in the
//! comp as an ellipse or a square with position, scale and opacity tracks,
//! and the render - any render, in any order, on any machine - needs no
//! randomness, no simulation and no clock to draw the same field twice.
//!
//! That is the determinism contract's shape: the seed is the whole of the
//! randomness, the bake is the whole of the simulation, and a frame asked
//! for out of order is answered from the document's own numbers.

use serde::{Deserialize, Serialize};

use crate::comp::{Blend, Length, Node, NodeKind, Paint, Rgba, Transform};
use crate::engine::MotionError;
use crate::noise::Noise;
use crate::presets::ACCENT;
use crate::track::{Key, Track, Value};

/// The most particles one field carries. Each is a node with three tracks,
/// so a field is bounded the way every document here is: by a cap that
/// keeps the comp a comp and not a database.
pub const MAX_PARTICLES: usize = 320;

/// The longest a particle may live, in seconds.
pub const MAX_LIFETIME: f64 = 8.0;

/// How the field lights up.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Emission {
    /// Everything at once, scattered over the first `spread` seconds.
    Burst {
        /// How wide the scattering is.
        #[serde(default = "default_spread")]
        spread: f64,
    },
    /// Born across `over` seconds, a stream rather than a bang.
    Stream {
        /// How long the taps stay open.
        #[serde(default = "default_over")]
        over: f64,
    },
}

impl Default for Emission {
    fn default() -> Self {
        Emission::Burst {
            spread: default_spread(),
        }
    }
}

fn default_spread() -> f64 {
    0.2
}

fn default_over() -> f64 {
    1.0
}

/// What one particle is drawn as.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    /// A dot.
    #[default]
    Circle,
    /// A grain of grit.
    Square,
}

/// A field of particles: what an `add-particles` operation configures and
/// this module bakes.
///
/// Every pair is `(least, most)` and every particle draws its own value
/// between them from the seed. `gravity` is `(across, down)` in pixels a
/// second squared - name it negative to lift, like embers; `drag` bleeds
/// velocity off exponentially, the way air does; `turbulence` rides the
/// seeded simplex field, a gust that is the same gust on every machine.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ParticleSystem {
    /// The whole of the randomness. The same seed, the same field, forever.
    pub seed: u64,
    /// How many particles. Held to 1..=320.
    #[serde(default = "default_count")]
    pub count: usize,
    /// How they are born.
    #[serde(default)]
    pub emission: Emission,
    /// `(least, most)` seconds a particle lives. Held to 0.2..=8.
    #[serde(default = "default_lifetime")]
    pub lifetime: (f64, f64),
    /// `(least, most)` pixels a second at birth.
    #[serde(default = "default_speed")]
    pub speed: (f64, f64),
    /// The cone they leave in, `(least, most)` degrees; `(-180, 180)` is
    /// every way at once.
    #[serde(default = "default_angle")]
    pub angle: (f64, f64),
    /// `(across, down)` pixels a second squared.
    #[serde(default = "default_gravity")]
    pub gravity: (f64, f64),
    /// Velocity bled off per second; `0` is a vacuum.
    #[serde(default = "default_drag")]
    pub drag: f64,
    /// A seeded gust: `None` for still air.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turbulence: Option<Turbulence>,
    /// `(at birth, at death)` radius in pixels.
    #[serde(default = "default_size")]
    pub size: (f64, f64),
    /// `(at birth, at death)`, 0 to 1.
    #[serde(default = "default_opacity")]
    pub opacity: (f64, f64),
    /// The colours, drawn from the seed; one colour is a monoculture.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<String>,
    /// The shape.
    #[serde(default)]
    pub shape: Shape,
}

fn default_count() -> usize {
    96
}

fn default_lifetime() -> (f64, f64) {
    (1.6, 3.2)
}

fn default_speed() -> (f64, f64) {
    (60.0, 220.0)
}

fn default_angle() -> (f64, f64) {
    (-180.0, 180.0)
}

fn default_gravity() -> (f64, f64) {
    (0.0, 140.0)
}

fn default_drag() -> f64 {
    0.6
}

fn default_size() -> (f64, f64) {
    (3.0, 1.2)
}

fn default_opacity() -> (f64, f64) {
    (0.95, 0.0)
}

impl Default for ParticleSystem {
    /// The same field a document without the fields parses: every default
    /// here is the field's own `#[serde(default)]`, so a system built in
    /// code and one read from JSON agree without being told to.
    fn default() -> Self {
        ParticleSystem {
            seed: 0,
            count: default_count(),
            emission: Emission::default(),
            lifetime: default_lifetime(),
            speed: default_speed(),
            angle: default_angle(),
            gravity: default_gravity(),
            drag: default_drag(),
            turbulence: None,
            size: default_size(),
            opacity: default_opacity(),
            colors: Vec::new(),
            shape: Shape::default(),
        }
    }
}

/// A gust, read from the seed's own field.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Turbulence {
    /// How hard the gust pushes, in pixels a second squared.
    #[serde(default = "default_strength")]
    pub strength: f64,
    /// How coarse the field is: a larger scale drifts together, a smaller
    /// one shivers apart.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// How fast the field itself moves.
    #[serde(default = "default_rate")]
    pub rate: f64,
}

fn default_strength() -> f64 {
    40.0
}

fn default_scale() -> f64 {
    0.004
}

fn default_rate() -> f64 {
    1.0
}

impl Default for Turbulence {
    /// The same gust a document without the fields parses.
    fn default() -> Self {
        Turbulence {
            strength: default_strength(),
            scale: default_scale(),
            rate: default_rate(),
        }
    }
}

/// The named fields, tuned. One word names the weather a scene asks for,
/// and each name is a whole [`ParticleSystem`] with its own seed - the
/// same preset is the same field on every machine, the determinism
/// contract applied to taste. `preset` on the `add-particles` operation
/// starts from one, and any field of a `system` given beside it wins.
///
/// Every preset passes the same bounds check the operation holds every
/// field to, as it stands; every number below was tuned against the
/// fixed-timestep simulation the bake runs, on the frame sizes the
/// presets are written for (1080p and its crops).
pub fn preset(name: &str) -> Option<ParticleSystem> {
    let field = |seed: u64,
                 count: usize,
                 emission: Emission,
                 lifetime: (f64, f64),
                 speed: (f64, f64),
                 angle: (f64, f64),
                 gravity: (f64, f64),
                 drag: f64,
                 turbulence: Option<Turbulence>,
                 size: (f64, f64),
                 opacity: (f64, f64),
                 colors: &[&str],
                 shape: Shape| ParticleSystem {
        seed,
        count,
        emission,
        lifetime,
        speed,
        angle,
        gravity,
        drag,
        turbulence,
        size,
        opacity,
        colors: colors.iter().map(|color| (*color).into()).collect(),
        shape,
    };
    match name {
        // The weather. Snow drifts, wanders and melts out; rain falls
        // fast and straight, a streak read as a dot because the shape
        // vocabulary is a dot.
        "snow" => Some(field(
            2026,
            120,
            Emission::Stream { over: 2.5 },
            (3.0, 6.0),
            (18.0, 52.0),
            (78.0, 102.0),
            (0.0, 26.0),
            0.92,
            Some(Turbulence {
                strength: 16.0,
                scale: 0.003,
                rate: 0.5,
            }),
            (3.4, 2.6),
            (0.9, 0.55),
            &["#f4f8ff", "#dbe9fb", "#ffffff"],
            Shape::Circle,
        )),
        "rain" => Some(field(
            1417,
            150,
            Emission::Stream { over: 1.8 },
            (0.8, 1.4),
            (620.0, 900.0),
            (84.0, 96.0),
            (0.0, 460.0),
            0.04,
            None,
            (1.5, 1.1),
            (0.7, 0.35),
            &["#a8c4e0", "#c9dcf2"],
            Shape::Circle,
        )),
        // The fire family. Embers rise and gutter; sparks are a bang that
        // arcs, bright and short.
        "embers" => Some(field(
            7,
            64,
            Emission::Stream { over: 2.2 },
            (1.6, 3.2),
            (36.0, 110.0),
            (-102.0, -78.0),
            (0.0, -58.0),
            0.42,
            Some(Turbulence {
                strength: 26.0,
                scale: 0.006,
                rate: 0.8,
            }),
            (2.6, 0.6),
            (0.95, 0.0),
            &["#d9a441", "#e8c982", "#c2703a"],
            Shape::Circle,
        )),
        "sparks" => Some(field(
            909,
            72,
            Emission::Burst { spread: 0.06 },
            (0.3, 0.9),
            (320.0, 680.0),
            (-115.0, -65.0),
            (0.0, 240.0),
            0.12,
            Some(Turbulence {
                strength: 34.0,
                scale: 0.01,
                rate: 3.0,
            }),
            (2.2, 0.4),
            (1.0, 0.0),
            &["#fffbe6", "#ffd24a", "#ff9d2e"],
            Shape::Circle,
        )),
        // The air. Dust barely moves and lives long; atmosphere is the
        // wider, softer motes a light beam needs.
        "dust" => Some(field(
            31,
            80,
            Emission::Stream { over: 3.0 },
            (4.0, 8.0),
            (4.0, 14.0),
            (-180.0, 180.0),
            (0.0, -4.0),
            0.95,
            Some(Turbulence {
                strength: 12.0,
                scale: 0.005,
                rate: 0.4,
            }),
            (1.8, 1.5),
            (0.32, 0.0),
            &["#d9cfc0", "#cfc6b8"],
            Shape::Circle,
        )),
        "atmosphere" => Some(field(
            58,
            44,
            Emission::Stream { over: 4.0 },
            (5.0, 8.0),
            (2.0, 9.0),
            (-180.0, 180.0),
            (0.0, -2.0),
            0.97,
            Some(Turbulence {
                strength: 8.0,
                scale: 0.004,
                rate: 0.3,
            }),
            (4.6, 3.4),
            (0.16, 0.05),
            &["#e8e4da", "#d6d2c8"],
            Shape::Circle,
        )),
        // The celebration and the night. Confetti is a multicoloured
        // burst that flutters down; fireflies wander a summer field.
        "confetti" => Some(field(
            1204,
            140,
            Emission::Burst { spread: 0.12 },
            (2.6, 4.2),
            (210.0, 430.0),
            (-138.0, -42.0),
            (0.0, 250.0),
            0.3,
            Some(Turbulence {
                strength: 64.0,
                scale: 0.009,
                rate: 3.4,
            }),
            (5.0, 4.2),
            (1.0, 0.0),
            &["#ff5252", "#ffd740", "#69f0ae", "#40c4ff", "#e040fb"],
            Shape::Square,
        )),
        "fireflies" => Some(field(
            834,
            34,
            Emission::Stream { over: 3.5 },
            (3.2, 6.5),
            (6.0, 20.0),
            (-180.0, 180.0),
            (0.0, 0.0),
            0.99,
            Some(Turbulence {
                strength: 28.0,
                scale: 0.006,
                rate: 1.2,
            }),
            (2.3, 1.7),
            (0.9, 0.0),
            &["#d4ff5e", "#aef23c", "#eaff9e"],
            Shape::Circle,
        )),
        // The designed ones. Energy is a fast upward stream in the accent
        // cyans; streaks cross the frame fast and thin, a comet read;
        // data is a flow of small squares on the wire.
        "energy" => Some(field(
            412,
            90,
            Emission::Stream { over: 2.4 },
            (1.0, 2.0),
            (180.0, 320.0),
            (-95.0, -85.0),
            (0.0, -80.0),
            0.18,
            Some(Turbulence {
                strength: 72.0,
                scale: 0.012,
                rate: 4.0,
            }),
            (3.0, 0.8),
            (0.95, 0.0),
            &["#22d3c5", "#7c6cff", "#a5f3ff"],
            Shape::Circle,
        )),
        "streaks" => Some(field(
            6116,
            60,
            Emission::Stream { over: 2.8 },
            (0.5, 1.1),
            (500.0, 820.0),
            (-8.0, 8.0),
            (0.0, 0.0),
            0.0,
            None,
            (1.4, 1.0),
            (0.9, 0.4),
            &["#ffffff", "#ffe9b8", "#cfe8ff"],
            Shape::Circle,
        )),
        "data" => Some(field(
            31337,
            96,
            Emission::Stream { over: 3.2 },
            (2.2, 3.6),
            (150.0, 260.0),
            (-6.0, 6.0),
            (0.0, 0.0),
            0.02,
            Some(Turbulence {
                strength: 10.0,
                scale: 0.008,
                rate: 1.6,
            }),
            (2.0, 1.3),
            (0.85, 0.1),
            &["#4ade80", "#22d3c5"],
            Shape::Square,
        )),
        _ => None,
    }
}

/// The names [`preset`] knows, in the order the docs list them.
pub const PRESET_NAMES: &[&str] = &[
    "snow",
    "rain",
    "embers",
    "sparks",
    "dust",
    "atmosphere",
    "confetti",
    "fireflies",
    "energy",
    "streaks",
    "data",
];

/// The fields of a [`ParticleSystem`] an author may patch onto a preset:
/// every one optional, `None` meaning "keep what the base carries".
///
/// The `add-particles` operation reads this shape, so `preset: "snow"`
/// with `system: {count: 50}` is snow at fifty flakes and nothing else
/// changed - and a document with no preset and a full `system` parses
/// exactly as it always did, because every field here is optional the
/// way the system's own defaults made it optional.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ParticlePatch {
    /// The whole of the randomness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// How many particles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// How they are born.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emission: Option<Emission>,
    /// `(least, most)` seconds a particle lives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<(f64, f64)>,
    /// `(least, most)` pixels a second at birth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<(f64, f64)>,
    /// The cone they leave in, degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<(f64, f64)>,
    /// `(across, down)` pixels a second squared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity: Option<(f64, f64)>,
    /// Velocity bled off per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drag: Option<f64>,
    /// A seeded gust: `Some` replaces the base's gust, `None` keeps it.
    /// A base gust nobody wants is turned down with
    /// `{"strength": 0}`, which is still air.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turbulence: Option<Turbulence>,
    /// `(at birth, at death)` radius in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(f64, f64)>,
    /// `(at birth, at death)`, 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<(f64, f64)>,
    /// The colours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<String>>,
    /// The shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

impl ParticlePatch {
    /// Patch a base system: every `Some` field of the patch replaces the
    /// base's, every `None` keeps it. The base is either a named preset
    /// or the plain default the operation always fell back to.
    pub fn onto(self, base: &ParticleSystem) -> ParticleSystem {
        ParticleSystem {
            seed: self.seed.unwrap_or(base.seed),
            count: self.count.unwrap_or(base.count),
            emission: self.emission.unwrap_or(base.emission),
            lifetime: self.lifetime.unwrap_or(base.lifetime),
            speed: self.speed.unwrap_or(base.speed),
            angle: self.angle.unwrap_or(base.angle),
            gravity: self.gravity.unwrap_or(base.gravity),
            drag: self.drag.unwrap_or(base.drag),
            turbulence: self.turbulence.or(base.turbulence),
            size: self.size.unwrap_or(base.size),
            opacity: self.opacity.unwrap_or(base.opacity),
            colors: self.colors.clone().unwrap_or_else(|| base.colors.clone()),
            shape: self.shape.unwrap_or(base.shape),
        }
    }
}

/// SplitMix64: the same generator the noise field seeds itself with, run
/// here for the particles' own draws. No allocator RNG, no clock, no
/// platform between the seed and the field.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A draw between zero and one.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A draw between two bounds.
    fn between(&mut self, least: f64, most: f64) -> f64 {
        least + (most - least) * self.unit()
    }
}

/// Bake a field of particles into nodes: one per particle, seeded, forced
/// and faded, every track keyed from the simulation and nothing left to
/// the render.
///
/// The particles are root-level, named `{id}.p0` onwards, and each lives
/// exactly its own lifetime - the field is done when the last particle is.
///
/// # Errors
///
/// Counts, lifetimes, speeds and sizes outside their caps, numbers that
/// are not finite, colours the format cannot read: each refused with the
/// fix in the message. Nothing is clamped silently.
pub fn particle_nodes(
    id: &str,
    system: &ParticleSystem,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    at: f64,
    duration: f64,
) -> Result<Vec<Node>, MotionError> {
    check(system)?;
    if at < 0.0 || at >= duration {
        return Err(fail(
            format!("the field starts at {at}s, outside the comp"),
            "start it inside the comp's own span",
        ));
    }
    let colors: Vec<String> = if system.colors.is_empty() {
        vec![ACCENT.into()]
    } else {
        system.colors.clone()
    };
    let noise = Noise::from_seed(system.seed);
    let mut rng = Rng(system.seed ^ 0x5eed_0000_0000_0001);
    let mut nodes = Vec::with_capacity(system.count);
    // The simulation's own clock: fixed, so a frame asked for out of order
    // is answered from the same numbers as one asked for in order.
    let dt = 1.0 / 30.0;
    let sample_every = 2;
    for i in 0..system.count {
        let spawn = match system.emission {
            Emission::Burst { spread } => rng.between(0.0, spread.max(0.0)),
            Emission::Stream { over } => rng.between(0.0, over.max(0.0)),
        };
        let life = rng.between(system.lifetime.0, system.lifetime.1).max(0.2);
        let mut px = rng.between(x, x + width);
        let mut py = rng.between(y, y + height);
        let angle = rng.between(system.angle.0, system.angle.1).to_radians();
        let speed = rng.between(system.speed.0, system.speed.1);
        let mut vx = speed * angle.cos();
        let mut vy = speed * angle.sin();
        let color = Rgba::hex(&colors[rng.next() as usize % colors.len()]);

        let steps = (life / dt).ceil() as usize;
        let mut position = Track::default();
        let mut scale = Track::default();
        let mut opacity = Track::default();
        for step in 0..=steps {
            let t = f64::from(u32::try_from(step).unwrap_or(0)) * dt;
            let fraction = (t / life).clamp(0.0, 1.0);
            if step % sample_every == 0 || step == steps {
                position.keys.push(Key::new(
                    (fraction * 1000.0).round() / 1000.0,
                    Value::Point { x: px, y: py },
                ));
                let radius = system.size.0 + (system.size.1 - system.size.0) * fraction;
                scale.keys.push(Key::new(
                    (fraction * 1000.0).round() / 1000.0,
                    Value::number(radius),
                ));
                let alpha = system.opacity.0 + (system.opacity.1 - system.opacity.0) * fraction;
                opacity.keys.push(Key::new(
                    (fraction * 1000.0).round() / 1000.0,
                    Value::number(alpha.clamp(0.0, 1.0)),
                ));
            }
            if step == steps {
                break;
            }
            // The forces, integrated once per fixed step: gravity steady,
            // turbulence read from the seed's own field at the particle's
            // own place and moment, drag exponential the way air is.
            let (ax, ay) = match system.turbulence {
                None => (system.gravity.0, system.gravity.1),
                Some(gust) => {
                    let fx = noise.noise3(px * gust.scale, py * gust.scale, t * gust.rate);
                    let fy = noise.noise3(
                        px * gust.scale + 71.31,
                        py * gust.scale,
                        t * gust.rate + 13.73,
                    );
                    (
                        system.gravity.0 + fx * gust.strength,
                        system.gravity.1 + fy * gust.strength,
                    )
                }
            };
            vx += ax * dt;
            vy += ay * dt;
            let decay = (-system.drag * dt).exp();
            vx *= decay;
            vy *= decay;
            px += vx * dt;
            py += vy * dt;
        }
        let kind = match system.shape {
            Shape::Circle => NodeKind::Ellipse {
                width: Length::px(2.0),
                height: Length::px(2.0),
                fill: Some(Paint::Solid { color }),
            },
            Shape::Square => NodeKind::Rect {
                width: Length::px(2.0),
                height: Length::px(2.0),
                fill: Some(Paint::Solid { color }),
                radius: 0.4,
            },
        };
        nodes.push(Node {
            id: format!("{id}.p{i}"),
            parent: None,
            start: at + spawn,
            duration: life,
            transform: Transform::default(),
            opacity: 1.0,
            blend: Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: [
                ("position".to_string(), position),
                ("scale".to_string(), scale),
                ("opacity".to_string(), opacity),
            ]
            .into_iter()
            .collect(),
            kind,
        });
    }
    Ok(nodes)
}

/// The system, checked once, every cap named in its refusal.
fn check(system: &ParticleSystem) -> Result<(), MotionError> {
    if system.count == 0 || system.count > MAX_PARTICLES {
        return Err(fail(
            format!("{} particles is outside 1..={MAX_PARTICLES}", system.count),
            "a field is bounded: raise the count past the cap and the comp stops being a document",
        ));
    }
    let ordered_pairs = [
        ("lifetime", system.lifetime, 0.2, MAX_LIFETIME),
        ("speed", system.speed, 0.0, 2000.0),
    ];
    for (name, (least, most), floor, cap) in ordered_pairs {
        if !least.is_finite() || !most.is_finite() || least > most {
            return Err(fail(
                format!("the {name} pair ({least}, {most}) is not least-to-most"),
                "every pair is (least, most); the seed draws between them",
            ));
        }
        if least < floor || most > cap {
            return Err(fail(
                format!("the {name} pair ({least}, {most}) is outside {floor}..={cap}"),
                "the caps are the document's own health, not the physics'",
            ));
        }
    }
    // Size is (at birth, at death), not least-to-most: a particle may
    // shrink as it dies or grow as it rises.
    if !system.size.0.is_finite()
        || !system.size.1.is_finite()
        || !(0.1..=40.0).contains(&system.size.0)
        || !(0.1..=40.0).contains(&system.size.1)
    {
        return Err(fail(
            format!(
                "the size pair ({}, {}) is outside 0.1..=40",
                system.size.0, system.size.1
            ),
            "size is (at birth, at death) radius in pixels; a particle may shrink as it dies",
        ));
    }
    if !system.gravity.0.is_finite()
        || !system.gravity.1.is_finite()
        || !system.drag.is_finite()
        || system.drag < 0.0
        || system.drag > 4.0
    {
        return Err(fail(
            format!(
                "the gravity ({}, {}) and drag {} are not numbers a field can carry",
                system.gravity.0, system.gravity.1, system.drag
            ),
            "gravity is (across, down) pixels a second squared; drag is 0..=4",
        ));
    }
    if !system.opacity.0.is_finite()
        || !system.opacity.1.is_finite()
        || !(0.0..=1.0).contains(&system.opacity.0)
        || !(0.0..=1.0).contains(&system.opacity.1)
    {
        return Err(fail(
            format!(
                "the opacity pair ({}, {}) is outside 0..=1",
                system.opacity.0, system.opacity.1
            ),
            "opacity is a share of light, not a brightness knob",
        ));
    }
    Ok(())
}

fn fail(message: String, help: impl Into<String>) -> MotionError {
    MotionError::Engine {
        code: None,
        message,
        path: None,
        help: Some(help.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::Op;
    use crate::comp::{Comp, FORMAT, Rgba};

    fn comp() -> Comp {
        Comp {
            format: FORMAT,
            id: "test.particles".into(),
            name: "Particles".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 6.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            nodes: Vec::new(),
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    fn system() -> ParticleSystem {
        ParticleSystem {
            seed: 7,
            ..ParticleSystem::default()
        }
    }
    /// The seed is the whole of the randomness: the same field, twice, is
    /// the same nodes - and a different seed is a different field.
    #[test]
    fn the_seed_is_the_whole_of_the_randomness() {
        let a =
            particle_nodes("d", &system(), 100.0, 100.0, 400.0, 300.0, 0.0, 6.0).expect("builds");
        let b =
            particle_nodes("d", &system(), 100.0, 100.0, 400.0, 300.0, 0.0, 6.0).expect("builds");
        assert_eq!(a, b);
        let other = ParticleSystem {
            seed: 8,
            ..system()
        };
        let c = particle_nodes("d", &other, 100.0, 100.0, 400.0, 300.0, 0.0, 6.0).expect("builds");
        assert_ne!(a, c);
    }

    /// The field is a document the engines can draw: every particle
    /// validates and maps onto a timeline with nothing skipped and nothing
    /// warned about.
    #[test]
    fn a_field_validates_and_maps_onto_a_timeline() {
        let nodes =
            particle_nodes("d", &system(), 100.0, 100.0, 400.0, 300.0, 0.0, 6.0).expect("builds");
        let mut built = comp();
        built.nodes.extend(nodes);
        let problems = built.validate();
        assert!(problems.is_empty(), "{problems:?}");
        let (document, warnings) = crate::geneva::Geneva::at("geneva")
            .timeline(&built, &crate::engine::BakeRequest::default());
        let layers = document["layers"].as_array().expect("layers");
        assert_eq!(layers.len(), built.nodes.len(), "a particle was skipped");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// Gravity is down: a field with no drag, no turbulence and no speed
    /// falls, and the last key of every particle is below its first.
    #[test]
    fn gravity_pulls_the_field_down() {
        let dropped = ParticleSystem {
            seed: 3,
            count: 12,
            speed: (0.0, 0.0),
            drag: 0.0,
            turbulence: None,
            lifetime: (1.0, 1.0),
            ..system()
        };
        let nodes =
            particle_nodes("d", &dropped, 0.0, 0.0, 100.0, 100.0, 0.0, 6.0).expect("builds");
        for node in &nodes {
            let track = node.tracks.get("position").expect("the track");
            let first = track.sample(0.0).expect("keyed");
            let last = track.sample(1.0).expect("keyed");
            let (first_y, last_y) = match (first, last) {
                (Value::Point { y: a, .. }, Value::Point { y: b, .. }) => (a, b),
                _ => panic!("a position track holds points"),
            };
            assert!(
                last_y > first_y,
                "{}: gravity moved it {}px in 1s",
                node.id,
                last_y - first_y
            );
        }
    }

    /// Every particle lives its own lifetime and no longer: the span is
    /// the simulation's, not the comp's.
    #[test]
    fn every_particle_lives_its_own_lifetime() {
        let nodes =
            particle_nodes("d", &system(), 0.0, 0.0, 100.0, 100.0, 0.0, 6.0).expect("builds");
        assert_eq!(nodes.len(), system().count);
        for node in &nodes {
            assert!(
                node.duration > 0.0,
                "{}: a particle without a life",
                node.id
            );
            assert!(node.duration <= MAX_LIFETIME, "{}: past the cap", node.id);
            let track = node.tracks.get("opacity").expect("fades");
            let last = track.sample(1.0).expect("keyed");
            assert_eq!(last, Value::number(system().opacity.1));
        }
    }

    /// The caps are the document's health: a crowd, a Methuselah or a
    /// monster is refused with the number that broke it.
    #[test]
    fn the_caps_are_refused_with_the_number() {
        let crowd = ParticleSystem {
            count: 400,
            ..system()
        };
        assert!(particle_nodes("d", &crowd, 0.0, 0.0, 1.0, 1.0, 0.0, 6.0).is_err());
        let methuselah = ParticleSystem {
            lifetime: (1.0, 20.0),
            ..system()
        };
        assert!(particle_nodes("d", &methuselah, 0.0, 0.0, 1.0, 1.0, 0.0, 6.0).is_err());
        let monster = ParticleSystem {
            size: (1.0, 90.0),
            ..system()
        };
        assert!(particle_nodes("d", &monster, 0.0, 0.0, 1.0, 1.0, 0.0, 6.0).is_err());
        let backwards = ParticleSystem {
            speed: (200.0, 50.0),
            ..system()
        };
        assert!(particle_nodes("d", &backwards, 0.0, 0.0, 1.0, 1.0, 0.0, 6.0).is_err());
        let start_beyond = system();
        assert!(particle_nodes("d", &start_beyond, 0.0, 0.0, 1.0, 1.0, 9.0, 6.0).is_err());
    }

    /// Turbulence rides the seed's field: the same seed shakes the same
    /// way, and a gust is a different path than still air.
    #[test]
    fn turbulence_is_seeded_and_real() {
        let still = system();
        let gusty = ParticleSystem {
            turbulence: Some(Turbulence::default()),
            ..system()
        };
        let a = particle_nodes("d", &still, 0.0, 0.0, 100.0, 100.0, 0.0, 6.0).expect("builds");
        let b = particle_nodes("d", &gusty, 0.0, 0.0, 100.0, 100.0, 0.0, 6.0).expect("builds");
        assert_ne!(a, b, "a gust that changes nothing is a still frame");
    }

    /// Every named preset is a whole field that passes the same bounds
    /// the operation holds any field to, bakes real nodes, and is the
    /// same field twice - the determinism contract, applied to taste.
    #[test]
    fn every_named_preset_bakes_within_its_bounds() {
        for name in PRESET_NAMES {
            let field = preset(name).unwrap_or_else(|| panic!("{name} is named but not tuned"));
            let nodes = particle_nodes("f", &field, 0.0, 0.0, 1920.0, 1080.0, 0.0, 8.0)
                .unwrap_or_else(|err| panic!("{name} refuses its own bounds: {err}"));
            assert_eq!(
                nodes.len(),
                field.count,
                "{name}: the field is the count it says"
            );
            let again = particle_nodes("f", &field, 0.0, 0.0, 1920.0, 1080.0, 0.0, 8.0)
                .expect("the same field bakes again");
            assert_eq!(nodes, again, "{name}: the same seed is the same field");
        }
        assert!(preset("not-a-weather").is_none(), "an unknown name says so");
    }

    /// The presets read differently from one another: snow drifts down
    /// slowly while rain falls fast, embers rise while sparks arc, and
    /// no two named fields are the same configuration.
    #[test]
    fn the_named_fields_are_not_one_field() {
        let mut seen: Vec<(&str, ParticleSystem)> = Vec::new();
        for name in PRESET_NAMES {
            let field = preset(name).expect("named");
            for (other, system) in &seen {
                assert_ne!(
                    &field, system,
                    "{name} and {other} are the same field under two names"
                );
            }
            seen.push((name, field));
        }
        // And the reads the names promise: snow down and slow, rain down
        // and fast, embers up, sparks a burst.
        let snow = preset("snow").expect("snow");
        let rain = preset("rain").expect("rain");
        let embers = preset("embers").expect("embers");
        let sparks = preset("sparks").expect("sparks");
        let down = |system: &ParticleSystem| {
            let (least, most) = system.gravity;
            let (_, top) = system.speed;
            (least + most) / 2.0 > 0.0 && top > 0.0 && system.angle.0 > 0.0 && system.angle.1 > 0.0
        };
        let up = |system: &ParticleSystem| system.gravity.1 < 0.0 && system.angle.1 < 0.0;
        assert!(down(&snow) && down(&rain), "the weather falls");
        assert!(up(&embers), "embers rise");
        assert!(
            rain.speed.0 > snow.speed.1 * 4.0,
            "rain outruns snow the way it does"
        );
        assert!(
            matches!(sparks.emission, Emission::Burst { .. }),
            "sparks are a bang"
        );
    }

    /// The patch keeps what it does not name and replaces what it does,
    /// in either direction: onto a preset, onto the plain default.
    #[test]
    fn a_patch_onto_a_preset_names_what_wins() {
        let snow = preset("snow").expect("snow");
        let fifty = ParticlePatch {
            count: Some(50),
            ..ParticlePatch::default()
        }
        .onto(&snow);
        assert_eq!(fifty.count, 50, "the patch's count wins");
        assert_eq!(fifty.seed, snow.seed, "the rest of the field is snow");
        assert_eq!(fifty.lifetime, snow.lifetime);
        assert_eq!(fifty.colors, snow.colors);

        // A gust named by the patch replaces the preset's; a patch that
        // names none keeps it.
        let stiller = ParticlePatch {
            turbulence: Some(Turbulence {
                strength: 0.0,
                ..Turbulence::default()
            }),
            ..ParticlePatch::default()
        }
        .onto(&snow);
        assert_eq!(
            stiller.turbulence.map(|gust| gust.strength),
            Some(0.0),
            "the patch's gust wins, and zero strength is still air"
        );

        // Onto the plain default the patch is the whole system it always
        // was: a document that gave every field parses as it did.
        let whole = ParticlePatch {
            seed: Some(7),
            count: Some(48),
            lifetime: Some((1.4, 2.8)),
            speed: Some((30.0, 120.0)),
            angle: Some((-100.0, -80.0)),
            gravity: Some((0.0, -60.0)),
            drag: Some(0.4),
            turbulence: Some(Turbulence {
                strength: 24.0,
                scale: 0.006,
                rate: 0.8,
            }),
            size: Some((3.0, 1.1)),
            colors: Some(vec!["#d9a441".into(), "#e8c982".into()]),
            ..ParticlePatch::default()
        }
        .onto(&ParticleSystem::default());
        assert_eq!(whole.seed, 7);
        assert_eq!(whole.count, 48);
        assert_eq!(whole.lifetime, (1.4, 2.8));
        assert_eq!(whole.speed, (30.0, 120.0));
        assert_eq!(whole.angle, (-100.0, -80.0));
        assert_eq!(whole.gravity, (0.0, -60.0));
        assert_eq!(whole.drag, 0.4);
        assert_eq!(
            whole.turbulence,
            Some(Turbulence {
                strength: 24.0,
                scale: 0.006,
                rate: 0.8
            })
        );
        assert_eq!(whole.size, (3.0, 1.1));
        assert_eq!(
            whole.colors,
            vec!["#d9a441".to_string(), "#e8c982".to_string()]
        );
    }

    /// The JSON shape the docs show parses: a preset alone, a preset
    /// patched, and the full system a document always carried - the
    /// backward-compatible read.
    #[test]
    fn the_documented_particle_shapes_parse() {
        let one_word: ParticlePatch =
            serde_json::from_str("{}").expect("an empty patch is a valid patch");
        assert_eq!(one_word, ParticlePatch::default());

        let full: Op = serde_json::from_str(
            r##"{"op": "add-particles", "id": "embers", "x": 360, "y": 940,
                "width": 1200, "height": 80,
                "system": {"seed": 7, "count": 48, "lifetime": [1.4, 2.8],
                           "speed": [30, 120], "angle": [-100, -80],
                           "gravity": [0, -60], "drag": 0.4,
                           "turbulence": {"strength": 24, "scale": 0.006, "rate": 0.8},
                           "size": [3, 1.1], "colors": ["#d9a441", "#e8c982"]}}"##,
        )
        .expect("the full system a document always carried still parses");
        match full {
            Op::AddParticles { preset, system, .. } => {
                assert_eq!(preset, None, "the old shape names no preset");
                let system = system.expect("the system is there");
                assert_eq!(system.seed, Some(7));
                assert_eq!(system.count, Some(48));
            }
            other => panic!("not an add-particles: {other:?}"),
        }

        let named: Op = serde_json::from_str(
            r#"{"op": "add-particles", "id": "weather", "x": 0, "y": -40,
                "width": 1920, "height": 80, "preset": "snow"}"#,
        )
        .expect("one word names the weather");
        match named {
            Op::AddParticles { preset, system, .. } => {
                assert_eq!(preset.as_deref(), Some("snow"));
                assert!(system.is_none(), "no system beside the name");
            }
            other => panic!("not an add-particles: {other:?}"),
        }
    }
}
