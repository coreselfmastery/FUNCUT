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

use crate::comp::{Node, Paint, TextNode};

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
    let content = content_of(text);
    let gradient_ink = match &text.fill {
        Some(Paint::Gradient { .. }) => text.fill.clone(),
        _ => None,
    };
    let needs_split = gradient_ink.is_some() || text.shadow.is_some();

    let mut glyph_style = base_style(text, frame);
    glyph_style.runs = runs_of(text, &content, local, current);
    if gradient_ink.is_some() {
        // The glyphs pass draws white and becomes the mask a gradient
        // paints through.
        glyph_style.color = "#ffffffff".into();
    }
    let plate_style_base = base_style(text, frame);
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
