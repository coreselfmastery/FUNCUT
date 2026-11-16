//! Boolean operations on outlines, through i_overlay.
//!
//! Two shapes become one: the union, difference, intersection or
//! exclusive-or of a path and a second outline, the way a logo mark is
//! cut from two shapes. The commands flatten to polylines - lines as
//! they are, cubics as thirty-two steps - and the overlay runs on an
//! exact integer grid, so the same two outlines always produce the
//! same answer, frame after frame. What comes back is polylines too:
//! `move-to`, `line-to`s and a `close`, as many subpaths as the result
//! has contours.

use crate::comp::{BooleanOp, PathCommand, Point};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

/// A comp point as the float point the overlay reads.
fn as_xy(point: Point) -> [f64; 2] {
    [point.x, point.y]
}

/// Flattens commands into closed contours: one contour per subpath,
/// curves stepped finer than a pixel of any path a logo walks.
fn flatten(commands: &[PathCommand]) -> Vec<Vec<[f64; 2]>> {
    let mut contours: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut contour: Vec<[f64; 2]> = Vec::new();
    let mut cursor = Point::new(0.0, 0.0);
    let push_contour = |contour: &mut Vec<[f64; 2]>, contours: &mut Vec<Vec<[f64; 2]>>| {
        // A contour needs three points to enclose anything.
        if contour.len() >= 3 {
            contours.push(contour.clone());
        }
        contour.clear();
    };
    for command in commands {
        match *command {
            PathCommand::MoveTo { point } => {
                push_contour(&mut contour, &mut contours);
                cursor = point;
                contour.push(as_xy(point));
            }
            PathCommand::LineTo { point } => {
                cursor = point;
                contour.push(as_xy(point));
            }
            PathCommand::CubicTo { c1, c2, point } => {
                let from = cursor;
                for i in 1..=32u32 {
                    let t = f64::from(i) / 32.0;
                    let one = 1.0 - t;
                    contour.push([
                        one * one * one * from.x
                            + 3.0 * one * one * t * c1.x
                            + 3.0 * one * t * t * c2.x
                            + t * t * t * point.x,
                        one * one * one * from.y
                            + 3.0 * one * one * t * c1.y
                            + 3.0 * one * t * t * c2.y
                            + t * t * t * point.y,
                    ]);
                }
                cursor = point;
            }
            PathCommand::Close => {
                push_contour(&mut contour, &mut contours);
            }
        }
    }
    push_contour(&mut contour, &mut contours);
    contours
}

/// The signed area one contour encloses, by the shoelace: positive
/// counter-clockwise, negative clockwise, zero for nothing.
#[cfg(test)]
fn signed_area(contour: &[[f64; 2]]) -> f64 {
    let mut area = 0.0;
    for (i, point) in contour.iter().enumerate() {
        let next = contour[(i + 1) % contour.len()];
        area += point[0] * next[1] - next[0] * point[1];
    }
    area / 2.0
}

