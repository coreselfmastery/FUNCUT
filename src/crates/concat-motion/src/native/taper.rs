//! Variable-width strokes: the tapered ribbon.
//!
//! A stroke whose width is not one number but a profile along the
//! outline - a pin tip at one end, full width at the other, an arrow
//! head where it lands. The outline is flattened into a polyline, each
//! sample is offset along its normal by half the width it carries, and
//! the two sides close into one filled polygon: a ribbon, drawn as
//! fill rather than stroke because no stroke has a width that changes
//! as it walks.

use crate::comp::{ArrowHead, PathCommand, Point, StrokeTaper};

/// One sample of the flattened centreline.
struct Sample {
    /// The point, in the node's own pixels.
    point: Point,
    /// The unit normal at the sample.
    normal: (f64, f64),
    /// The fraction of the walked length reached at the sample, `0..=1`.
    along: f64,
}

/// Flattens the commands into samples with normals and arc lengths.
fn flatten(commands: &[PathCommand]) -> Vec<Sample> {
    let mut samples: Vec<Sample> = Vec::new();
    let mut cursor = Point::new(0.0, 0.0);
    let mut walked = 0.0;
    let push = |from: Point, to: Point, samples: &mut Vec<Sample>, walked: &mut f64| {
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let length = dx.hypot(dy);
        if length < 1e-9 {
            return;
        }
        *walked += length;
        let normal = (-dy / length, dx / length);
        samples.push(Sample {
            point: to,
            normal,
            along: 0.0,
        });
    };
    for command in commands {
        match *command {
            PathCommand::MoveTo { point } => {
                cursor = point;
                if samples.is_empty() {
                    samples.push(Sample {
                        point,
                        normal: (0.0, 1.0),
                        along: 0.0,
                    });
                }
            }
            PathCommand::LineTo { point } => {
                push(cursor, point, &mut samples, &mut walked);
                cursor = point;
            }
            PathCommand::CubicTo { c1, c2, point } => {
                let from = cursor;
                for i in 1..=32u32 {
                    let t = f64::from(i) / 32.0;
                    let one = 1.0 - t;
                    let at = |p: Point| Point {
                        x: one * one * one * p.x
                            + 3.0 * one * one * t * c1.x
                            + 3.0 * one * t * t * c2.x
                            + t * t * t * point.x,
                        y: one * one * one * p.y
                            + 3.0 * one * one * t * c1.y
                            + 3.0 * one * t * t * c2.y
                            + t * t * t * point.y,
                    };
                    let to = at(from);
                    push(cursor, to, &mut samples, &mut walked);
                    cursor = to;
                }
            }
            PathCommand::Close => {}
        }
    }
    let total = samples.last().map(|_| walked).unwrap_or(0.0).max(1e-9);
    let mut carried = 0.0;
    let mut previous = Point::new(0.0, 0.0);
    for (i, sample) in samples.iter_mut().enumerate() {
        if i > 0 {
            carried += (sample.point.x - previous.x).hypot(sample.point.y - previous.y);
        }
        previous = sample.point;
        sample.along = (carried / total).clamp(0.0, 1.0);
    }
    samples
}

/// The width multiplier a profile gives at a fraction of the walk: the
/// ends' answers eased through the middle, so a taper swells like ink
/// and not like a ruler.
fn scale_at(taper: &StrokeTaper, along: f64) -> f64 {
    // Cosine ease between the two ends: smooth at both, monotone
    // between, and exactly `start` and `end` at the ends themselves.
    let eased = 0.5 - 0.5 * (std::f64::consts::PI * along).cos();
    taper.start + (taper.end - taper.start) * eased
}

/// The arrow head's triangle, appended to the ribbon as its own closed
/// subpath: a point past the outline's end, flanked back along the
/// tangent.
fn arrow_head(end: Point, tangent: (f64, f64), width: f64, head: &ArrowHead) -> Vec<Point> {
    let length = width.max(1e-3) * head.length.max(0.0);
    let half = width.max(1e-3) * head.width.max(0.0) * 0.5;
    let (tx, ty) = tangent;
    let normal = (-ty, tx);
    let tip = Point::new(end.x + tx * length, end.y + ty * length);
    let flank = |side: f64| Point {
        x: end.x + normal.0 * half * side,
        y: end.y + normal.1 * half * side,
    };
    vec![tip, flank(1.0), flank(-1.0)]
}

