// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The SVG importer: an SVG document converted into comp nodes, usvg
//! parsing the tree. Every path becomes a comp path node in canvas
//! coordinates - the viewBox folded in by usvg's own absolute transforms -
//! with its fill and its stroke, gradients included; a group becomes the
//! paint order it already is, and nothing is invented.
//!
//! Text, images and patterns are said as skipped, honestly: the parser
//! carries no text engine (usvg without its text feature), a picture is an
//! asset the SVG has no bytes for, and a pattern has no counterpart in the
//! comp's paint vocabulary.

use concat_motion::comp::{
    Comp, FORMAT, GradientKind, Node, NodeKind, Paint, PathCommand, Point, Rgba, Stroke, Transform,
};

use crate::{AdapterError, Import};

/// The character-class slug the other adapters share: lowercase,
/// alphanumerics and dashes, everything else a dash.
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for (i, ch) in name.char_indices() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if i > 0 && !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

/// An SVG document's own read of a colour: opaque sRGB u8 into the comp's
/// f32, alpha folded in by the caller.
fn color_of(color: usvg::Color, alpha: f64) -> Rgba {
    Rgba::new(
        f64::from(color.red) / 255.0,
        f64::from(color.green) / 255.0,
        f64::from(color.blue) / 255.0,
        alpha,
    )
}

/// A paint's stops as the comp reads them: the fill's own opacity folded
/// into every colour, the offsets kept as written.
fn stops_of(stops: &[usvg::Stop], opacity: f64) -> Vec<(f64, Rgba)> {
    stops
        .iter()
        .map(|stop| {
            (
                f64::from(stop.offset().get()).clamp(0.0, 1.0),
                color_of(stop.color(), f64::from(stop.opacity().get()) * opacity),
            )
        })
        .collect()
}

/// A usvg paint as a comp paint: colours straight, linear gradients with
/// the angle the line from `x1,y1` to `x2,y2` runs at, radial ones as
/// discs. A pattern is the one paint the comp has no word for.
fn paint_of(paint: &usvg::Paint, opacity: f64, import: &mut Import) -> Option<Paint> {
    match paint {
        usvg::Paint::Color(color) => Some(Paint::Solid {
            color: color_of(*color, opacity),
        }),
        usvg::Paint::LinearGradient(linear) => Some(Paint::Gradient {
            stops: stops_of(linear.stops(), opacity),
            ramp: GradientKind::Linear,
            angle: f64::from(
                (linear.y2() - linear.y1())
                    .atan2(linear.x2() - linear.x1())
                    .to_degrees(),
            ),
            from: 0.0,
            to: 1.0,
        }),
        usvg::Paint::RadialGradient(radial) => Some(Paint::Gradient {
            stops: stops_of(radial.stops(), opacity),
            ramp: GradientKind::Radial,
            angle: 0.0,
            from: 0.0,
            to: 1.0,
        }),
        usvg::Paint::Pattern(_) => {
            import.note("skipped: a pattern paint has no counterpart in the comp's vocabulary");
            None
        }
    }
}

/// One usvg path as comp commands: the points carried through the node's
/// absolute transform, quads elevated to cubics the way every vector stack
/// elevates them.
fn commands_of(path: &usvg::Path) -> Vec<PathCommand> {
    let transform = path.abs_transform();
    let at = |point: tiny_skia_path::Point| -> Point {
        let mut mapped = point;
        transform.map_point(&mut mapped);
        Point::new(f64::from(mapped.x), f64::from(mapped.y))
    };
    let mut commands = Vec::new();
    let mut last = Point::new(0.0, 0.0);
    for segment in path.data().segments() {
        match segment {
            tiny_skia_path::PathSegment::MoveTo(point) => {
                let to = at(point);
                commands.push(PathCommand::MoveTo { point: to });
                last = to;
            }
            tiny_skia_path::PathSegment::LineTo(point) => {
                let to = at(point);
                commands.push(PathCommand::LineTo { point: to });
                last = to;
            }
            tiny_skia_path::PathSegment::QuadTo(control, point) => {
                let control = at(control);
                let to = at(point);
                // The quad's own curve, elevated to the cubic that rides it:
                // both control points two thirds of the way to the quad's.
                commands.push(PathCommand::CubicTo {
                    c1: Point::new(
                        last.x + (control.x - last.x) * 2.0 / 3.0,
                        last.y + (control.y - last.y) * 2.0 / 3.0,
                    ),
                    c2: Point::new(
                        to.x + (control.x - to.x) * 2.0 / 3.0,
                        to.y + (control.y - to.y) * 2.0 / 3.0,
                    ),
                    point: to,
                });
                last = to;
            }
            tiny_skia_path::PathSegment::CubicTo(c1, c2, point) => {
                commands.push(PathCommand::CubicTo {
                    c1: at(c1),
                    c2: at(c2),
                    point: at(point),
                });
                last = at(point);
            }
            tiny_skia_path::PathSegment::Close => commands.push(PathCommand::Close),
        }
    }
    commands
}

