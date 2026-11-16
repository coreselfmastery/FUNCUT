// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! A text node as pixels: the [`TitleStyle`] the shaped-run path in
//! `concat-text` renders, the two passes a gradient ink or a custom
//! shadow needs, and the arithmetic that lands the block where the
//! node's transform says it sits.
//!
//! The heavy lifting - faces, fallbacks across scripts, shaping, bidi,
//! wrapping, outlines - is `concat-text`'s, the same engine the window's
//! titles draw with. This bridge owns what is motion's alone: word
//! timing, the karaoke highlight, gradient ink over glyphs, a shadow
//! with its own colour and offset, and the plate behind the words.

use concat_text::{Align, Fonts, RenderedFrame, TitleRun, TitleStyle};
use tiny_skia::Pixmap;

use crate::comp::{Node, Paint, PathCommand, Point, Rgba, TextFit, TextNode, TextOnPath};

/// Everything a text node needs drawn around it: the passes, in the
/// order they composite.
pub(crate) struct TextPasses {
    /// The plate behind the words, when the style asks for one and the
    /// glyphs could not carry it themselves: drawn first, alone, so
    /// nothing the glyphs do can stain it.
    pub plate: Option<Pixmap>,
    /// The shadow behind the glyphs, over the plate: the glyph alpha,
    /// tinted, blurred and offset. `None` when the node has no shadow.
    pub shadow: Option<Pixmap>,
    /// The glyphs themselves, the highlight baked into their runs.
    pub glyphs: Pixmap,
    /// The block's size in output pixels: what the node's box is.
    pub block: (f64, f64),
    /// Where the block's centre landed on the glyphs canvas, in output
    /// pixels from its top-left.
    pub centre: (f64, f64),
}

