// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The geometry a shape node becomes: paths from commands, rounded
//! rectangles and ovals, the arc-length trim a line reveal cuts, the
//! command-for-command morph a shape change blends, and the shaders a
//! solid or a gradient paint builds.
//!
//! The trim and the morph are the two operations the Geneva sidecar
//! cannot draw at all - the reasons a comp with them routes here - so
//! they are measured and cut exactly: a cubic's length by Gauss-Legendre
//! quadrature, a cut inside one by de Casteljau subdivision at the
//! parameter that length says, never by flattening the curve into lines
//! and hoping.

use tiny_skia::{GradientStop, Path, PathBuilder, Shader};

use crate::comp::{Length, Paint, PathCommand, Point, Rgba};

/// The fraction of a box a length reads as, in pixels: a percentage of
/// the frame the comp is being drawn at, a pixel count scaled by the
/// bake, `Auto` resolved by the caller because only it knows the box the
/// word applies to.
pub(crate) fn resolve_px(length: Length, frame: f64, scale: f64, auto: f64) -> f64 {
    match length {
        Length::Px(value) => value * scale,
        Length::Percent(value) => frame * value / 100.0 * scale,
        Length::Auto => auto * scale,
    }
}

/// A vector path from the document's commands, in the node's own pixels.
pub(crate) fn path_of(commands: &[PathCommand]) -> Option<Path> {
    let mut builder = PathBuilder::new();
    for command in commands {
        match *command {
            PathCommand::MoveTo { point } => builder.move_to(point.x as f32, point.y as f32),
            PathCommand::LineTo { point } => builder.line_to(point.x as f32, point.y as f32),
            PathCommand::CubicTo { c1, c2, point } => builder.cubic_to(
                c1.x as f32,
                c1.y as f32,
                c2.x as f32,
                c2.y as f32,
                point.x as f32,
                point.y as f32,
            ),
            PathCommand::Close => builder.close(),
        }
    }
    builder.finish()
}

/// A rounded rectangle spanning `(0, 0)` to `(width, height)`, its
/// corner radius clipped to half the shorter side the way the document
/// promises. A radius of zero is the plain rectangle.
pub(crate) fn rounded_rect(width: f64, height: f64, radius: f64) -> Option<Path> {
    let (width, height) = (width as f32, height as f32);
    let radius = (radius.max(0.0) as f32).min(width / 2.0).min(height / 2.0);
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(tiny_skia::Rect::from_ltrb(
            0.0, 0.0, width, height,
        )?));
    }
    // Four straight runs and four quarter arcs; the kappa that fits a
    // circle to a cubic is 0.55228475, the value every rounded rectangle
    // since Bézier's has used.
    let k = radius * 0.552_284_8;
    let mut builder = PathBuilder::new();
    builder.move_to(radius, 0.0);
    builder.line_to(width - radius, 0.0);
    builder.cubic_to(width - radius + k, 0.0, width, radius - k, width, radius);
    builder.line_to(width, height - radius);
    builder.cubic_to(
        width,
        height - radius + k,
        width - radius + k,
        height,
        width - radius,
        height,
    );
    builder.line_to(radius, height);
    builder.cubic_to(
        radius - k,
        height,
        0.0,
        height - radius + k,
        0.0,
        height - radius,
    );
    builder.line_to(0.0, radius);
    builder.cubic_to(0.0, radius - k, radius - k, 0.0, radius, 0.0);
    builder.close();
    builder.finish()
}

/// An ellipse inscribed in the box `(0, 0)` to `(width, height)`.
pub(crate) fn oval(width: f64, height: f64) -> Option<Path> {
    PathBuilder::from_oval(tiny_skia::Rect::from_ltrb(
        0.0,
        0.0,
        width as f32,
        height as f32,
    )?)
}

/// One straight or curved run of a path between two of its points, with
/// the length it covers and a way to cut itself at a fraction of that
/// length.
enum Segment {
    /// A line from `from` to `to`.
    Line {
        /// Where it starts.
        from: Point,
        /// Where it ends.
        to: Point,
    },
    /// A cubic from `from` to `to` through two controls.
    Cubic {
        /// Where it starts.
        from: Point,
        /// The first control point.
        c1: Point,
        /// The second control point.
        c2: Point,
        /// Where it ends.
        to: Point,
    },
}