/// Convert an SVG document into a comp. The document is read defensively -
/// a document that does not parse is refused, and everything skipped or
/// approximated is said in the [`Import`]'s notes.
pub fn import(text: &str) -> Result<Import, AdapterError> {
    let not_svg = |said: &str| AdapterError::NotTheFormat {
        format: "SVG",
        said: said.into(),
    };
    if !text.trim_start().starts_with('<') {
        return Err(not_svg("the document does not begin as XML"));
    }
    let tree = usvg::Tree::from_str(text, &usvg::Options::default())
        .map_err(|error| not_svg(&format!("the document does not parse: {error:?}")))?;
    let width = tree.size().width().ceil().clamp(1.0, 8192.0);
    let height = tree.size().height().ceil().clamp(1.0, 8192.0);
    let mut import = Import::clean(Comp {
        format: FORMAT,
        id: "svg.document".into(),
        name: "SVG document".into(),
        width: width.round() as u32,
        height: height.round() as u32,
        fps: 30.0,
        duration: 3.0,
        background: Rgba::new(0.0, 0.0, 0.0, 0.0),
        assets: Default::default(),
        curves: Default::default(),
        nodes: Vec::new(),
        comps: Default::default(),
        slots: Vec::new(),
    });
    import.comp.id = "svg.document".into();
    walk(tree.root(), &mut import, 0);
    if import.comp.nodes.is_empty() {
        return Err(not_svg(
            "the document parsed but carries nothing the comp can draw",
        ));
    }
    Ok(import)
}