/// Draws one text node's content at `at` seconds into a frame-sized
/// canvas, ready for the scene to place: `frame` is the output canvas
/// size, `scale` the comp-to-output factor, and `fonts` the faces.
///
/// A word that has not been spoken yet is held in place with
/// transparent ink rather than left out, so the words that are said do
/// not move when one arrives - the same layout the markup karaoke
/// keeps.
pub(crate) fn passes(
    node: &Node,
    text: &TextNode,
    at: f64,
    frame: (f64, f64),
    scale: f64,
    fonts: &Fonts,
) -> TextPasses {
    let local = at - node.start;
    let current = Node::current_word(text, at, node.start);
    // The span markup lives in the plain text; the words a caption's
    // timing carries are already per-word styled and carry no markup.
    let (mut plain, spans) = if text.words.is_empty() && text.text.contains('<') {
        crate::typography::parse_spans(&text.text)
    } else {
        (content_of(text), Vec::new())
    };
    let on_path = text.on_path.as_ref();
    if let Some(on) = on_path {
        return passes_on_path(
            node, text, on, &plain, &spans, local, current, frame, scale, fonts,
        );
    }
    // The fit solver answers a max_width with the mode the node names:
    // measure once unwrapped, then shrink the size, cut the line, or
    // leave the wrapping to the renderer below.
    let text = solve_fit(text, &mut plain, &spans, frame, fonts);
    let content = if text.words.is_empty() {
        plain.clone()
    } else {
        content_of(&text)
    };
    let gradient_ink = match &text.fill {
        Some(Paint::Gradient { .. }) => text.fill.clone(),
        _ => None,
    };
    let needs_split = gradient_ink.is_some() || text.shadow.is_some();

    let mut glyph_style = base_style(&text, frame);
    let mut spans = spans;
    // A span the fit solver's cut left past the end is clipped, not
    // dropped whole: half a styled run is half a run.
    for span in &mut spans {
        span.end = span.end.min(content.chars().count());
    }
    spans.retain(|span| span.end > span.start);
    glyph_style.runs = runs_of(&text, &content, local, current)
        .into_iter()
        .chain(span_runs(&spans))
        .collect();
    if gradient_ink.is_some() {
        // The glyphs pass draws white and becomes the mask a gradient
        // paints through.
        glyph_style.color = "#ffffffff".into();
    }
    let plate_style_base = base_style(&text, frame);
    let glyphs =
        match concat_text::render_frame(fonts, &glyph_style, frame.0 as u32, frame.1 as u32) {
            Ok(rendered) => rendered,
            Err(_) => {
                return TextPasses {
                    plate: None,
                    shadow: None,
                    glyphs: empty(frame),
                    block: (0.0, 0.0),
                    centre: (frame.0 / 2.0, frame.1 / 2.0),
                };
            }
        };

    // The plate pass, when one is needed the glyphs cannot carry:
    // the same runs so the layout cannot move between the two - a font
    // scale moves a line, and a plate drawn from different runs would
    // not sit under the words it is a plate for.
    let plate = if text.background.a > 0.0 && needs_split {
        let mut plate_style = plate_style_base;
        plate_style.runs = glyph_style
            .runs
            .iter()
            .map(|run| TitleRun {
                start: run.start,
                end: run.end,
                color: Some("#00000000".into()),
                font_weight: run.font_weight,
                italic: run.italic,
                underline: run.underline,
                strikethrough: run.strikethrough,
                font_scale: run.font_scale,
                stroke_color: Some("#00000000".into()),
            })
            .collect();
        concat_text::render_frame(fonts, &plate_style, frame.0 as u32, frame.1 as u32)
            .ok()
            .map(|rendered| pixmap_of(&rendered))
    } else if text.background.a > 0.0 {
        // A flat-ink, shadow-free text draws in one pass with its plate.
        Some(pixmap_of(&glyphs))
    } else {
        None
    };

    let mut glyph_pixmap = pixmap_of(&glyphs);
    // A gradient ink replaces the glyphs' flat colour through their own
    // alpha: the white pass is a mask the ramp paints.
    if let Some(fill) = &gradient_ink {
        paint_through_alpha(&mut glyph_pixmap, fill);
    }
    // A gradient highlight paints only the current word's box, over the
    // base ink the pass already drew.
    if let (Some(highlight), Some(word)) = (&text.highlight, current)
        && matches!(highlight.fill, Some(Paint::Gradient { .. }))
        && let Some(Paint::Gradient { .. }) = &highlight.fill
        && let Some(fill) = &highlight.fill
        && let Some(rect) = word_rect(&glyphs, word)
    {
        let mut region = glyph_pixmap.clone();
        keep_rect(&mut region, rect);
        paint_through_alpha(&mut region, fill);
        raster_composite(&mut glyph_pixmap, &region);
    }

    // The span plates: a `bg` run's own glyph alpha, tinted, blurred
    // a touch for the padding a plate wants, laid under the glyphs.
    let backplates: Vec<Pixmap> = bg_spans(&spans)
        .into_iter()
        .filter_map(|(span, color)| {
            let mut mask_style = glyph_style.clone();
            mask_style.color = "#00000000".into();
            mask_style.stroke_color = "#00000000".into();
            mask_style.runs = vec![TitleRun {
                start: span.start,
                end: span.end,
                color: Some("#ffffffff".into()),
                font_weight: None,
                italic: None,
                underline: None,
                strikethrough: None,
                font_scale: None,
                stroke_color: Some("#00000000".into()),
            }];
            let mask =
                concat_text::render_frame(fonts, &mask_style, frame.0 as u32, frame.1 as u32)
                    .ok()
                    .map(|rendered| pixmap_of(&rendered))?;
            let mut plate = crate::native::raster::tinted_alpha(&mask, color);
            crate::native::raster::blur(&mut plate, text.size * 0.08 * scale);
            Some(plate)
        })
        .collect();
    if !backplates.is_empty() {
        let mut under = Pixmap::new(glyph_pixmap.width(), glyph_pixmap.height())
            .unwrap_or_else(|| empty(frame));
        for plate in &backplates {
            crate::native::raster::composite(&mut under, plate, crate::comp::Blend::Normal, 1.0);
        }
        crate::native::raster::composite(
            &mut under,
            &glyph_pixmap,
            crate::comp::Blend::Normal,
            1.0,
        );
        glyph_pixmap = under;
    }

    // The shadow: the glyph alpha, tinted, blurred, offset. Over the
    // plate, under the glyphs - where a text shadow has always sat.
    let shadow = text.shadow.as_ref().map(|shadow| {
        let mut halo = crate::native::raster::tinted_alpha(&glyph_pixmap, shadow.color);
        crate::native::raster::blur(&mut halo, shadow.blur * scale);
        let mut moved =
            Pixmap::new(halo.width(), halo.height()).unwrap_or_else(|| empty((1.0, 1.0)));
        crate::native::raster::blit(
            &mut moved,
            &halo,
            tiny_skia::Transform::from_translate(
                shadow.x as f32 * scale as f32,
                shadow.y as f32 * scale as f32,
            ),
            crate::comp::Blend::Normal,
            1.0,
        );
        moved
    });

    let (canvas_w, canvas_h) = (f64::from(glyphs.width), f64::from(glyphs.height));
    TextPasses {
        plate,
        shadow,
        glyphs: glyph_pixmap,
        block: (
            f64::from(glyphs.block_width),
            f64::from(glyphs.block_height),
        ),
        centre: (
            canvas_w / 2.0 + f64::from(glyphs.block_dx),
            canvas_h / 2.0 + f64::from(glyphs.block_dy),
        ),
    }
}