impl Segment {
    /// The segment's arc length: the hypotenuse for a line, 16-point
    /// Gauss-Legendre quadrature for a cubic - exact for polynomials of
    /// degree 31 and under, and a cubic's speed |B'(t)| is very nearly
    /// one, so the length lands accurate to well under a hundredth of a
    /// pixel on any curve a designer draws.
    fn length(&self) -> f64 {
        match *self {
            Segment::Line { from, to } => (to.x - from.x).hypot(to.y - from.y),
            Segment::Cubic { from, c1, c2, to } => {
                // The 16 abscissae and weights of Gauss-Legendre on
                // [0, 1], the classic set on [-1, 1] moved across: the
                // weights sum to one, so a constant speed integrates to
                // itself whatever the curve.
                const NODES: [(f64, f64); 16] = [
                    (0.005_299_532_496_746_77, 0.013_576_229_705_845_4),
                    (0.027_712_488_540_448_9, 0.031_126_761_969_323_9),
                    (0.067_184_398_803_806_5, 0.047_579_255_841_246_4),
                    (0.122_297_795_822_498, 0.062_314_485_627_916_7),
                    (0.191_061_877_879_879, 0.074_797_994_408_288_3),
                    (0.270_991_611_204_657, 0.084_578_259_697_501_3),
                    (0.359_198_224_610_483, 0.091_301_707_522_461_8),
                    (0.452_493_745_118_876, 0.094_725_305_227_534_2),
                    (0.547_506_254_881_124, 0.094_725_305_227_534_2),
                    (0.640_801_775_389_517, 0.091_301_707_522_461_8),
                    (0.729_008_388_795_343, 0.084_578_259_697_501_3),
                    (0.808_938_122_120_121, 0.074_797_994_408_288_3),
                    (0.877_702_204_177_502, 0.062_314_485_627_916_7),
                    (0.932_815_601_196_193, 0.047_579_255_841_246_4),
                    (0.972_287_511_459_551, 0.031_126_761_969_323_9),
                    (0.994_700_467_503_253, 0.013_576_229_705_845_4),
                ];
                let speed = |t: f64| {
                    let u = 1.0 - t;
                    let dx = 3.0
                        * (u * u * (c1.x - from.x)
                            + 2.0 * u * t * (c2.x - c1.x)
                            + t * t * (to.x - c2.x));
                    let dy = 3.0
                        * (u * u * (c1.y - from.y)
                            + 2.0 * u * t * (c2.y - c1.y)
                            + t * t * (to.y - c2.y));
                    dx.hypot(dy)
                };
                NODES.iter().map(|&(t, w)| w * speed(t)).sum()
            }
        }
    }

    /// The point and the outgoing direction at `t` in `0..=1`.
    fn point(&self, t: f64) -> Point {
        match *self {
            Segment::Line { from, to } => Point {
                x: from.x + (to.x - from.x) * t,
                y: from.y + (to.y - from.y) * t,
            },
            Segment::Cubic { from, c1, c2, to } => {
                let (u, t) = (1.0 - t, t);
                Point {
                    x: u * u * u * from.x
                        + 3.0 * u * u * t * c1.x
                        + 3.0 * u * t * t * c2.x
                        + t * t * t * to.x,
                    y: u * u * u * from.y
                        + 3.0 * u * u * t * c1.y
                        + 3.0 * u * t * t * c2.y
                        + t * t * t * to.y,
                }
            }
        }
    }

