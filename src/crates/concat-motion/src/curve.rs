// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Curves: how a value travels from one keyframe to the next.
//!
//! Four shapes, because they cover what motion design actually uses:
//!
//! - [`Curve::Bezier`] - a CSS `cubic-bezier()`. The editor's curve editor
//!   hands one of these out for every key, and the presets write the rest.
//! - [`Curve::Steps`] - a CSS `steps()`, for hold-then-jump motion: typewriters,
//!   mechanical counters, a word popping in on the beat.
//! - [`Curve::Points`] - a CSS `linear()` ramp of intermediate stops, which is
//!   how a designer traces a hand-drawn feel without a spline editor.
//! - [`Curve::Spring`] - a damped harmonic oscillator solved analytically and
//!   rescaled to land exactly on the next key. Springs are the default in every
//!   modern motion identity, and a bezier cannot fake the overshoot they make.
//!
//! Everything here is `f64` in, `f64` out, with no allocation on the sampling
//! path except `Points`, and no frame-rate assumption: the caller asks for a
//! fraction of the segment.
//!
//! The bezier solver is the same shape as the one in `concat-core`'s
//! `animate` module - Newton first, bisection where the curve is flat enough
//! that Newton stalls - because a comp and a clip must not disagree about what
//! `ease-out` means by a fraction of a pixel.

use serde::{Deserialize, Serialize};

/// A timing function for one segment between two keys.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Curve {
    /// A straight line: the value moves evenly.
    #[default]
    Linear,
    /// A CSS `cubic-bezier(x1, y1, x2, y2)`, endpoints pinned at 0 and 1.
    Bezier {
        /// First control point's x, clamped into `0..=1`.
        x1: f64,
        /// First control point's y; may overshoot.
        y1: f64,
        /// Second control point's x, clamped into `0..=1`.
        x2: f64,
        /// Second control point's y; may overshoot.
        y2: f64,
    },
    /// A CSS `steps(n, position)`.
    Steps {
        /// How many steps the segment is walked in.
        count: u32,
        /// Where the jump inside each step happens.
        #[serde(default)]
        position: StepPosition,
    },
    /// A CSS `linear()` ramp: `(input, output)` stops, inputs 0 to 1 in order.
    Points {
        /// The stops, as `(input, output)` pairs.
        stops: Vec<(f64, f64)>,
    },
    /// A damped spring.
    Spring(Spring),
}

/// Where a `steps()` curve jumps within each step.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepPosition {
    /// Hold each step's value and jump at the interval's end (CSS
    /// `jump-end`, the default): `steps(4)` at `t = 0.01` is still 0.
    #[default]
    JumpEnd,
    /// Jump at the interval's start (CSS `jump-start`): `steps(4)` at
    /// `t = 0.01` is already 1/4 - and so is `t = 0`, because the first
    /// interval starts there and the rise leads it.
    JumpStart,
    /// No jump at either end (CSS `jump-none`): `n - 1` rises between 0
    /// and 1, so the first interval holds 0 and the last lands on 1.
    JumpNone,
    /// A jump at 0 and at 1 (CSS `jump-both`): `n + 1` rises of
    /// `1 / (n + 1)` each, so the value starts above 0 and ends at 1.
    JumpBoth,
}

/// The parameters of a spring, in the shape CSS and every design tool use.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Spring {
    /// Stiffness: how hard the spring pulls. Higher is faster.
    pub stiffness: f64,
    /// Damping: how quickly the bounce dies. Lower overshoots more.
    pub damping: f64,
    /// Mass: how much it resists the pull. Higher is slower and heavier.
    pub mass: f64,
}

impl Default for Spring {
    fn default() -> Self {
        Spring {
            stiffness: 170.0,
            damping: 26.0,
            mass: 1.0,
        }
    }
}