/// The fit solver: measure the line unwrapped, and when it runs past
/// `max_width` do what the node's mode says - step the size down
/// (bounded by `min_size`), cut the line at the last full word and end
/// it with an ellipsis, or leave it to the wrapping the renderer below
/// already does. A line that fits keeps everything it had.
fn solve_fit(
    text: &TextNode,
    plain: &mut String,
    spans: &[crate::typography::Span],
    frame: (f64, f64),
    fonts: &Fonts,
) -> TextNode {
    let Some(max_width) = text.max_width else {
        return text.clone();
    };
    let mode = text.fit.unwrap_or(TextFit::Wrap);
    if mode == TextFit::Wrap || text.words.is_empty() && text.text.trim().is_empty() {
        return text.clone();
    }
    // The measure: the same style, unwrapped, so the answer is one
    // number - how wide the line is as a single run.
    let mut probe = text.clone();
    probe.max_width = None;
    let measured = |probe: &TextNode| {
        let style = base_style(probe, frame);
        concat_text::render_frame(fonts, &style, frame.0 as u32, frame.1 as u32)
            .map(|rendered| f64::from(rendered.block_width))
            .unwrap_or(f64::INFINITY)
    };
    let width = measured(&probe);
    if width <= max_width {
        return text.clone();
    }
    match mode {
        TextFit::Wrap => text.clone(),
        TextFit::ShrinkToFit => {
            let factor = max_width / width;
            let floor = text.min_size.unwrap_or(text.size * 0.25);
            let size = (text.size * factor).clamp(floor.min(text.size), text.size);
            probe.size = size;
            probe.letter_spacing *= size / text.size;
            probe
        }
        TextFit::Ellipsis => {
            // Cut at the last whole word that fits, then the ellipsis -
            // a line that cannot fit its own first word keeps the word
            // and the ellipsis both, cut mid-word, because a caption
            // that shows nothing is worse than one that shows a break.
            let chars: Vec<char> = plain.chars().collect();
            let mut kept = chars.len();
            let fits;
            loop {
                let candidate: String = chars[..kept].iter().collect();
                probe.text = format!("{candidate}…");
                if measured(&probe) <= max_width || kept == 0 {
                    fits = probe.text.clone();
                    break;
                }
                kept = chars[..kept]
                    .iter()
                    .rposition(|c| *c == ' ')
                    .unwrap_or(kept - 1);
            }
            let _ = spans;
            probe.text = fits;
            *plain = probe.text.clone();
            probe
        }
    }
}

/// The runs a span list adds to the block render: each span's answers
/// over the base style, for exactly its characters.
fn span_runs(spans: &[crate::typography::Span]) -> Vec<TitleRun> {
    spans
        .iter()
        .map(|span| TitleRun {
            start: span.start,
            end: span.end,
            color: span.color.map(|color| color.to_hex()),
            font_weight: span.weight.map(f64::from),
            italic: span.italic,
            underline: None,
            strikethrough: None,
            font_scale: span.scale,
            stroke_color: None,
        })
        .collect()
}