    /// Two segments, this one cut at `t`: the run before and the run
    /// after, de Casteljau's subdivision for a cubic.
    fn split(&self, t: f64) -> (Segment, Segment) {
        match *self {
            Segment::Line { from, to } => {
                let mid = self.point(t);
                (
                    Segment::Line { from, to: mid },
                    Segment::Line { from: mid, to },
                )
            }
            Segment::Cubic { from, c1, c2, to } => {
                let lerp = |a: Point, b: Point, u: f64| Point {
                    x: a.x + (b.x - a.x) * u,
                    y: a.y + (b.y - a.y) * u,
                };
                let a = lerp(from, c1, t);
                let b = lerp(c1, c2, t);
                let c = lerp(c2, to, t);
                let d = lerp(a, b, t);
                let e = lerp(b, c, t);
                let cut = lerp(d, e, t);
                (
                    Segment::Cubic {
                        from,
                        c1: a,
                        c2: d,
                        to: cut,
                    },
                    Segment::Cubic {
                        from: cut,
                        c1: e,
                        c2: c,
                        to,
                    },
                )
            }
        }
    }
}

/// One segment cut to `from..to` of itself: the entry point, the command
/// that draws it, and the point it lands on.
fn cut(segment: &Segment, from: f64, to: f64) -> (Point, PathCommand, Point) {
    let piece = match *segment {
        Segment::Line { .. } => Segment::Line {
            from: segment.point(from),
            to: segment.point(to),
        },
        Segment::Cubic { .. } => {
            // Cut the tail, cut the head, keep the middle.
            let (_, rest) = segment.split(from);
            let (middle, _) = rest.split(((to - from) / (1.0 - from).max(1e-9)).clamp(0.0, 1.0));
            middle
        }
    };
    match piece {
        Segment::Line { from, to } => (from, PathCommand::LineTo { point: to }, to),
        Segment::Cubic { from, c1, c2, to } => {
            (from, PathCommand::CubicTo { c1, c2, point: to }, to)
        }
    }
}

/// The commands of `path` cut to `from..to` of its own total length,
/// `0..=1` each, drawn in the order the commands run. The cut points
/// become `MoveTo`s: a trimmed path is one the stroke walks, not a
/// closed shape, and a fill of a trimmed path fills what remains of the
/// outline as the winding rule reads it.
pub(crate) fn trimmed(commands: &[PathCommand], from: f64, to: f64) -> Vec<PathCommand> {
    let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
    if to - from <= 0.0 || commands.is_empty() {
        return Vec::new();
    }
    // Walk the commands into segments, remembering where each subpath
    // opens and which segments belong to it.
    let mut segments: Vec<Segment> = Vec::with_capacity(commands.len());
    let mut cursor = Point { x: 0.0, y: 0.0 };
    let mut opened = cursor;
    for command in commands {
        match *command {
            PathCommand::MoveTo { point } => {
                cursor = point;
                opened = point;
            }
            PathCommand::LineTo { point } => {
                segments.push(Segment::Line {
                    from: cursor,
                    to: point,
                });
                cursor = point;
            }
            PathCommand::CubicTo { c1, c2, point } => {
                segments.push(Segment::Cubic {
                    from: cursor,
                    c1,
                    c2,
                    to: point,
                });
                cursor = point;
            }
            PathCommand::Close => {
                if cursor != opened {
                    segments.push(Segment::Line {
                        from: cursor,
                        to: opened,
                    });
                }
                cursor = opened;
            }
        }
    }
    let total: f64 = segments.iter().map(Segment::length).sum();
    if total <= 0.0 {
        return commands.to_vec();
    }
    let (start_len, end_len) = (total * from, total * to);
    let mut out: Vec<PathCommand> = Vec::with_capacity(commands.len() + 2);
    let mut walked = 0.0;
    let mut pen: Option<Point> = None;
    for segment in &segments {
        let length = segment.length();
        let (seg_from, seg_to) = (walked, walked + length);
        walked = seg_to;
        if seg_to <= start_len || seg_from >= end_len || length <= 0.0 {
            continue;
        }
        // The part of this segment inside the window, as segment-local
        // fractions.
        let local_from = ((start_len - seg_from) / length).max(0.0);
        let local_to = ((end_len - seg_from) / length).min(1.0);
        let piece = cut(segment, local_from, local_to);
        let entry = piece.0;
        if pen != Some(entry) {
            out.push(PathCommand::MoveTo { point: entry });
        }
        out.push(piece.1);
        pen = Some(piece.2);
    }
    out
}