/// Builds the tapered stroke's ribbon: one closed polygon that fills
/// where a constant stroke would paint, with the width the profile
/// gives at each step, plus the arrow head when it carries one.
///
/// Returns the polygon's outline as commands, ready for the shape
/// raster; `None` when the walk has no length to carry a ribbon, or
/// the profile's every answer is zero.
pub fn ribbon(
    commands: &[PathCommand],
    width: f64,
    taper: &StrokeTaper,
) -> Option<Vec<PathCommand>> {
    let samples = flatten(commands);
    if samples.len() < 2 {
        return None;
    }
    let half = |along: f64| width * scale_at(taper, along).max(0.0) * 0.5;
    // Both sides of the walk: the left side forward, the right side
    // back, closed into one polygon.
    let mut left = Vec::with_capacity(samples.len());
    let mut right = Vec::with_capacity(samples.len());
    for sample in &samples {
        let h = half(sample.along);
        left.push(Point::new(
            sample.point.x + sample.normal.0 * h,
            sample.point.y + sample.normal.1 * h,
        ));
        right.push(Point::new(
            sample.point.x - sample.normal.0 * h,
            sample.point.y - sample.normal.1 * h,
        ));
    }
    if left
        .iter()
        .zip(&right)
        .all(|(l, r)| (l.x - r.x).abs() < 1e-9 && (l.y - r.y).abs() < 1e-9)
    {
        return None;
    }
    let mut outline = Vec::with_capacity(left.len() + right.len() + 4);
    let first = left[0];
    outline.push(PathCommand::MoveTo { point: first });
    for point in left.iter().skip(1) {
        outline.push(PathCommand::LineTo { point: *point });
    }
    for point in right.iter().skip(1).rev() {
        outline.push(PathCommand::LineTo { point: *point });
    }
    outline.push(PathCommand::Close);
    // The arrow head rides as its own subpath: the ribbon's end and the
    // tangent there are the head's own start.
    if let Some(head) = &taper.arrow {
        let end = samples[samples.len() - 1].point;
        let before = samples[samples.len() - 2].point;
        let (dx, dy) = (end.x - before.x, end.y - before.y);
        let length = dx.hypot(dy);
        if length > 1e-9 {
            let triangle = arrow_head(end, (dx / length, dy / length), width, head);
            outline.push(PathCommand::MoveTo { point: triangle[0] });
            outline.push(PathCommand::LineTo { point: triangle[1] });
            outline.push(PathCommand::LineTo { point: triangle[2] });
            outline.push(PathCommand::Close);
        }
    }
    Some(outline)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(from: (f64, f64), to: (f64, f64)) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo {
                point: Point::new(from.0, from.1),
            },
            PathCommand::LineTo {
                point: Point::new(to.0, to.1),
            },
        ]
    }

    /// A taper that starts at nothing and ends at full width keeps its
    /// tip a point and its tail the whole width.
    #[test]
    fn a_taper_grows_along_the_walk() {
        let taper = StrokeTaper {
            start: 0.0,
            end: 1.0,
            arrow: None,
        };
        let ribbon = ribbon(&line((0.0, 0.0), (100.0, 0.0)), 10.0, &taper).unwrap();
        // The polygon's own bounds: y spans -5..=5 at the fat end, and
        // the first MoveTo is the tip itself.
        let mut widths: Vec<f64> = Vec::new();
        for command in &ribbon {
            if let PathCommand::MoveTo { point } | PathCommand::LineTo { point } = command {
                widths.push(point.y.abs());
            }
        }
        let (first, last) = (widths[0], widths[widths.len() - 1]);
        assert!(first < 0.1, "{ribbon:?}");
        assert!((last - 5.0).abs() < 0.1, "{ribbon:?}");
    }

    /// The arrow head adds its own subpath past the end: a tip farther
    /// along the tangent than the ribbon's own tail.
    #[test]
    fn an_arrow_head_points_past_the_end() {
        let taper = StrokeTaper {
            start: 1.0,
            end: 1.0,
            arrow: Some(ArrowHead {
                length: 2.0,
                width: 3.0,
            }),
        };
        let ribbon = ribbon(&line((0.0, 0.0), (100.0, 0.0)), 4.0, &taper).unwrap();
        let far = ribbon
            .iter()
            .filter_map(|command| match command {
                PathCommand::MoveTo { point } | PathCommand::LineTo { point } => Some(point.x),
                PathCommand::CubicTo { point, .. } => Some(point.x),
                PathCommand::Close => None,
            })
            .fold(f64::MIN, f64::max);
        // 100 + 4 * 2 = 108: the head's tip.
        assert!((far - 108.0).abs() < 1e-6, "{ribbon:?}");
    }

    /// A profile that is zero everywhere has no ribbon to draw.
    #[test]
    fn a_zero_profile_draws_nothing() {
        let taper = StrokeTaper {
            start: 0.0,
            end: 0.0,
            arrow: None,
        };
        assert!(ribbon(&line((0.0, 0.0), (100.0, 0.0)), 10.0, &taper).is_none());
    }

    /// A walk of one point carries no ribbon.
    #[test]
    fn a_point_carries_nothing() {
        let taper = StrokeTaper {
            start: 1.0,
            end: 1.0,
            arrow: None,
        };
        let point = vec![PathCommand::MoveTo {
            point: Point::new(0.0, 0.0),
        }];
        assert!(ribbon(&point, 10.0, &taper).is_none());
    }
}
