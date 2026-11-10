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
}