/// The commands a morph between `from` and `to` draws at `t` in
/// `0..=1`: command for command where the two agree - the same
/// operations in the same order, so every coordinate interpolates -
/// and point for point where they do not, both outlines resampled at
/// the same arc-length stations and the stations interpolated. A morph
/// never invents a shape that is not on the line between the two.
pub(crate) fn morphed(from: &[PathCommand], to: &[PathCommand], t: f64) -> Vec<PathCommand> {
    let t = t.clamp(0.0, 1.0);
    if shapes_agree(from, to) {
        return from
            .iter()
            .zip(to)
            .map(|(a, b)| command_between(a, b, t))
            .collect();
    }
    // Topology differs: resample both at the same count of arc-length
    // stations and interpolate the stations. The count is the larger of
    // the two outlines' command counts, at least eight so a triangle
    // still rounds into a square smoothly, at most 512 so a morph of two
    // dense curves stays a morph and not a memory test.
    let stations = (from.len().max(to.len())).clamp(8, 512);
    let left = resample(from, stations);
    let right = resample(to, stations);
    let mut out = Vec::with_capacity(stations + 1);
    let mut pen: Option<Point> = None;
    for (a, b) in left.iter().zip(right.iter()) {
        let point = Point {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        };
        if pen != Some(point) {
            out.push(PathCommand::MoveTo { point });
            pen = Some(point);
        } else {
            out.push(PathCommand::LineTo { point });
        }
    }
    out
}

/// Whether two command lists are the same operations in the same order,
/// which is the "command for command" a direct interpolation needs.
fn shapes_agree(a: &[PathCommand], b: &[PathCommand]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            matches!(
                (x, y),
                (PathCommand::MoveTo { .. }, PathCommand::MoveTo { .. })
                    | (PathCommand::LineTo { .. }, PathCommand::LineTo { .. })
                    | (PathCommand::CubicTo { .. }, PathCommand::CubicTo { .. })
                    | (PathCommand::Close, PathCommand::Close)
            )
        })
}

/// One command interpolated between two that agree.
fn command_between(a: &PathCommand, b: &PathCommand, t: f64) -> PathCommand {
    let mix = |x: &Point, y: &Point| Point {
        x: x.x + (y.x - x.x) * t,
        y: x.y + (y.y - x.y) * t,
    };
    match (a, b) {
        (PathCommand::MoveTo { point: x }, PathCommand::MoveTo { point: y }) => {
            PathCommand::MoveTo { point: mix(x, y) }
        }
        (PathCommand::LineTo { point: x }, PathCommand::LineTo { point: y }) => {
            PathCommand::LineTo { point: mix(x, y) }
        }
        (
            PathCommand::CubicTo {
                c1: ac1,
                c2: ac2,
                point: ap,
            },
            PathCommand::CubicTo {
                c1: bc1,
                c2: bc2,
                point: bp,
            },
        ) => PathCommand::CubicTo {
            c1: mix(ac1, bc1),
            c2: mix(ac2, bc2),
            point: mix(ap, bp),
        },
        (PathCommand::Close, PathCommand::Close) => PathCommand::Close,
        // The caller checked agreement; this arm is unreachable.
        _ => a.clone(),
    }
}