/// Walk a group's children: paths convert, groups recurse, everything else
/// is said. Depth is bounded the way the Lottie walk bounds precomps.
fn walk(group: &usvg::Group, import: &mut Import, depth: usize) {
    if depth > 8 {
        import.note("skipped: a group deeper than eight levels, the walk stops there");
        return;
    }
    for node in group.children() {
        match node {
            usvg::Node::Path(path) => {
                if !path.is_visible() {
                    import.note(format!(
                        "skipped: path {:?} is invisible in the source",
                        path.id()
                    ));
                    continue;
                }
                let id = if path.id().is_empty() {
                    format!("svg.path{}", import.comp.nodes.len() + 1)
                } else {
                    format!("svg.{}", slug(path.id()))
                };
                let fill = path.fill().and_then(|fill| {
                    paint_of(fill.paint(), f64::from(fill.opacity().get()), import)
                });
                let stroke = path.stroke().map(|stroke| {
                    // The stroke rides the node's absolute transform: a
                    // 4-unit stroke on a viewBox doubled to the canvas is
                    // 8 pixels on the canvas, and the dashes travel with it.
                    let (sx, sy) = path.abs_transform().get_scale();
                    let scale = f64::from(((sx + sy) / 2.0).max(0.0));
                    let width = f64::from(stroke.width().get()).max(0.0) * scale;
                    let opacity = f64::from(stroke.opacity().get());
                    let color = match stroke.paint() {
                        usvg::Paint::Color(color) => color_of(*color, opacity),
                        usvg::Paint::LinearGradient(linear) => {
                            import.note(
                                "approximated: a gradient stroke is drawn at the middle of its own ramp, as a flat colour",
                            );
                            let stops = stops_of(linear.stops(), opacity);
                            let middle = stops.len() / 2;
                            stops.get(middle).or(stops.first()).map(|(_, c)| *c).unwrap_or(Rgba::new(1.0, 1.0, 1.0, opacity))
                        }
                        usvg::Paint::RadialGradient(radial) => {
                            import.note(
                                "approximated: a gradient stroke is drawn at the middle of its own ramp, as a flat colour",
                            );
                            let stops = stops_of(radial.stops(), opacity);
                            let middle = stops.len() / 2;
                            stops.get(middle).or(stops.first()).map(|(_, c)| *c).unwrap_or(Rgba::new(1.0, 1.0, 1.0, opacity))
                        }
                        usvg::Paint::Pattern(_) => Rgba::new(1.0, 1.0, 1.0, opacity),
                    };
                    Stroke { color, width }
                });
                let dash = path
                    .stroke()
                    .and_then(|stroke| stroke.dasharray())
                    .and_then(|dashes| {
                        let (sx, sy) = path.abs_transform().get_scale();
                        let scale = f64::from(((sx + sy) / 2.0).max(0.0));
                        let first = f64::from(*dashes.first()?) * scale;
                        let second = f64::from(*dashes.get(1).unwrap_or(&0.0)) * scale;
                        if first <= 0.0 {
                            return None;
                        }
                        if dashes.len() > 2 {
                            import.note(
                                "approximated: a dash array longer than two is drawn at its first pair",
                            );
                        }
                        Some((first, second))
                    });
                import.comp.nodes.push(Node {
                    id,
                    parent: None,
                    start: 0.0,
                    duration: 0.0,
                    transform: Transform::default(),
                    opacity: 1.0,
                    blend: concat_motion::comp::Blend::Normal,
                    mask: None,
                    matte: None,
                    motion_blur: None,
                    effects: Vec::new(),
                    tracks: Default::default(),
                    kind: NodeKind::Path {
                        commands: commands_of(path),
                        fill,
                        stroke,
                        dash,
                        trim: None,
                        morph: None,
                        boolean: None,
                        taper: None,
                    },
                });
            }
            usvg::Node::Group(child) => walk(child, import, depth + 1),
            usvg::Node::Image(image) => {
                import.note(format!(
                    "skipped: image {:?} is pixels the SVG does not carry",
                    image.id()
                ));
            }
            usvg::Node::Text(text) => {
                import.note(format!(
                    "skipped: text {:?} - the parser builds no text engine",
                    text.id()
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document with a viewBox, two paths - one filled, one stroked and
    /// dashed - and a linear gradient, converts into two path nodes in
    /// canvas coordinates, the viewBox folded in.
    #[test]
    fn paths_convert_in_canvas_coordinates() {
        let svg = r##"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="200" height="200">
  <rect id="card" x="10" y="20" width="30" height="40" fill="#d9a441"/>
  <line id="rule" x1="0" y1="90" x2="100" y2="90"
        stroke="#f4f1ea" stroke-width="4" stroke-dasharray="8 4"/>
</svg>
"##;
        let import = import(svg).expect("converts");
        assert_eq!((import.comp.width, import.comp.height), (200, 200));
        assert_eq!(import.comp.nodes.len(), 2, "{:?}", import.comp.nodes);
        // The viewBox is folded in: a rect at 10,20 in a 100-unit viewBox
        // on a 200px canvas lands at 20,40, twice the size.
        let card = import.comp.node("svg.card").expect("the card converted");
        let NodeKind::Path { fill, .. } = &card.kind else {
            panic!("a path, found {:?}", card.kind);
        };
        let Some(Paint::Solid { color }) = fill else {
            panic!("a solid fill, found {fill:?}");
        };
        assert_eq!(color, &Rgba::hex("#d9a441"));
        let first = commands_first(card);
        assert!((first.x - 20.0).abs() < 1e-6, "{first:?}");
        // The rule: dashed, stroked, at the bottom of the canvas.
        let rule = import.comp.node("svg.rule").expect("the rule converted");
        let NodeKind::Path { stroke, dash, .. } = &rule.kind else {
            panic!("a path, found {:?}", rule.kind);
        };
        let stroke = stroke.as_ref().expect("the rule is stroked");
        assert!((stroke.width - 8.0).abs() < 1e-6, "{stroke:?}");
        assert_eq!(*dash, Some((16.0, 8.0)));
    }

    fn commands_first(node: &Node) -> Point {
        let NodeKind::Path { commands, .. } = &node.kind else {
            panic!("a path");
        };
        match commands.first() {
            Some(PathCommand::MoveTo { point }) => *point,
            other => panic!("a move first, found {other:?}"),
        }
    }

    /// A gradient fill converts with its stops; a pattern is said.
    #[test]
    fn gradients_convert_and_patterns_are_said() {
        let svg = r##"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="100" height="100">
  <defs>
    <linearGradient id="gold" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#0c0d10"/>
      <stop offset="1" stop-color="#d9a441"/>
    </linearGradient>
    <pattern id="dots" width="10" height="10" patternUnits="userSpaceOnUse">
      <circle cx="5" cy="5" r="2" fill="#d9a441"/>
    </pattern>
  </defs>
  <rect id="chip" x="0" y="0" width="100" height="100" fill="url(#gold)"/>
  <rect id="pat" x="0" y="0" width="60" height="20" fill="url(#dots)"/>
</svg>
"##;
        let import = import(svg).expect("converts");
        let chip = import.comp.node("svg.chip").expect("the chip converted");
        let NodeKind::Path { fill, .. } = &chip.kind else {
            panic!("a path");
        };
        let Some(Paint::Gradient { stops, .. }) = fill else {
            panic!("a gradient fill, found {fill:?}");
        };
        assert_eq!(stops.first().unwrap().1, Rgba::hex("#0c0d10"));
        assert_eq!(stops.last().unwrap().1, Rgba::hex("#d9a441"));
        // A pattern fill is said as skipped, the comp has no word for it.
        let pat = import
            .comp
            .node("svg.pat")
            .expect("the pattern rect converted");
        let NodeKind::Path { fill, .. } = &pat.kind else {
            panic!("a path");
        };
        assert!(fill.is_none(), "the pattern was not invented: {fill:?}");
        assert!(
            import.notes.iter().any(|note| note.contains("pattern")),
            "{:?}",
            import.notes
        );
    }

    /// A document that is not SVG is refused, and so is one that parses
    /// into nothing the comp can draw.
    #[test]
    fn a_non_document_is_refused() {
        assert!(import("not a document").is_err());
        assert!(import("<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").is_err());
    }
}