/// Combines two outlines into one: `subject` against `operand`, by the
/// operation named. An outline with no contour that encloses anything
/// yields the other unchanged, so an empty operand is a no-op rather
/// than a hole.
pub fn combine(
    subject: &[PathCommand],
    operand: &[PathCommand],
    op: BooleanOp,
) -> Vec<PathCommand> {
    let (subj, clip) = (flatten(subject), flatten(operand));
    if subj.is_empty() {
        return operand.to_vec();
    }
    if clip.is_empty() {
        return subject.to_vec();
    }
    let rule = match op {
        BooleanOp::Union => OverlayRule::Union,
        BooleanOp::Subtract => OverlayRule::Difference,
        BooleanOp::Intersect => OverlayRule::Intersect,
        BooleanOp::Xor => OverlayRule::Xor,
    };
    // The i64 engine: comp pixels are small numbers and the grid it
    // scales to keeps every corner the answer carries.
    let shapes = subj.overlay_as::<i64>(&clip, rule, FillRule::NonZero);
    let mut commands = Vec::new();
    for shape in shapes {
        for contour in shape {
            if contour.len() < 3 {
                continue;
            }
            commands.push(PathCommand::MoveTo {
                point: Point::new(contour[0][0], contour[0][1]),
            });
            for point in contour.iter().skip(1) {
                commands.push(PathCommand::LineTo {
                    point: Point::new(point[0], point[1]),
                });
            }
            commands.push(PathCommand::Close);
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(at: f64, size: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo {
                point: Point::new(at, at),
            },
            PathCommand::LineTo {
                point: Point::new(at + size, at),
            },
            PathCommand::LineTo {
                point: Point::new(at + size, at + size),
            },
            PathCommand::LineTo {
                point: Point::new(at, at + size),
            },
            PathCommand::Close,
        ]
    }

    /// The area the result covers, holes counted out: contours of
    /// opposite winding cancel, the measure every operation's answer is
    /// checked by.
    fn area(commands: &[PathCommand]) -> f64 {
        flatten(commands)
            .iter()
            .map(|contour| signed_area(contour))
            .sum::<f64>()
            .abs()
    }

    /// Two squares that share a quarter union into the area of both
    /// minus the quarter they share.
    #[test]
    fn two_squares_union() {
        let (a, b) = (square(0.0, 10.0), square(5.0, 10.0));
        let union = combine(&a, &b, BooleanOp::Union);
        let covered = area(&union);
        assert!(
            (covered - 175.0).abs() < 0.75,
            "union covers {covered}, not 175"
        );
    }

    /// A subtraction cuts the operand's area out of the subject.
    #[test]
    fn a_square_cuts_a_hole() {
        let (a, b) = (square(0.0, 20.0), square(5.0, 10.0));
        let cut = combine(&a, &b, BooleanOp::Subtract);
        let covered = area(&cut);
        assert!(
            (covered - 300.0).abs() < 0.75,
            "the cut covers {covered}, not 300"
        );
    }

    /// An intersection keeps only what the two share.
    #[test]
    fn two_squares_intersect() {
        let (a, b) = (square(0.0, 10.0), square(5.0, 10.0));
        let shared = combine(&a, &b, BooleanOp::Intersect);
        let covered = area(&shared);
        assert!(
            (covered - 25.0).abs() < 0.75,
            "the share covers {covered}, not 25"
        );
    }

    /// An exclusive-or keeps what either covers, minus what both do.
    #[test]
    fn two_squares_xor() {
        let (a, b) = (square(0.0, 10.0), square(5.0, 10.0));
        let either = combine(&a, &b, BooleanOp::Xor);
        let covered = area(&either);
        assert!(
            (covered - 150.0).abs() < 0.75,
            "the xor covers {covered}, not 150"
        );
    }

    /// An empty operand changes nothing, either side.
    #[test]
    fn an_empty_operand_changes_nothing() {
        let a = square(0.0, 10.0);
        assert_eq!(combine(&a, &[], BooleanOp::Union), a);
        assert_eq!(combine(&[], &a, BooleanOp::Intersect), a);
    }

    /// Curves flatten into the overlay and come back as the polylines
    /// the result is drawn from: two circles, unioned, cover a little
    /// more than one and less than two.
    #[test]
    fn a_curved_outline_combines() {
        let circle = |cx: f64, cy: f64, r: f64| {
            let k = 0.5523;
            let (x0, y0) = (cx + r, cy);
            let cubic = |c1: (f64, f64), c2: (f64, f64), to: (f64, f64)| PathCommand::CubicTo {
                c1: Point::new(c1.0, c1.1),
                c2: Point::new(c2.0, c2.1),
                point: Point::new(to.0, to.1),
            };
            vec![
                PathCommand::MoveTo {
                    point: Point::new(x0, y0),
                },
                cubic((x0, y0 + k * r), (cx + k * r, cy + r), (cx, cy + r)),
                cubic((cx - k * r, cy + r), (cx - r, y0 + k * r), (cx - r, cy)),
                cubic((cx - r, cy - k * r), (cx - k * r, cy - r), (cx, cy - r)),
                cubic((cx + k * r, cy - r), (x0, cy - k * r), (x0, y0)),
                PathCommand::Close,
            ]
        };
        // Centres 6*sqrt(2) apart, radii 10 both: the union is a lens
        // a good deal above one circle and below two.
        let (a, b) = (circle(10.0, 10.0, 10.0), circle(16.0, 10.0, 10.0));
        let union = combine(&a, &b, BooleanOp::Union);
        let covered = area(&union);
        assert!(
            covered > 315.0 && covered < 628.0,
            "the lens covers {covered}, which is not between one circle and two"
        );
        // One circle alone against a far-away square: an intersection
        // that shares nothing keeps nothing.
        let far = square(100.0, 10.0);
        let shared = combine(&a, &far, BooleanOp::Intersect);
        assert!(shared.is_empty(), "{shared:?}");
    }
}