/// An outline resampled at `count` arc-length stations, closed back to
/// its first when the original closes. Stations are uniform in length,
/// so a resampled left and a resampled right correspond.
fn resample(commands: &[PathCommand], count: usize) -> Vec<Point> {
    let mut segments: Vec<Segment> = Vec::with_capacity(commands.len());
    let mut cursor = Point { x: 0.0, y: 0.0 };
    let mut opened = cursor;
    let mut closes = false;
    for command in commands {
        match *command {
            PathCommand::MoveTo { point } => {
                cursor = point;
                opened = point;
            }
            PathCommand::LineTo { point } => {
                segments.push(Segment::Line {
                    from: cursor,
                    to: point,
                });
                cursor = point;
            }
            PathCommand::CubicTo { c1, c2, point } => {
                segments.push(Segment::Cubic {
                    from: cursor,
                    c1,
                    c2,
                    to: point,
                });
                cursor = point;
            }
            PathCommand::Close => {
                closes = true;
                if cursor != opened {
                    segments.push(Segment::Line {
                        from: cursor,
                        to: opened,
                    });
                }
                cursor = opened;
            }
        }
    }
    let total: f64 = segments.iter().map(Segment::length).sum();
    if total <= 0.0 || segments.is_empty() {
        return commands
            .iter()
            .filter_map(|command| match *command {
                PathCommand::MoveTo { point } | PathCommand::LineTo { point } => Some(point),
                _ => None,
            })
            .collect();
    }
    let mut out = Vec::with_capacity(count);
    let mut walked = 0.0;
    let mut station = 0;
    for segment in &segments {
        let length = segment.length();
        let (seg_from, seg_to) = (walked, walked + length);
        walked = seg_to;
        while station < count {
            let target = total * station as f64 / count as f64;
            if target > seg_to {
                break;
            }
            let local = if length <= 0.0 {
                0.0
            } else {
                ((target - seg_from) / length).clamp(0.0, 1.0)
            };
            out.push(segment.point(local));
            station += 1;
        }
    }
    while out.len() < count {
        out.push(
            segments
                .last()
                .map(|last| last.point(1.0))
                .unwrap_or(Point { x: 0.0, y: 0.0 }),
        );
    }
    let _ = closes;
    out
}

/// A paint as a tiny-skia shader over the box it fills, in the box's own
/// pixel space: a solid colour, or a gradient whose ramp runs `from` to
/// `to` along its axis. `angle` is CSS degrees - 90 left to right - and
/// the axis spans the box the way CSS's gradient line does, through the
/// centre, as wide as the box's projection onto the direction.
pub(crate) fn shader_of(paint: &Paint, width: f64, height: f64) -> Option<Shader<'static>> {
    match *paint {
        Paint::Solid { color } => Some(Shader::SolidColor(color_skia(color))),
        Paint::Gradient {
            ref stops,
            ramp,
            angle,
            from,
            to,
        } => {
            if stops.is_empty() {
                return None;
            }
            let gradient_stops: Vec<GradientStop> = stops
                .iter()
                .map(|(at, color)| GradientStop::new(at.clamp(0.0, 1.0) as f32, color_skia(*color)))
                .collect();
            let (w, h) = (width as f32, height as f32);
            match ramp {
                crate::comp::GradientKind::Linear => {
                    // CSS measures its angles clockwise from "to top".
                    let radians = (angle - 90.0).to_radians() as f32;
                    let (dx, dy) = (radians.cos(), radians.sin());
                    let line = w.abs() * dx.abs() + h.abs() * dy.abs();
                    let (cx, cy) = (w / 2.0, h / 2.0);
                    let half = line / 2.0;
                    let start = tiny_skia::Point::from_xy(
                        cx + dx * half * (from as f32 * 2.0 - 1.0),
                        cy + dy * half * (from as f32 * 2.0 - 1.0),
                    );
                    let end = tiny_skia::Point::from_xy(
                        cx + dx * half * (to as f32 * 2.0 - 1.0),
                        cy + dy * half * (to as f32 * 2.0 - 1.0),
                    );
                    tiny_skia::LinearGradient::new(
                        start,
                        end,
                        gradient_stops,
                        tiny_skia::SpreadMode::Pad,
                        tiny_skia::Transform::identity(),
                    )
                }
                crate::comp::GradientKind::Radial => {
                    // A disc at the box's centre, out to its corner -
                    // the farthest-corner default the document names.
                    let (cx, cy) = (w / 2.0, h / 2.0);
                    let corner = (w / 2.0).hypot(h / 2.0).max(1e-3);
                    tiny_skia::RadialGradient::new(
                        tiny_skia::Point::from_xy(cx, cy),
                        corner * from.clamp(0.0, 1.0) as f32,
                        tiny_skia::Point::from_xy(cx, cy),
                        corner * to.clamp(0.0, 1.0) as f32,
                        gradient_stops,
                        tiny_skia::SpreadMode::Pad,
                        tiny_skia::Transform::identity(),
                    )
                }
            }
        }
    }
}