/// The spans that asked for a plate, with the colour they asked for.
fn bg_spans(spans: &[crate::typography::Span]) -> Vec<(crate::typography::Span, Rgba)> {
    spans
        .iter()
        .filter_map(|span| span.background.map(|color| (*span, color)))
        .collect()
}

/// One sample of a walked path: where it is and which way it runs.
struct PathStep {
    /// The point, in the node's own pixels.
    point: Point,
    /// The tangent's angle, degrees clockwise on the screen.
    angle: f64,
}

/// A path flattened into arc length: the polyline every text-on-a-path
/// walks, with each sample's place and tangent.
struct PathWalk {
    /// The samples, in order, each with the length reached at it.
    steps: Vec<(f64, PathStep)>,
    /// The path's whole length.
    total: f64,
}

/// Flattens commands into a walked path: lines as one segment, cubics
/// as thirty-two, which is finer than a glyph is wide on any path a
/// title walks.
fn walk_path(commands: &[PathCommand]) -> PathWalk {
    let mut steps: Vec<(f64, PathStep)> = Vec::new();
    let mut cursor = Point::new(0.0, 0.0);
    let mut carried = 0.0;
    let push = |from: Point, to: Point, carried: &mut f64, steps: &mut Vec<(f64, PathStep)>| {
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let length = dx.hypot(dy);
        if length < 1e-9 {
            return;
        }
        *carried += length;
        steps.push((
            *carried,
            PathStep {
                point: to,
                angle: dy.atan2(dx).to_degrees(),
            },
        ));
    };
    for command in commands {
        match *command {
            PathCommand::MoveTo { point } => {
                cursor = point;
                if steps.is_empty() {
                    steps.push((0.0, PathStep { point, angle: 0.0 }));
                }
            }
            PathCommand::LineTo { point } => {
                push(cursor, point, &mut carried, &mut steps);
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
                    push(cursor, to, &mut carried, &mut steps);
                    cursor = to;
                }
            }
            PathCommand::Close => {
                let start = steps.first().map(|(_, step)| step.point).unwrap_or(cursor);
                push(cursor, start, &mut carried, &mut steps);
                cursor = start;
            }
        }
    }
    PathWalk {
        total: carried,
        steps,
    }
}

impl PathWalk {
    /// The step at `length` along the walk, held at the ends.
    fn at(&self, length: f64) -> PathStep {
        let length = length.clamp(0.0, self.total);
        for (reached, step) in &self.steps {
            if *reached >= length {
                return PathStep {
                    point: step.point,
                    angle: step.angle,
                };
            }
        }
        self.steps
            .last()
            .map(|(_, step)| PathStep {
                point: step.point,
                angle: step.angle,
            })
            .unwrap_or(PathStep {
                point: Point::new(0.0, 0.0),
                angle: 0.0,
            })
    }
}

/// The glyph cache an on-path render shares between its characters:
/// one pixmap and the pen advance metrics, keyed by everything that
/// changes the drawing.
type GlyphCache =
    std::collections::HashMap<(char, u64, String, u32, bool), (Pixmap, f64, f64, f64)>;