impl Curve {
    /// CSS `ease`, as its four numbers.
    pub const EASE: Curve = Curve::Bezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
    };
    /// CSS `ease-in`.
    pub const EASE_IN: Curve = Curve::Bezier {
        x1: 0.42,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };
    /// CSS `ease-out`.
    pub const EASE_OUT: Curve = Curve::Bezier {
        x1: 0.0,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };
    /// CSS `ease-in-out`.
    pub const EASE_IN_OUT: Curve = Curve::Bezier {
        x1: 0.42,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };
    /// The curve modern UI motion is built on: fast out of the gate, a long
    /// settle, no overshoot. Reads as "expensive" on a card entrance.
    pub const EMPHASIS: Curve = Curve::Bezier {
        x1: 0.16,
        y1: 1.0,
        x2: 0.3,
        y2: 1.0,
    };
    /// A spring with the defaults every preset here shares.
    pub const SPRING: Curve = Curve::Spring(Spring {
        stiffness: 170.0,
        damping: 26.0,
        mass: 1.0,
    });
    /// A spring that overshoots once and settles: logo pops, chip entrances.
    pub const SPRING_POP: Curve = Curve::Spring(Spring {
        stiffness: 260.0,
        damping: 15.0,
        mass: 1.0,
    });
    /// A typewriter: `steps(8, jump-end)` - eight mechanical holds, the
    /// reveal arriving in clacks rather than a slide. The curve a
    /// per-character stagger was born to carry.
    pub const TYPEWRITER: Curve = Curve::Steps {
        count: 8,
        position: StepPosition::JumpEnd,
    };

    /// A bounce without JavaScript: `linear(0, 1.4, 0.9, 1)` - the CSS
    /// `linear()` ramp that overshoots by 40 percent and settles back,
    /// which is a whole bounce in one timing function. The four plain
    /// values are the even inputs of `linear(0, 1.4, 0.9, 1)`, spelled
    /// as the stops the model carries. A function, not a constant,
    /// because the stops are an allocation.
    pub fn bounce() -> Curve {
        Curve::Points {
            stops: vec![(0.0, 0.0), (1.0 / 3.0, 1.4), (2.0 / 3.0, 0.9), (1.0, 1.0)],
        }
    }

    /// The curve a document names - the easing library, exposed as a
    /// vocabulary instead of five structs' worth of construction.
    ///
    /// Every name is what `Curve::css()` writes or what a storyboard, an
    /// MCP call or an agent's JSON carries: `"ease-out"`, `"spring-pop"`,
    /// `"typewriter"`, `"bounce"`, and the rest of the list below. An
    /// unknown name is `None`, so a caller says it in its own words
    /// rather than guessing a default.
    pub fn named(name: &str) -> Option<Curve> {
        match name.trim().to_ascii_lowercase().as_str() {
            "linear" => Some(Curve::Linear),
            "ease" => Some(Curve::EASE),
            "ease-in" => Some(Curve::EASE_IN),
            "ease-out" => Some(Curve::EASE_OUT),
            "ease-in-out" => Some(Curve::EASE_IN_OUT),
            "emphasis" => Some(Curve::EMPHASIS),
            "spring" => Some(Curve::SPRING),
            "spring-pop" => Some(Curve::SPRING_POP),
            "typewriter" => Some(Curve::TYPEWRITER),
            "bounce" => Some(Curve::bounce()),
            _ => None,
        }
    }

    /// The names the easing library answers to, in the order a gallery
    /// would show them.
    pub const NAMES: [&str; 10] = [
        "linear",
        "ease",
        "ease-in",
        "ease-out",
        "ease-in-out",
        "emphasis",
        "spring",
        "spring-pop",
        "typewriter",
        "bounce",
    ];

    /// The eased fraction for a linear one, at `t` in `0..=1`.
    ///
    /// A spring may return values above 1 or below 0 on the way: that
    /// overshoot is the point of it, and the caller mixes with it rather than
    /// clamping. A bezier with a y outside `0..=1` does the same.
    pub fn apply(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::Bezier { x1, y1, x2, y2 } => bezier_y_at_x(*x1, *y1, *x2, *y2, t),
            Curve::Steps { count, position } => steps(t, *count, *position),
            Curve::Points { stops } => points(t, stops),
            Curve::Spring(spring) => spring_at(spring, t),
        }
    }

    /// The curve as the CSS timing function the markup engine plays:
    /// `linear`, `cubic-bezier()`, `steps()` or `linear()`.
    ///
    /// CSS has no spring, so a spring is approximated by the bezier that
    /// overshoots by the amount the spring's own damping ratio says it
    /// would - the one honest translation, and the reason a comp that wants
    /// its spring *exactly* animates it as a timeline track
    /// ([`Curve::Spring`] is solved analytically there) rather than as
    /// markup CSS. A critic damping ratio settles without overshoot and
    /// maps to the settle curve every preset shares.
    pub fn css(&self) -> String {
        match self {
            Curve::Linear => "linear".into(),
            Curve::Bezier { x1, y1, x2, y2 } => {
                format!("cubic-bezier({x1}, {y1}, {x2}, {y2})")
            }
            Curve::Steps { count, position } => {
                let name = match position {
                    StepPosition::JumpEnd => "jump-end",
                    StepPosition::JumpStart => "jump-start",
                    StepPosition::JumpNone => "jump-none",
                    StepPosition::JumpBoth => "jump-both",
                };
                format!("steps({count}, {name})")
            }
            Curve::Points { stops } => {
                let ramp = stops
                    .iter()
                    .map(|(input, output)| format!("{input} {output}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("linear({ramp})")
            }
            Curve::Spring(spring) => spring_css(spring),
        }
    }
}

/// The bezier a spring reads as in CSS: the overshoot of the damped
/// oscillator, spent on the bezier's first control point.
fn spring_css(spring: &Spring) -> String {
    let natural = (spring.stiffness / spring.mass).max(1e-9).sqrt();
    let ratio = (spring.damping / (2.0 * natural)).clamp(0.0, 1.0);
    // A damping ratio of 0.95 already overshoots by less than a hundredth
    // of a percent - imperceptible - so everything from there up is the
    // settle curve, and only a real overshoot writes one.
    if ratio >= 0.95 {
        // Settling: no overshoot to spend.
        "cubic-bezier(0.16, 1, 0.3, 1)".into()
    } else {
        // Under damped: the first peak of e^(-ζπ/√(1-ζ²)) is the overshoot.
        let overshoot = (-std::f64::consts::PI * ratio / (1.0 - ratio * ratio).sqrt()).exp();
        // Two decimals, so the stylesheet a comp writes is the same bytes
        // for the same spring, and a cache key does not see jitter.
        let peak = ((1.0 + overshoot) * 100.0).round() / 100.0;
        format!("cubic-bezier(0.3, {peak}, 0.6, 1)")
    }
}

/// A curve from the JSON a storyboard or an agent carries: a name from
/// the easing library (`"bounce"`, `"typewriter"`, `"ease-out"`), or a
/// full curve object (`{"kind": "steps", "count": 8}`). A name that is
/// not in the library is an error naming the ones that are, because a
/// typo silently ignored is a layer that animates on the wrong curve.
pub fn from_json(value: &serde_json::Value) -> Result<Curve, String> {
    match value {
        serde_json::Value::String(name) => Curve::named(name).ok_or_else(|| {
            format!(
                "{name:?} is not a curve the library knows; the names are {}",
                Curve::NAMES.join(", ")
            )
        }),
        other => serde_json::from_value(other.clone())
            .map_err(|error| format!("a curve is a name or a curve object: {error}")),
    }
}

/// x of a cubic bezier with endpoints pinned at 0 and 1, at parameter `t`.
fn bezier_axis(p1: f64, p2: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

fn bezier_axis_slope(p1: f64, p2: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

/// Solve a CSS cubic-bezier for y at a given x.
///
/// Newton first, bisection as the fallback where the curve is flat enough that
/// Newton stalls. The same solver as `concat-core`'s, deliberately: a preview
/// whose easing disagreed with the export's would disagree invisibly.
pub fn bezier_y_at_x(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
    let mut t = x;

    for _ in 0..8 {
        let error = bezier_axis(x1, x2, t) - x;
        if error.abs() < 1e-7 {
            return bezier_axis(y1, y2, t);
        }
        let slope = bezier_axis_slope(x1, x2, t);
        if slope.abs() < 1e-9 {
            break;
        }
        t = (t - error / slope).clamp(0.0, 1.0);
    }

    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..40 {
        let at = bezier_axis(x1, x2, t);
        if (at - x).abs() < 1e-7 {
            break;
        }
        if at > x {
            hi = t;
        } else {
            lo = t;
        }
        t = 0.5 * (lo + hi);
    }
    bezier_axis(y1, y2, t)
}

/// A CSS `steps()` at `t`.
///
/// The four positions are the CSS ones, spelled the CSS way, because the
/// name is what Geneva is handed and the bake must agree with
/// [`Curve::apply`] sample for sample: the spec's algorithm is
/// `floor(t * n)`, plus one for the positions that jump at an interval's
/// start, clamped to the number of jumps - so a boundary belongs to the
/// interval that *starts* there when the jump leads it, and to the one
/// that ends there when the jump trails it.
fn steps(t: f64, count: u32, position: StepPosition) -> f64 {
    let n = count.max(1) as f64;
    match position {
        // CSS `jump-end`, the default: hold, then jump on the boundary.
        StepPosition::JumpEnd => (t * n).floor() / n,
        // CSS `jump-start`: the rise leads the interval. The spec's
        // algorithm is `floor(t * n) + 1`, clamped to `n` - so a boundary
        // belongs to the interval that starts there, and the first jump is
        // at 0 itself. `ceil(t * n)` agrees only between boundaries.
        StepPosition::JumpStart => ((t * n).floor() + 1.0).min(n) / n,
        // CSS `jump-none`: n - 1 interior rises of 1 / (n - 1), none at
        // the ends; n = 1 degenerates to the constant 0, as before.
        StepPosition::JumpNone => (t * n).floor().clamp(0.0, n - 1.0) / (n - 1.0).max(1.0),
        // CSS `jump-both`: n + 1 rises of 1 / (n + 1), the first at 0 and
        // the last at 1.
        StepPosition::JumpBoth => ((t * n).floor() + 1.0) / (n + 1.0),
    }
}

/// A CSS `linear()` ramp at `t`: piecewise-linear through the stops, holding
/// the ends outside them. Stops are sorted by input when sampled, so a
/// hand-written list with a wobble still reads.
fn points(t: f64, stops: &[(f64, f64)]) -> f64 {
    let mut sorted: Vec<(f64, f64)> = stops.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if sorted.is_empty() {
        return t;
    }
    if t <= sorted[0].0 {
        return sorted[0].1;
    }
    for pair in sorted.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let span = b.0 - a.0;
            if span.abs() < 1e-12 {
                return b.1;
            }
            let u = (t - a.0) / span;
            return a.1 + (b.1 - a.1) * u;
        }
    }
    sorted[sorted.len() - 1].1
}

/// A spring at `t` in `0..=1`, rescaled so it lands exactly on 1 at the end of
/// the segment.
///
/// Solved analytically: the damped oscillator's step response, sampled at
/// `t = 1` to find where it would have settled and dividing it out. That is
/// what makes a spring usable as a *keyframe* curve - the parameters shape the
/// overshoot, not the duration, so retiming a comp does not retune its motion.
fn spring_at(spring: &Spring, t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let stiffness = spring.stiffness.max(1e-6);
    let damping = spring.damping.max(0.0);
    let mass = spring.mass.max(1e-6);
    let omega0 = (stiffness / mass).sqrt();
    let zeta = damping / (2.0 * (stiffness * mass).sqrt());

    // The segment is one unit long and the physics is scaled into it: t is
    // enough of the settle for the shape to read, the normalisation puts the
    // arrival exactly on the next key.
    let raw = |x: f64| -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if zeta < 1.0 {
            let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
            let envelope = (-zeta * omega0 * x).exp();
            let phase = (1.0 - zeta * zeta).sqrt();
            1.0 - envelope * ((omega_d * x).cos() + (zeta / phase) * (omega_d * x).sin())
        } else if zeta == 1.0 {
            let envelope = (-omega0 * x).exp();
            1.0 - envelope * (1.0 + omega0 * x)
        } else {
            let root = (zeta * zeta - 1.0).sqrt();
            let (r1, r2) = (-omega0 * (zeta - root), -omega0 * (zeta + root));
            let c2 = r1 / (r1 - r2);
            let c1 = 1.0 - c2;
            1.0 - (c1 * (r1 * x).exp() + c2 * (r2 * x).exp())
        }
    };

    let settle = raw(1.0);
    if settle.abs() < 1e-6 {
        return t;
    }
    raw(t) / settle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_the_identity() {
        for t in [0.0, 0.1, 0.5, 0.9, 1.0] {
            assert!((Curve::Linear.apply(t) - t).abs() < 1e-12);
        }
    }

    /// The CSS a markup stylesheet needs, spelled the way the engine plays
    /// it: every curve shape has its own grammar, and the one CSS cannot
    /// say - a spring - is the bezier that overshoots by the spring's own
    /// damping ratio instead of a guess.
    #[test]
    fn every_curve_writes_the_css_timing_function_it_is() {
        assert_eq!(Curve::Linear.css(), "linear");
        assert_eq!(
            Curve::Bezier {
                x1: 0.16,
                y1: 1.0,
                x2: 0.3,
                y2: 1.0
            }
            .css(),
            "cubic-bezier(0.16, 1, 0.3, 1)"
        );
        assert_eq!(
            Curve::Steps {
                count: 8,
                position: StepPosition::JumpEnd
            }
            .css(),
            "steps(8, jump-end)"
        );
        assert_eq!(
            Curve::Points {
                stops: vec![(0.0, 0.0), (0.35, 1.4), (0.6, 0.9), (1.0, 1.0)]
            }
            .css(),
            "linear(0 0, 0.35 1.4, 0.6 0.9, 1 1)"
        );
        // The library's settling spring is critic-damped at heart: it maps
        // to the settle curve, not to an overshoot it does not have.
        assert_eq!(Curve::SPRING.css(), "cubic-bezier(0.16, 1, 0.3, 1)");
        // The pop spring overshoots: 260/15/1 is a damping ratio of 0.465,
        // whose first peak is e^(-0.465π/√(1-0.465²)) ≈ 0.19 above the mark.
        assert_eq!(Curve::SPRING_POP.css(), "cubic-bezier(0.3, 1.19, 0.6, 1)");
    }

    /// The named library: every name answers, every curve is reachable
    /// by the name a document carries, and the two new ones are what
    /// their use says - a typewriter's clacks and a bounce's overshoot.
    #[test]
    fn the_easing_library_answers_to_its_names() {
        for name in Curve::NAMES {
            assert!(Curve::named(name).is_some(), "{name} is not in the library");
        }
        assert!(Curve::named("nope").is_none());
        assert_eq!(Curve::named("typewriter"), Some(Curve::TYPEWRITER));
        assert_eq!(Curve::named("bounce"), Some(Curve::bounce()));
        // Case and padding are the document's, not the library's.
        assert_eq!(Curve::named("  Ease-Out "), Some(Curve::EASE_OUT));
    }

    /// A bounce overshoots and lands: `linear(0, 1.4, 0.9, 1)` is above
    /// 1 on the way and exactly 1 at the end - a whole bounce in one
    /// timing function, no JavaScript in sight.
    #[test]
    fn a_bounce_overshoots_and_lands() {
        assert!((Curve::bounce().apply(1.0) - 1.0).abs() < 1e-9);
        let peak = Curve::bounce().apply(1.0 / 3.0);
        assert!(peak > 1.2, "the bounce must overshoot, not slide: {peak}");
        let settle = Curve::bounce().apply(0.9);
        assert!(settle > 0.9 && settle < 1.0, "the settle reads: {settle}");
        assert!(
            Curve::bounce().css().starts_with("linear("),
            "{}",
            Curve::bounce().css()
        );
    }

    /// A typewriter holds and jumps: eight clacks, the value never ahead
    /// of the step it is in.
    #[test]
    fn a_typewriter_holds_then_jumps() {
        assert_eq!(Curve::TYPEWRITER.apply(0.01), 0.0);
        assert_eq!(Curve::TYPEWRITER.apply(0.2), 0.125);
        assert_eq!(Curve::TYPEWRITER.apply(1.0), 1.0);
    }

    /// The JSON a storyboard carries: a name, a curve object, and a
    /// refusal that names the library rather than guessing.
    #[test]
    fn a_curve_comes_from_json_as_a_name_or_an_object() {
        use serde_json::json;
        assert_eq!(
            from_json(&json!("bounce")).expect("the name is in the library"),
            Curve::bounce()
        );
        assert_eq!(
            from_json(&json!({"kind": "steps", "count": 8, "position": "jump-end"}))
                .expect("a curve object"),
            Curve::TYPEWRITER
        );
        let error = from_json(&json!("nope")).expect_err("the name is unknown");
        assert!(error.contains("nope"), "{error}");
        assert!(error.contains("bounce"), "{error}");
    }

    #[test]
    fn a_bezier_keeps_its_ends_and_stays_monotone_for_css_curves() {
        for curve in [
            Curve::EASE,
            Curve::EASE_IN,
            Curve::EASE_OUT,
            Curve::EASE_IN_OUT,
            Curve::EMPHASIS,
        ] {
            assert!(curve.apply(0.0).abs() < 1e-9, "{curve:?} starts at 0");
            assert!((curve.apply(1.0) - 1.0).abs() < 1e-9, "{curve:?} ends at 1");
            let mut last = -1.0;
            for i in 0..=100 {
                let v = curve.apply(i as f64 / 100.0);
                assert!(v >= last - 1e-9, "{curve:?} went backwards at {i}");
                last = v;
            }
        }
    }

    #[test]
    fn ease_out_is_ahead_of_the_line_and_ease_in_behind_it() {
        assert!(Curve::EASE_OUT.apply(0.25) > 0.25);
        assert!(Curve::EASE_IN.apply(0.25) < 0.25);
        assert!((Curve::EASE_IN_OUT.apply(0.5) - 0.5).abs() < 0.02);
    }

    #[test]
    fn steps_hold_and_then_jump() {
        let s = Curve::Steps {
            count: 4,
            position: StepPosition::JumpEnd,
        };
        // jump-end holds each step's value and rises on the boundary:
        // a t of 0.01 is still 0, 0.26 has just stepped to 1/4.
        assert!(s.apply(0.01).abs() < 1e-12);
        assert!((s.apply(0.26) - 0.25).abs() < 1e-12);
        assert!((s.apply(0.51) - 0.5).abs() < 1e-12);
        assert!((s.apply(1.0) - 1.0).abs() < 1e-12);
        let start = Curve::Steps {
            count: 4,
            position: StepPosition::JumpStart,
        };
        // jump-start leads the interval: 0.01 is already the first step.
        assert!((start.apply(0.01) - 0.25).abs() < 1e-12);
        assert!((start.apply(0.26) - 0.5).abs() < 1e-12);
        assert!((start.apply(1.0) - 1.0).abs() < 1e-12);
        // And the first interval starts at 0, so its rise is already in
        // effect there - the CSS spec's `floor(0 * 4) + 1` - just as every
        // interior boundary belongs to the interval that starts on it.
        assert!((start.apply(0.0) - 0.25).abs() < 1e-12);
        assert!((start.apply(0.5) - 0.75).abs() < 1e-12);
        // step-start is steps(1, jump-start): the end state, from 0 on.
        let now = Curve::Steps {
            count: 1,
            position: StepPosition::JumpStart,
        };
        assert!((now.apply(0.0) - 1.0).abs() < 1e-12);
        assert!((now.apply(0.999) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn jump_none_and_jump_both_match_css() {
        let none = Curve::Steps {
            count: 4,
            position: StepPosition::JumpNone,
        };
        // No rise at either end: 0 into the first interval, 1 at the last,
        // and three interior steps of a third each.
        assert!(none.apply(0.0).abs() < 1e-12);
        assert!((none.apply(0.26) - 1.0 / 3.0).abs() < 1e-12);
        assert!((none.apply(1.0) - 1.0).abs() < 1e-12);
        let both = Curve::Steps {
            count: 4,
            position: StepPosition::JumpBoth,
        };
        // A rise at 0 and at 1: five rises of a fifth, starting above 0.
        assert!((both.apply(0.0) - 0.2).abs() < 1e-12);
        assert!((both.apply(0.26) - 0.4).abs() < 1e-12);
        assert!((both.apply(1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_linear_ramp_walks_its_stops() {
        let c = Curve::Points {
            stops: vec![(0.0, 0.0), (0.3, 0.8), (1.0, 1.0)],
        };
        assert!(c.apply(0.15) > 0.39 && c.apply(0.15) < 0.41);
        assert!((c.apply(0.3) - 0.8).abs() < 1e-12);
        assert!((c.apply(0.65) - 0.9).abs() < 0.011);
    }

    #[test]
    fn a_spring_lands_on_one_and_overshoots_under_damped() {
        let s = Curve::SPRING_POP;
        assert!(s.apply(0.0).abs() < 1e-12);
        assert!((s.apply(1.0) - 1.0).abs() < 1e-12);
        let peak = (1..100)
            .map(|i| s.apply(i as f64 / 100.0))
            .fold(0.0_f64, f64::max);
        assert!(
            peak > 1.02,
            "an under-damped spring overshoots: peak {peak}"
        );
        let calm = Curve::SPRING;
        let calm_peak = (1..100)
            .map(|i| calm.apply(i as f64 / 100.0))
            .fold(0.0_f64, f64::max);
        assert!(calm_peak < peak, "more damping overshoots less");
    }

    #[test]
    fn a_critically_damped_spring_does_not_ring() {
        let s = Curve::Spring(Spring {
            stiffness: 200.0,
            damping: 2.0 * (200.0_f64).sqrt(),
            mass: 1.0,
        });
        let mut previous = 0.0;
        for i in 1..200 {
            let v = s.apply(i as f64 / 200.0);
            assert!(v >= previous - 1e-9, "ringing at {i}: {v} after {previous}");
            previous = v;
        }
        // Critically damped: it approaches 1 without crossing it, and the
        // curve is pinned there at the end of the segment rather than left
        // asymptotically short of the next key.
        assert!(previous > 0.99, "settles: {previous}");
        assert!(
            (Curve::Spring(Spring {
                stiffness: 200.0,
                damping: 2.0 * (200.0_f64).sqrt(),
                mass: 1.0
            })
            .apply(1.0)
                - 1.0)
                .abs()
                < 1e-12
        );
    }
}