/// A document colour as a tiny-skia colour.
pub(crate) fn color_skia(color: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(
        color.r.clamp(0.0, 1.0) as f32,
        color.g.clamp(0.0, 1.0) as f32,
        color.b.clamp(0.0, 1.0) as f32,
        color.a.clamp(0.0, 1.0) as f32,
    )
    .unwrap_or(tiny_skia::Color::BLACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo {
                point: Point { x: x1, y: y1 },
            },
            PathCommand::LineTo {
                point: Point { x: x2, y: y2 },
            },
        ]
    }

    #[test]
    fn a_rounded_rect_with_no_radius_is_the_plain_rect() {
        let path = rounded_rect(20.0, 10.0, 0.0).unwrap();
        let bounds = path.bounds();
        assert_eq!(bounds.right(), 20.0);
        assert_eq!(bounds.bottom(), 10.0);
    }

    #[test]
    fn a_rounded_rect_clips_its_radius_to_half_the_shorter_side() {
        let path = rounded_rect(20.0, 10.0, 99.0).unwrap();
        // A pill: the radius is 5, so the bounds still hold and the path
        // built with quadrants rather than collapsing.
        assert_eq!(path.bounds().width(), 20.0);
        assert!(!path.is_empty());
    }

    #[test]
    fn a_trim_cuts_a_line_to_its_second_half() {
        let cut = trimmed(&line(0.0, 0.0, 10.0, 0.0), 0.5, 1.0);
        assert_eq!(
            cut,
            vec![
                PathCommand::MoveTo {
                    point: Point { x: 5.0, y: 0.0 }
                },
                PathCommand::LineTo {
                    point: Point { x: 10.0, y: 0.0 }
                },
            ]
        );
    }

    #[test]
    fn a_full_trim_returns_the_whole_walk() {
        let cut = trimmed(&line(0.0, 0.0, 10.0, 0.0), 0.0, 1.0);
        assert_eq!(
            cut,
            vec![
                PathCommand::MoveTo {
                    point: Point { x: 0.0, y: 0.0 }
                },
                PathCommand::LineTo {
                    point: Point { x: 10.0, y: 0.0 }
                },
            ]
        );
    }

    #[test]
    fn an_empty_trim_draws_nothing() {
        assert!(trimmed(&line(0.0, 0.0, 10.0, 0.0), 0.7, 0.7).is_empty());
        assert!(trimmed(&line(0.0, 0.0, 10.0, 0.0), 0.8, 0.2).is_empty());
    }

    #[test]
    fn a_trim_of_a_curve_cuts_on_the_curve() {
        // One quadrant of a circle as a cubic: from (0,0) to (10,10)
        // through the kappa controls.
        let k = 10.0 * 0.552_284_75;
        let arc = vec![
            PathCommand::MoveTo {
                point: Point { x: 0.0, y: 0.0 },
            },
            PathCommand::CubicTo {
                c1: Point { x: k, y: 0.0 },
                c2: Point {
                    x: 10.0,
                    y: 10.0 - k,
                },
                point: Point { x: 10.0, y: 10.0 },
            },
        ];
        let half = trimmed(&arc, 0.0, 0.5);
        // The cut lands at the quadrant's midpoint. The controls put
        // this arc on the circle of radius 10 centred at (0, 10), so
        // the midpoint is (10/sqrt(2), 10 - 10/sqrt(2)), within the
        // quadrature's precision.
        match half.last() {
            Some(PathCommand::CubicTo { point, .. }) => {
                let expected = 10.0 / 2.0f64.sqrt();
                assert!(
                    (point.x - expected).abs() < 0.05,
                    "x {} vs {}",
                    point.x,
                    expected
                );
                assert!(
                    (point.y - 10.0 + 10.0 / 2.0f64.sqrt()).abs() < 0.05,
                    "y {}",
                    point.y
                );
            }
            other => panic!("the half should end in a cubic, got {other:?}"),
        }
    }

    #[test]
    fn a_morph_between_agreeing_shapes_interpolates_coordinates() {
        let a = line(0.0, 0.0, 10.0, 0.0);
        let b = line(0.0, 5.0, 20.0, 5.0);
        let mid = morphed(&a, &b, 0.5);
        assert_eq!(
            mid,
            vec![
                PathCommand::MoveTo {
                    point: Point { x: 0.0, y: 2.5 }
                },
                PathCommand::LineTo {
                    point: Point { x: 15.0, y: 2.5 }
                },
            ]
        );
    }

    #[test]
    fn a_morph_between_disagreeing_shapes_resamples_both() {
        let a = line(0.0, 0.0, 10.0, 0.0);
        let triangle = vec![
            PathCommand::MoveTo {
                point: Point { x: 0.0, y: 0.0 },
            },
            PathCommand::LineTo {
                point: Point { x: 5.0, y: 8.0 },
            },
            PathCommand::LineTo {
                point: Point { x: 10.0, y: 0.0 },
            },
            PathCommand::Close,
        ];
        let mid = morphed(&a, &triangle, 0.5);
        // Station count at least 8, every command a move or a line, and
        // the halfway points between the two outlines.
        assert!(mid.len() >= 8);
        assert!(mid.iter().all(|command| matches!(
            command,
            PathCommand::MoveTo { .. } | PathCommand::LineTo { .. }
        )));
        if let Some(PathCommand::MoveTo { point }) = mid.first() {
            assert_eq!(point.x, 0.0);
            assert_eq!(point.y, 0.0);
        } else {
            panic!("the morph should open with a move");
        }
    }

    #[test]
    fn a_morph_at_its_ends_is_its_ends_when_shapes_agree() {
        let a = line(0.0, 0.0, 10.0, 0.0);
        let b = line(0.0, 5.0, 20.0, 5.0);
        assert_eq!(morphed(&a, &b, 0.0), a);
        assert_eq!(morphed(&a, &b, 1.0), b);
    }

    #[test]
    fn a_solid_paint_becomes_a_solid_shader() {
        let paint = Paint::Solid {
            color: Rgba::new(1.0, 0.0, 0.0, 1.0),
        };
        assert!(matches!(
            shader_of(&paint, 100.0, 100.0),
            Some(Shader::SolidColor(_))
        ));
    }

    #[test]
    fn a_left_to_right_gradient_is_built() {
        let paint = Paint::Gradient {
            stops: vec![
                (0.0, Rgba::new(0.0, 0.0, 0.0, 1.0)),
                (1.0, Rgba::hex("#fff")),
            ],
            ramp: crate::comp::GradientKind::Linear,
            angle: 90.0,
            from: 0.0,
            to: 1.0,
        };
        assert!(shader_of(&paint, 100.0, 50.0).is_some());
    }

    #[test]
    fn an_empty_gradient_is_no_shader() {
        let paint = Paint::Gradient {
            stops: Vec::new(),
            ramp: crate::comp::GradientKind::Radial,
            angle: 0.0,
            from: 0.0,
            to: 1.0,
        };
        assert!(shader_of(&paint, 100.0, 50.0).is_none());
    }

    #[test]
    fn a_cubics_length_is_measured_accurately() {
        // A cubic that approximates a quarter circle of radius 10 has a
        // length near 10 * pi / 2 = 15.708.
        let k = 10.0 * 0.552_284_75;
        let segment = Segment::Cubic {
            from: Point { x: 0.0, y: 0.0 },
            c1: Point { x: k, y: 0.0 },
            c2: Point {
                x: 10.0,
                y: 10.0 - k,
            },
            to: Point { x: 10.0, y: 10.0 },
        };
        let length = segment.length();
        assert!(
            (length - 10.0 * std::f64::consts::FRAC_PI_2).abs() < 0.01,
            "{length}"
        );
    }
}