/// The on-path render: one canvas, one glyph at a time, each turned to
/// the tangent where it sits. The current word's highlight scales and
/// recolours its glyphs the way the block path's does; the plate behind
/// the words becomes the ink's own bounds.
#[allow(clippy::too_many_arguments)]
fn passes_on_path(
    node: &Node,
    text: &TextNode,
    on: &TextOnPath,
    plain: &str,
    spans: &[crate::typography::Span],
    local: f64,
    current: Option<usize>,
    frame: (f64, f64),
    scale: f64,
    fonts: &Fonts,
) -> TextPasses {
    let _ = (local, current);
    let mut canvas = empty(frame);
    let walk = walk_path(&on.commands);
    // The offset: the field plus the track, both fractions of the path.
    let tracked = match node.property(
        "path-offset",
        0.0,
        0.0,
        crate::track::Value::Number { value: 0.0 },
    ) {
        crate::track::Value::Number { value } => value,
        _ => 0.0,
    };
    let mut along = (on.offset + tracked).rem_euclid(1.0).max(0.0) * walk.total;
    // The word layout the spans and the highlight read: plain char
    // indices, the same clock the block path counts.
    let chars: Vec<char> = plain.chars().collect();
    // One render per distinct (char, size, colour, weight, italic) the
    // line asks for: a title's repeated letters pay once.
    let mut cache: GlyphCache = std::collections::HashMap::new();
    for (start, character) in chars.iter().enumerate() {
        // The span answers, the innermost that covers the glyph.
        let span = spans
            .iter()
            .rev()
            .find(|span| span.start <= start && start < span.end);
        let mut style = base_style(text, frame);
        style.content = character.to_string();
        style.max_width = 0.0;
        if let Some(span) = span {
            if let Some(color) = span.color {
                style.color = color.to_hex();
            }
            if let Some(weight) = span.weight {
                style.font_weight = f64::from(weight);
            }
            if let Some(italic) = span.italic {
                style.italic = italic;
            }
            if let Some(size) = span.scale {
                style.font_size *= size.clamp(0.1, 8.0);
            }
        }
        let key = (
            *character,
            style.font_size.to_bits(),
            style.color.clone(),
            style.font_weight.round() as u32,
            style.italic,
        );
        let entry = match cache.get(&key) {
            Some(found) => found.clone(),
            None => {
                let rendered = match concat_text::render_frame(
                    fonts,
                    &style,
                    frame.0 as u32,
                    frame.1 as u32,
                ) {
                    Ok(rendered) => rendered,
                    Err(_) => continue,
                };
                let built = (
                    pixmap_of(&rendered),
                    f64::from(rendered.block_width),
                    f64::from(rendered.width) / 2.0 + f64::from(rendered.block_dx),
                    f64::from(rendered.height) / 2.0 + f64::from(rendered.block_dy),
                );
                cache.insert(key.clone(), built.clone());
                built
            }
        };
        let (_, advance, centre_x, centre_y) = entry;
        if *character == ' ' {
            along += advance * scale;
            continue;
        }
        let step = walk.at(along);
        let placed = tiny_skia::Transform::from_translate(
            (step.point.x * scale) as f32,
            (step.point.y * scale) as f32,
        )
        .pre_concat(tiny_skia::Transform::from_rotate(step.angle as f32))
        .pre_concat(tiny_skia::Transform::from_translate(
            -(centre_x * scale) as f32,
            -(centre_y * scale) as f32,
        ));
        crate::native::raster::blit(
            &mut canvas,
            cache_get(&cache, &key),
            placed,
            crate::comp::Blend::Normal,
            1.0,
        );
        along += (advance + on.spacing + text.letter_spacing) * scale;
    }
    // The ink's own bounds are the block: what the node's transform
    // places and what a mask is inscribed in.
    let (block, centre) = ink_bounds(&canvas, frame);
    // The plate behind the words: the bounds themselves, rounded the
    // way the block path's plate is.
    let plate = if text.background.a > 0.0 {
        let mut plate = empty(frame);
        if let Some(rect) = tiny_skia::Rect::from_ltrb(
            (centre.0 - block.0 / 2.0 - text.padding * scale) as f32,
            (centre.1 - block.1 / 2.0 - text.padding * scale) as f32,
            (centre.0 + block.0 / 2.0 + text.padding * scale) as f32,
            (centre.1 + block.1 / 2.0 + text.padding * scale) as f32,
        )
        .map(|rect| rounded(rect, text.radius * scale))
        {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(crate::native::shapes::color_skia(text.background));
            paint.anti_alias = true;
            plate.as_mut().fill_path(
                &rect,
                &paint,
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );
        }
        Some(plate)
    } else {
        None
    };
    TextPasses {
        plate,
        shadow: None,
        glyphs: canvas,
        block,
        centre,
    }
}

/// The cached glyph pixmap for a key, by reference.
fn cache_get<'a>(cache: &'a GlyphCache, key: &(char, u64, String, u32, bool)) -> &'a Pixmap {
    &cache.get(key).expect("the key was just inserted").0
}

/// A rectangle with its corners rounded, as a path.
fn rounded(rect: tiny_skia::Rect, radius: f64) -> tiny_skia::Path {
    match crate::native::shapes::rounded_rect(
        f64::from(rect.width()),
        f64::from(rect.height()),
        radius,
    ) {
        Some(path) => path,
        None => tiny_skia::PathBuilder::from_rect(rect),
    }
}

/// The ink's bounds on a canvas, as `(block size, centre)`: the same
/// two numbers a block render's `block_*` carry, read from the pixels
/// themselves because the on-path line has no layout of its own.
fn ink_bounds(canvas: &Pixmap, frame: (f64, f64)) -> ((f64, f64), (f64, f64)) {
    let (width, height) = (canvas.width() as usize, canvas.height() as usize);
    let data = canvas.data();
    let mut x0 = usize::MAX;
    let mut y0 = usize::MAX;
    let mut x1 = 0usize;
    let mut y1 = 0usize;
    for y in 0..height {
        for x in 0..width {
            if data[(y * width + x) * 4 + 3] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x0 == usize::MAX {
        return ((0.0, 0.0), (frame.0 / 2.0, frame.1 / 2.0));
    }
    (
        ((x1 - x0) as f64, (y1 - y0) as f64),
        ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0),
    )
}

/// The words a text node shows, as one string: the timed words joined
/// by spaces when they are timed, the plain text otherwise.
fn content_of(text: &TextNode) -> String {
    if text.words.is_empty() {
        text.text.clone()
    } else {
        text.words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The style the glyphs pass starts from: the node's own fields, as
/// the fractions of the output frame `concat-text` takes.
fn base_style(text: &TextNode, frame: (f64, f64)) -> TitleStyle {
    let height = frame.1.max(1.0);
    let width = frame.0.max(1.0);
    TitleStyle {
        content: content_of(text),
        font_family: text.font.clone(),
        font_size: text.size / height,
        font_weight: f64::from(text.weight.clamp(100, 900)),
        italic: text.italic,
        underline: false,
        strikethrough: false,
        color: text.color.to_hex(),
        align: match text.align {
            crate::comp::TextAlign::Left => Align::Left,
            crate::comp::TextAlign::Center => Align::Center,
            crate::comp::TextAlign::Right => Align::Right,
        },
        stroke_width: text
            .outline
            .as_ref()
            .map(|stroke| stroke.width / height)
            .unwrap_or(0.0),
        stroke_color: text
            .outline
            .as_ref()
            .map(|stroke| stroke.color.to_hex())
            .unwrap_or_default(),
        shadow: false,
        background: if text.background.a > 0.0 {
            text.background.to_hex()
        } else {
            String::new()
        },
        background_radius: text.radius / height,
        background_padding_x: text.padding / height,
        background_padding_y: text.padding / height,
        line_height: text.line_height,
        tracking: text.letter_spacing / height,
        max_width: text.max_width.map(|px| px / width).unwrap_or(0.0),
        max_height: 0.0,
        runs: Vec::new(),
    }
}

/// The runs a moment needs: every word not yet spoken transparent, the
/// one being spoken wearing the highlight, the rest in the base look.
fn runs_of(text: &TextNode, content: &str, local: f64, current: Option<usize>) -> Vec<TitleRun> {
    if text.words.is_empty() {
        return Vec::new();
    }
    let _ = content;
    let mut runs = Vec::new();
    let mut at = 0usize;
    for (index, word) in text.words.iter().enumerate() {
        let start = at;
        let end = start + word.text.chars().count();
        at = end + 1; // The joining space.
        if local < word.start {
            // Hidden until it is said: transparent ink, transparent
            // outline, so nothing of the word shows but its place is
            // held and the words that are said do not move.
            runs.push(TitleRun {
                start,
                end,
                color: Some("#00000000".into()),
                font_weight: None,
                italic: None,
                underline: None,
                strikethrough: None,
                font_scale: None,
                stroke_color: Some("#00000000".into()),
            });
        } else if Some(index) == current && text.highlight.is_some() {
            let highlight = text.highlight.as_ref().expect("checked above");
            runs.push(TitleRun {
                start,
                end,
                color: highlight.color.map(|color| color.to_hex()),
                font_weight: highlight.weight.map(f64::from),
                italic: None,
                underline: None,
                strikethrough: None,
                font_scale: highlight.scale,
                stroke_color: None,
            });
        }
    }
    runs
}

/// A rendered frame as a pixmap: straight RGBA in, premultiplied out.
fn pixmap_of(rendered: &RenderedFrame) -> Pixmap {
    let mut pixmap = Pixmap::new(rendered.width, rendered.height)
        .unwrap_or_else(|| Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates"));
    let (into, from) = (pixmap.data_mut(), &rendered.rgba);
    for (out, straight) in into.chunks_exact_mut(4).zip(from.chunks_exact(4)) {
        let (r, g, b, a) = (
            u16::from(straight[0]),
            u16::from(straight[1]),
            u16::from(straight[2]),
            u16::from(straight[3]),
        );
        out[0] = (r * a / 255) as u8;
        out[1] = (g * a / 255) as u8;
        out[2] = (b * a / 255) as u8;
        out[3] = straight[3];
    }
    pixmap
}

/// Paints a gradient through a pixmap's own alpha: every pixel becomes
/// the ramp's colour at its place, at the pixel's alpha. The white
/// glyphs pass under a gradient ink is a mask; this is the paint.
fn paint_through_alpha(pixmap: &mut Pixmap, fill: &Paint) {
    let (width, height) = (pixmap.width(), pixmap.height());
    let Some(shader) = crate::native::shapes::shader_of(fill, f64::from(width), f64::from(height))
    else {
        return;
    };
    let paint = tiny_skia::Paint {
        shader,
        ..tiny_skia::Paint::default()
    };
    let Some(cover) = tiny_skia::Rect::from_ltrb(0.0, 0.0, width as f32, height as f32)
        .map(tiny_skia::PathBuilder::from_rect)
    else {
        return;
    };
    let mut inked = Pixmap::new(width, height)
        .unwrap_or_else(|| Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates"));
    inked.as_mut().fill_path(
        &cover,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
    // The fill is opaque, so `inked`'s bytes are the ramp's straight
    // colour; the glyph alpha carries them as premultiplied ink. The
    // alphas are read before the write begins - one pixmap cannot be
    // read and written through the same borrow at once.
    let alphas: Vec<u8> = pixmap.data().chunks_exact(4).map(|px| px[3]).collect();
    let ink = inked.data();
    let data = pixmap.data_mut();
    for (at, (out, new)) in data
        .chunks_exact_mut(4)
        .zip(ink.chunks_exact(4))
        .enumerate()
    {
        let alpha = u16::from(alphas[at]);
        out[0] = (u16::from(new[0]) * alpha / 255) as u8;
        out[1] = (u16::from(new[1]) * alpha / 255) as u8;
        out[2] = (u16::from(new[2]) * alpha / 255) as u8;
        out[3] = alphas[at];
    }
}

/// Zeroes every pixel outside `(x, y, w, h)`, keeping what is inside:
/// the current word's share of a canvas the highlight gradient paints.
fn keep_rect(pixmap: &mut Pixmap, rect: (i32, i32, u32, u32)) {
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    let (x0, y0) = (rect.0.max(0), rect.1.max(0));
    let (x1, y1) = (
        (rect.0 + rect.2 as i32).min(width),
        (rect.1 + rect.3 as i32).min(height),
    );
    let data = pixmap.data_mut();
    for y in 0..height {
        for x in 0..width {
            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                let at = (y * width + x) as usize * 4;
                data[at] = 0;
                data[at + 1] = 0;
                data[at + 2] = 0;
                data[at + 3] = 0;
            }
        }
    }
}

/// One pixmap over another, plain source-over: the highlight region
/// onto the glyphs it belongs to.
fn raster_composite(under: &mut Pixmap, over: &Pixmap) {
    crate::native::raster::composite(under, over, crate::comp::Blend::Normal, 1.0);
}

/// The current word's box on the canvas, `(x, y, w, h)`, when the
/// renderer reports the words it laid out.
fn word_rect(rendered: &RenderedFrame, index: usize) -> Option<(i32, i32, u32, u32)> {
    rendered
        .words
        .get(index)
        .map(|word| (word.x, word.y, word.width, word.height))
}

/// A transparent pixmap of the asked-for size, for the paths that have
/// nothing to show.
fn empty(frame: (f64, f64)) -> Pixmap {
    Pixmap::new(frame.0.max(1.0) as u32, frame.1.max(1.0) as u32)
        .unwrap_or_else(|| Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates"))
}
