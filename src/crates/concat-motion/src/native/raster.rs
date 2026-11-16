// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The pixel operations the native engine is made of: colour-space
//! conversions, the linear-light compositor, a transformed blit, a
//! separable blur and the channel splits a chromatic aberration needs.
//!
//! Everything here works on tiny-skia pixmaps, which are premultiplied
//! sRGB. The project's compositor blends in linear light - that is the
//! contract `concat-render` keeps on the GPU and the one this engine
//! keeps on the CPU - so every composite walks premultiplied sRGB to
//! straight linear, blends, and walks back. The conversions are table
//! lookups, because a bake walks millions of pixels and a transcendental
//! per channel per pixel is a tax no frame asked for.

use tiny_skia::Pixmap;

use crate::comp::Blend;

/// sRGB 0..=255 to linear 0..=1, one entry per byte, built once: the
/// `powf` it needs is not a const fn, and a bake walks the table
/// millions of times to the one build's two hundred and fifty-six.
fn srgb_to_linear() -> &'static [f32; 256] {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0.0f32; 256];
        for (byte, slot) in table.iter_mut().enumerate() {
            let c = byte as f32 / 255.0;
            *slot = if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        table
    })
}

/// Linear 0..=1 to sRGB 0..=255, 4096 entries: finer than a byte, coarse
/// enough that the table beats the two `powf`s it replaces, and the error
/// it introduces is under one part in a thousand - smaller than the
/// dither a PNG encoder adds anyway. Built once, for the same reason.
fn linear_to_srgb() -> &'static [u8; 4096] {
    static TABLE: std::sync::OnceLock<[u8; 4096]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u8; 4096];
        for (at, slot) in table.iter_mut().enumerate() {
            let c = at as f32 / 4095.0;
            let srgb = if c <= 0.003_130_8 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            *slot = (srgb.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        table
    })
}

/// A straight linear colour.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Linear {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha.
    pub a: f32,
}

impl Linear {
    /// The transparent black every composite starts from.
    pub const CLEAR: Linear = Linear {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// A straight linear colour from a premultiplied sRGB pixel. The
    /// unpremultiply happens in gamma space, where the bytes were
    /// multiplied - the convention every 8-bit compositor, tiny-skia's
    /// included, stores - and the transfer function's table takes the
    /// straight bytes from there.
    fn from_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Linear {
        if a == 0 {
            return Linear::CLEAR;
        }
        let alpha = f32::from(a) / 255.0;
        let table = srgb_to_linear();
        let un = |c: u8| {
            let straight = (f32::from(c) / 255.0 / alpha).clamp(0.0, 1.0);
            table[(straight * 255.0).round() as usize]
        };
        Linear {
            r: un(r),
            g: un(g),
            b: un(b),
            a: alpha,
        }
    }

    /// Back to a premultiplied sRGB pixel, clamped: the straight linear
    /// channels walk the transfer function's table into straight
    /// bytes, and the alpha multiplies them the way the storage
    /// premultiplies - in gamma space, the mirror of the read above.
    /// The alpha itself is linear, because alpha never rode the gamma
    /// curve.
    fn to_premultiplied(self) -> (u8, u8, u8, u8) {
        let a = self.a.clamp(0.0, 1.0);
        let alpha_byte = (a * 255.0).round() as u8;
        let table = linear_to_srgb();
        let byte = |c: f32| {
            let straight = c.clamp(0.0, 1.0);
            let srgb = table[((straight * 4095.0).round() as usize).min(4095)];
            (f32::from(srgb) * a).round().min(f32::from(alpha_byte)) as u8
        };
        (byte(self.r), byte(self.g), byte(self.b), alpha_byte)
    }
}

/// The separable part of a blend: how two *straight* colours mix, before
/// the alpha arithmetic of compositing wraps it. W3C compositing and
/// blending level 1, the same table CSS mixes `mix-blend-mode` from.
fn blend_straight(blend: Blend, backdrop: f32, source: f32) -> f32 {
    match blend {
        Blend::Normal => source,
        Blend::Multiply => backdrop * source,
        Blend::Screen => backdrop + source - backdrop * source,
        Blend::Overlay => {
            if backdrop <= 0.5 {
                2.0 * backdrop * source
            } else {
                1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source)
            }
        }
        Blend::Darken => backdrop.min(source),
        Blend::Lighten => backdrop.max(source),
        Blend::Difference => (backdrop - source).abs(),
        Blend::SoftLight => {
            let d = if backdrop <= 0.25 {
                ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop
            } else {
                backdrop.sqrt()
            };
            if source <= 0.5 {
                // Cb - (1 - 2*Cs)*Cb*(1 - Cb): the light branch's D(Cb)
                // belongs to the other arm, where the spec's own factor
                // is the one this dark branch was built from.
                backdrop - 2.0 * (1.0 - backdrop) * backdrop * (0.5 - source)
            } else {
                backdrop + (d - backdrop) * 2.0 * (source - 0.5)
            }
        }
        Blend::Add => backdrop + source,
    }
}

/// Composites one straight linear source over one straight linear
/// backdrop: the alpha-weighted blend, source over where the backdrop is
/// transparent, backdrop kept where the source is. Returns straight
/// linear.
fn composite_pixel(blend: Blend, backdrop: Linear, source: Linear, opacity: f32) -> Linear {
    let source = Linear {
        a: source.a * opacity,
        ..source
    };
    let (ab, as_) = (backdrop.a, source.a);
    let mixed = |channel: fn(&Linear) -> f32| {
        blend_straight(blend, channel(&backdrop), channel(&source)) * ab * as_
            + channel(&source) * as_ * (1.0 - ab)
            + channel(&backdrop) * ab * (1.0 - as_)
    };
    let alpha = as_ + ab * (1.0 - as_);
    if alpha <= 0.0 {
        return Linear::CLEAR;
    }
    Linear {
        r: mixed(|c| c.r) / alpha,
        g: mixed(|c| c.g) / alpha,
        b: mixed(|c| c.b) / alpha,
        a: alpha,
    }
}

/// Composites `source` over `destination` in linear light, both the same
/// size, at `opacity`, with `blend`. This is the engine's one composite
/// for same-size layers - the shape layers, the mask products, the glow
/// and shadow underlays - so every one of them mixes the way the GPU
/// compositor mixes.
pub(crate) fn composite(destination: &mut Pixmap, source: &Pixmap, blend: Blend, opacity: f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let (from, into) = (source.data(), destination.data_mut());
    for (below, above) in into.chunks_exact_mut(4).zip(from.chunks_exact(4)) {
        let backdrop = Linear::from_premultiplied(below[0], below[1], below[2], below[3]);
        let source = Linear::from_premultiplied(above[0], above[1], above[2], above[3]);
        let out = composite_pixel(blend, backdrop, source, opacity).to_premultiplied();
        below[0] = out.0;
        below[1] = out.1;
        below[2] = out.2;
        below[3] = out.3;
    }
}

/// Where a transform puts a point: the one helper, because
/// `Transform` mutates in place and a mistake here moves every layer.
fn map(transform: tiny_skia::Transform, x: f32, y: f32) -> (f32, f32) {
    let mut point = tiny_skia::Point { x, y };
    transform.map_point(&mut point);
    (point.x, point.y)
}

/// Draws `source` over `destination` under an affine `transform`, in
/// linear light, with bilinear sampling and `blend` and `opacity`. This
/// is the path a picture, a text canvas or a sub-comp takes into the
/// frame: tiny-skia places paths under transforms but not pixmaps, and
/// the pixel loop here is the honest replacement - the same bilinear
/// sample the compositor's texture units take, the same linear blend.
///
/// `transform` maps *source* pixels to *destination* pixels, as the
/// scene graph composed it.
pub(crate) fn blit(
    destination: &mut Pixmap,
    source: &Pixmap,
    transform: tiny_skia::Transform,
    blend: Blend,
    opacity: f32,
) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let Some(inverse) = transform.invert() else {
        return;
    };
    let (width, height) = (destination.width(), destination.height());
    let (source_w, source_h) = (source.width() as f32, source.height() as f32);
    let from = source.data();
    let into = destination.data_mut();
    // The destination rows this transform can touch, cheaply bounded by
    // the source's corners run forward through it.
    let corners = [
        map(transform, 0.0, 0.0),
        map(transform, source_w, 0.0),
        map(transform, 0.0, source_h),
        map(transform, source_w, source_h),
    ];
    let min_y = corners.iter().map(|c| c.1.floor()).fold(f32::MAX, f32::min);
    let max_y = corners.iter().map(|c| c.1.ceil()).fold(f32::MIN, f32::max);
    let y_from = min_y.max(0.0) as u32;
    let y_to = max_y.min(height as f32) as u32;
    for y in y_from..y_to {
        let row = (y as usize) * (width as usize) * 4;
        for x in 0..width {
            // The destination pixel's centre, in the source's own
            // pixel-edge space: pixel i spans [i, i+1], centres at
            // i + 1/2, and the transform maps edge to edge.
            let sample = map(inverse, x as f32 + 0.5, y as f32 + 0.5);
            let (sx, sy) = (sample.0, sample.1);
            if sx <= -1.0 || sy <= -1.0 || sx >= source_w + 1.0 || sy >= source_h + 1.0 {
                continue;
            }
            let sampled = bilinear(from, source_w, source_h, sx, sy);
            if sampled.a <= 0.0 {
                continue;
            }
            let at = row + (x as usize) * 4;
            let backdrop =
                Linear::from_premultiplied(into[at], into[at + 1], into[at + 2], into[at + 3]);
            let out = composite_pixel(blend, backdrop, sampled, opacity).to_premultiplied();
            into[at] = out.0;
            into[at + 1] = out.1;
            into[at + 2] = out.2;
            into[at + 3] = out.3;
        }
    }
}

/// One bilinear sample of a premultiplied sRGB pixmap, in straight
/// linear. A sample outside the picture fades to transparent over the
/// one pixel it is out: an edge scales to a soft rim rather than a hard
/// snap or, worse, an extrapolated copy of the edge colour - which an
/// opaque edge would smear as far as the transform asked.
fn bilinear(data: &[u8], width: f32, height: f32, x: f32, y: f32) -> Linear {
    if width < 1.0 || height < 1.0 {
        return Linear::CLEAR;
    }
    // Coverage fades over the one pixel outside the picture's edges:
    // an edge scales to a soft rim, never to a hard snap or an
    // extrapolated copy of the edge colour.
    let fade = |v: f32, max: f32| {
        if v < 0.0 {
            (1.0 + v).max(0.0)
        } else if v > max {
            (1.0 - (v - max)).max(0.0)
        } else {
            1.0
        }
    };
    let cover = fade(x, width) * fade(y, height);
    if cover <= 0.0 {
        return Linear::CLEAR;
    }
    // One axis of the kernel: pixel i spans [i, i+1] with its centre
    // at i + 1/2, so a sample left of the first centre is the first
    // pixel whole, and one right of the last centre is the last.
    let axis = |v: f32, max: usize| -> (usize, usize, f32) {
        if v <= 0.5 || max == 1 {
            return (0, 0, 0.0);
        }
        if v >= max as f32 - 0.5 {
            return (max - 1, max - 1, 0.0);
        }
        let at = (v - 0.5).floor() as usize;
        (at, (at + 1).min(max - 1), v - 0.5 - at as f32)
    };
    let (x0, x1, fx) = axis(x, width as usize);
    let (y0, y1, fy) = axis(y, height as usize);
    let stride = (width.max(0.0) as usize) * 4;
    let pixel = |px: usize, py: usize| -> Linear {
        let at = py * stride + px * 4;
        Linear::from_premultiplied(data[at], data[at + 1], data[at + 2], data[at + 3])
    };
    let mix = |a: Linear, b: Linear, u: f32| Linear {
        r: a.r + (b.r - a.r) * u,
        g: a.g + (b.g - a.g) * u,
        b: a.b + (b.b - a.b) * u,
        a: a.a + (b.a - a.a) * u,
    };
    let top = mix(pixel(x0, y0), pixel(x1, y0), fx);
    let bottom = mix(pixel(x0, y1), pixel(x1, y1), fx);
    let sampled = mix(top, bottom, fy);
    Linear {
        a: sampled.a * cover,
        ..sampled
    }
}

/// A separable box blur over premultiplied sRGB, run twice for a soft
/// falloff - the same shape `concat-text` softens a title's shadow with,
/// so a comp's shadow and a title's shadow fall the same way. `radius`
/// in pixels; under half a pixel leaves the picture alone.
///
/// The blur runs in gamma space, on premultiplied channels: a halo or a
/// mask feather is a soft edge, not a physical light transport, and the
/// difference is under a percent of a stop.
pub(crate) fn blur(pixmap: &mut Pixmap, radius: f64) {
    let radius = radius.clamp(0.0, 256.0);
    if radius < 0.5 {
        return;
    }
    let radius = radius.round() as usize;
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    if width == 0 || height == 0 {
        return;
    }
    let data = pixmap.data_mut();
    let mut scratch = vec![0u8; data.len()];
    for _ in 0..2 {
        // Horizontal, into scratch.
        for y in 0..height {
            let row = y * width * 4;
            for channel in 0..4 {
                let mut sum: u32 = 0;
                let window = (2 * radius + 1) as u32;
                let at = |x: isize| -> u32 {
                    let x = x.clamp(0, width as isize - 1) as usize;
                    u32::from(data[row + x * 4 + channel])
                };
                for x in -(radius as isize)..=(radius as isize) {
                    sum += at(x);
                }
                for x in 0..width {
                    scratch[row + x * 4 + channel] = (sum / window) as u8;
                    sum += at(x as isize + radius as isize + 1);
                    sum -= at(x as isize - radius as isize);
                }
            }
        }
        // Vertical, back into data.
        for x in 0..width {
            for channel in 0..4 {
                let mut sum: u32 = 0;
                let window = (2 * radius + 1) as u32;
                let at = |y: isize| -> u32 {
                    let y = y.clamp(0, height as isize - 1) as usize;
                    u32::from(scratch[(y * width + x) * 4 + channel])
                };
                for y in -(radius as isize)..=(radius as isize) {
                    sum += at(y);
                }
                for y in 0..height {
                    data[(y * width + x) * 4 + channel] = (sum / window) as u8;
                    sum += at(y as isize + radius as isize + 1);
                    sum -= at(y as isize - radius as isize);
                }
            }
        }
    }
}

/// A copy of `source`'s alpha, filled with `color`: the halo a glow or a
/// drop shadow is made of. Straight sRGB in, premultiplied sRGB out,
/// because the colour never blends on the way - it is multiplied by the
/// mask and composited later.
pub(crate) fn tinted_alpha(source: &Pixmap, color: crate::comp::Rgba) -> Pixmap {
    let mut out = Pixmap::new(source.width(), source.height())
        .unwrap_or_else(|| Pixmap::new(1, 1).expect("a 1x1 pixmap always allocates"));
    let (r, g, b) = (
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    for (out, from) in out
        .data_mut()
        .chunks_exact_mut(4)
        .zip(source.data().chunks_exact(4))
    {
        let alpha = from[3];
        out[0] = (u16::from(r) * u16::from(alpha) / 255) as u8;
        out[1] = (u16::from(g) * u16::from(alpha) / 255) as u8;
        out[2] = (u16::from(b) * u16::from(alpha) / 255) as u8;
        out[3] = alpha;
    }
    out
}

/// Chromatic aberration: the red and blue channels drift radially out
/// from `centre` by `amount` pixels at the frame's widest corner, the
/// green staying put. Bilinear on the drifted channels, in place.
pub(crate) fn aberrate(pixmap: &mut Pixmap, amount: f64, centre: (f64, f64)) {
    if amount.abs() < 0.05 {
        return;
    }
    let (width, height) = (pixmap.width() as f64, pixmap.height() as f64);
    let (columns, rows) = (pixmap.width(), pixmap.height());
    // The widest corner, whichever it is: the drift is `amount` pixels at
    // the farthest corner from `centre`, not only at the bottom-right one
    // (a centre toward that corner would otherwise blow the drift up by
    // the distance to the far side it never measured).
    let corner = centre
        .0
        .max(width - centre.0)
        .hypot(centre.1.max(height - centre.1))
        .max(1.0);
    let strength = amount / corner;
    let original = pixmap.clone();
    let from = original.data();
    let into = pixmap.data_mut();
    for y in 0..rows {
        for x in 0..columns {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (dx, dy) = (px - centre.0, py - centre.1);
            let at = (y as usize) * (columns as usize) * 4 + (x as usize) * 4;
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            let pull = |scale: f64, channel: usize| -> u8 {
                let (sx, sy) = (px + dx * scale - 0.5, py + dy * scale - 0.5);
                let sample = bilinear_channel(
                    from,
                    width as f32,
                    height as f32,
                    sx as f32,
                    sy as f32,
                    channel,
                );
                (sample * 255.0).clamp(0.0, 255.0) as u8
            };
            into[at] = pull(strength, 0);
            into[at + 2] = pull(-strength, 2);
        }
    }
}

/// One channel of one bilinear sample, straight from premultiplied
/// storage: aberration moves channels, not colours, so the premultiply
/// is kept and the channel is sampled where it is.
fn bilinear_channel(data: &[u8], width: f32, height: f32, x: f32, y: f32, channel: usize) -> f32 {
    let (x, y) = (x.clamp(0.0, width - 1.0), y.clamp(0.0, height - 1.0));
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let x1 = (x0 + 1).min(width as usize - 1);
    let y1 = (y0 + 1).min(height as usize - 1);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let stride = (width as usize) * 4;
    let at = |px: usize, py: usize| f32::from(data[py * stride + px * 4 + channel]) / 255.0;
    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
    top + (bottom - top) * fy
}

/// One bilinear sample of a whole pixel as a straight-linear colour, the
/// read the displacement distortions share: every one of them answers
/// "what colour lands here" by sampling the layer somewhere else, so the
/// sample is one function and the geometries are what differ.
fn sample_at(source: &Pixmap, x: f64, y: f64) -> Linear {
    let (width, height) = (f64::from(source.width()), f64::from(source.height()));
    bilinear(
        source.data(),
        width as f32,
        height as f32,
        x as f32,
        y as f32,
    )
}

/// A radial ripple, `at` seconds in: each destination pixel samples the
/// layer displaced along its own radius by a sine of the radius, so the
/// crests travel outward as `speed` turns the phase. In place, over a
/// copy, in straight linear light.
pub(crate) fn ripple(pixmap: &mut Pixmap, amplitude: f64, wavelength: f64, speed: f64, at: f64) {
    if amplitude.abs() < 0.05 || wavelength <= 0.0 {
        return;
    }
    let original = pixmap.clone();
    let (columns, rows) = (pixmap.width() as usize, pixmap.height() as usize);
    let (width, height) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
    let centre = (width / 2.0, height / 2.0);
    let phase = speed * at * std::f64::consts::TAU;
    let into = pixmap.data_mut();
    for y in 0..rows {
        for x in 0..columns {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (dx, dy) = (px - centre.0, py - centre.1);
            let radius = dx.hypot(dy);
            if radius < 1e-9 {
                continue;
            }
            let drift = (radius / wavelength * std::f64::consts::TAU - phase).sin() * amplitude;
            let sampled = sample_at(
                &original,
                px - dx / radius * drift,
                py - dy / radius * drift,
            );
            let at = (y * columns + x) * 4;
            let out = sampled.to_premultiplied();
            into[at] = out.0;
            into[at + 1] = out.1;
            into[at + 2] = out.2;
            into[at + 3] = out.3;
        }
    }
}

/// A directional wave, `at` seconds in: the displacement travels along
/// `angle` degrees, a sine of the position along that axis rather than
/// of the radius - the flag and the heat shimmer to ripple's pond.
pub(crate) fn wave(
    pixmap: &mut Pixmap,
    amplitude: f64,
    wavelength: f64,
    angle: f64,
    speed: f64,
    at: f64,
) {
    if amplitude.abs() < 0.05 || wavelength <= 0.0 {
        return;
    }
    let original = pixmap.clone();
    let (columns, rows) = (pixmap.width() as usize, pixmap.height() as usize);
    let radians = angle.to_radians();
    let (along_x, along_y) = (radians.cos(), radians.sin());
    let phase = speed * at * std::f64::consts::TAU;
    let into = pixmap.data_mut();
    for y in 0..rows {
        for x in 0..columns {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let along = px * along_x + py * along_y;
            let drift = (along / wavelength * std::f64::consts::TAU - phase).sin() * amplitude;
            let sampled = sample_at(&original, px - along_x * drift, py - along_y * drift);
            let at = (y * columns + x) * 4;
            let out = sampled.to_premultiplied();
            into[at] = out.0;
            into[at + 1] = out.1;
            into[at + 2] = out.2;
            into[at + 3] = out.3;
        }
    }
}

/// A bulge over the node's centre: within `radius`, the destination
/// samples the source closer to the centre by the lens's factor, which
/// magnifies (a positive `amount`) or pinches (a negative one); outside
/// the lens the layer is what it was.
pub(crate) fn bulge(pixmap: &mut Pixmap, amount: f64, radius: f64) {
    if amount.abs() < 1e-3 || radius <= 0.0 {
        return;
    }
    let original = pixmap.clone();
    let (columns, rows) = (pixmap.width() as usize, pixmap.height() as usize);
    let (width, height) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
    let centre = (width / 2.0, height / 2.0);
    let into = pixmap.data_mut();
    for y in 0..rows {
        for x in 0..columns {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (dx, dy) = (px - centre.0, py - centre.1);
            let distance = dx.hypot(dy);
            if distance >= radius || distance < 1e-9 {
                continue;
            }
            // The lens's power falls off as the square of the distance
            // from the centre, so the magnification is smooth into the
            // untouched rim rather than stepping at the edge.
            let falloff = (1.0 - distance / radius).powi(2);
            let factor = 1.0 + amount * falloff;
            if factor <= 1e-3 {
                continue;
            }
            let sampled = sample_at(&original, centre.0 + dx / factor, centre.1 + dy / factor);
            let at = (y * columns + x) * 4;
            let out = sampled.to_premultiplied();
            into[at] = out.0;
            into[at + 1] = out.1;
            into[at + 2] = out.2;
            into[at + 3] = out.3;
        }
    }
}

/// Pixelation: the layer averaged into squares `size` pixels wide. The
/// average is taken over the premultiplied bytes, which a mosaic is
/// honest about - a chunky average of a chunky look, in the same space
/// the layer stores.
pub(crate) fn pixelate(pixmap: &mut Pixmap, size: f64) {
    let size = size.clamp(1.0, 4096.0).round() as usize;
    if size <= 1 {
        return;
    }
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    if width == 0 || height == 0 {
        return;
    }
    let data = pixmap.data_mut();
    for block_y in (0..height).step_by(size) {
        for block_x in (0..width).step_by(size) {
            let (x1, y1) = ((block_x + size).min(width), (block_y + size).min(height));
            let mut sums = [0u32; 4];
            let mut count = 0u32;
            for y in block_y..y1 {
                for x in block_x..x1 {
                    let at = (y * width + x) * 4;
                    for channel in 0..4 {
                        sums[channel] += u32::from(data[at + channel]);
                    }
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let average: [u8; 4] = sums.map(|sum| (sum / count) as u8);
            for y in block_y..y1 {
                for x in block_x..x1 {
                    let at = (y * width + x) * 4;
                    data[at..at + 4].copy_from_slice(&average);
                }
            }
        }
    }
}

/// CRT scanlines: every other band of `count` across the frame height
/// darkened by `opacity`, in place, on the premultiplied bytes - a
/// multiply of the colour channels, which the premultiply carries with
/// it, is the darkening a tube does.
pub(crate) fn scanlines(pixmap: &mut Pixmap, count: u32, opacity: f64) {
    let count = count.max(1);
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let height = pixmap.height();
    let width = pixmap.width() as usize;
    let data = pixmap.data_mut();
    let band = (f64::from(height) / f64::from(count)).max(1.0);
    for y in 0..height {
        if ((f64::from(y) + 0.5) / band).floor() as u32 % 2 == 1 {
            let row = (y as usize) * width * 4;
            let keep = (1.0 - opacity) * 255.0;
            // Alpha rides along: a darkened band of an opaque layer stays
            // opaque, but a transparent pixel has no colour to darken and
            // its zero alpha is all it is.
            for byte in data.iter_mut().skip(row).take(width * 4) {
                *byte = ((u16::from(*byte) * keep as u16 + 127) / 255) as u8;
            }
        }
    }
}

/// The band's displacement for the tracking jitter: a hash of the band
/// and the step the clock has reached, folded to `-1..=1` - a
/// deterministic wobble, the same bytes on every render of the same
/// frame, which a seeded shake would need a seed handed to say.
fn band_drift(band: u64, step: u64) -> f64 {
    let mut hash = band
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(step.wrapping_mul(0xD1B5_4A32_D192_ED03));
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    hash ^= hash >> 33;
    // The odd multiples of a half avoid the exact zero a centre fold of
    // a small hash would keep landing on: the jitter always jitters.
    ((hash % 2_048) as f64 / 1024.0 - 1.0).clamp(-1.0, 1.0)
}

/// VHS tracking jitter, `at` seconds in: the rows displaced sideways in
/// bands eight pixels tall, the band pattern stepped by `speed` bands a
/// second, and one band sweeping down the frame displaced by its own
/// half - the worn tape's roll, the bar and the wobble together.
pub(crate) fn tracking(pixmap: &mut Pixmap, amount: f64, speed: f64, at: f64) {
    if amount.abs() < 0.05 {
        return;
    }
    let original = pixmap.clone();
    let (columns, rows) = (pixmap.width() as usize, pixmap.height() as usize);
    // The clock: `speed` bands a second, so the pattern rolls at the
    // speed a tape's own control track would.
    let step = (speed * at * 8.0).round() as u64;
    // The sweeping bar: one band in twenty, rolling down the frame once
    // every twenty band-steps.
    let bar_band = (step % 20) as f64 * 8.0;
    let band_rows = 8.0;
    let into = pixmap.data_mut();
    for y in 0..rows {
        let band = (y as f64 / band_rows).floor() as u64;
        let mut drift = band_drift(band, step) * amount;
        // The bar itself: the band the sweep is on, plus its two
        // neighbours, displaced harder and to one side - the classic
        // tear, not just a louder wobble.
        let distance = (y as f64 - bar_band).abs();
        if distance < band_rows * 3.0 {
            drift += amount * 1.5 * (1.0 - distance / (band_rows * 3.0));
        }
        if drift.abs() < 0.05 {
            continue;
        }
        for x in 0..columns {
            let at = (y * columns + x) * 4;
            let sampled = sample_at(&original, x as f64 + 0.5 - drift, y as f64 + 0.5);
            let out = sampled.to_premultiplied();
            into[at] = out.0;
            into[at + 1] = out.1;
            into[at + 2] = out.2;
            into[at + 3] = out.3;
        }
    }
}

/// A kaleidoscope: the destination's angle about the centre folded into
/// the first wedge, so every wedge shows the first one's mirror - the
/// fold is its own inverse, which is what makes it a destination-side
/// sample like every other distortion here.
pub(crate) fn kaleidoscope(pixmap: &mut Pixmap, segments: u32, rotation: f64) {
    let segments = segments.max(2);
    let original = pixmap.clone();
    let (columns, rows) = (pixmap.width() as usize, pixmap.height() as usize);
    let (width, height) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
    let centre = (width / 2.0, height / 2.0);
    let wedge = std::f64::consts::TAU / f64::from(segments);
    let spin = rotation.to_radians();
    let into = pixmap.data_mut();
    for y in 0..rows {
        for x in 0..columns {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (dx, dy) = (px - centre.0, py - centre.1);
            let radius = dx.hypot(dy);
            if radius < 1e-9 {
                continue;
            }
            // The angle in the fold's own frame, spun by the rotation;
            // the triangle wave a mirror fold is - a period of two
            // wedges, folded back onto the first - and the spin added
            // back, so the wedge the fold samples from is the one the
            // rotation points at.
            let turned = (dy.atan2(dx) - spin).rem_euclid(2.0 * wedge);
            let folded = if turned <= wedge {
                turned
            } else {
                2.0 * wedge - turned
            };
            let sample_angle = folded + spin;
            let sampled = sample_at(
                &original,
                centre.0 + sample_angle.cos() * radius,
                centre.1 + sample_angle.sin() * radius,
            );
            let at = (y * columns + x) * 4;
            let out = sampled.to_premultiplied();
            into[at] = out.0;
            into[at + 1] = out.1;
            into[at + 2] = out.2;
            into[at + 3] = out.3;
        }
    }
}

/// A projective (perspective) blit, the raster half of the 2.5D
/// transform chain: `homography` maps *source* pixel coordinates on the
/// layer's own `z = 0` plane to *destination* pixels, the 3x3 a node's
/// rotations, depth and perspective - and the camera's view-projection -
/// reduce to. Where [`blit`] inverse-maps through an affine transform,
/// this one divides: the fourth row of the chain is the denominator.
///
/// Backfaces are culled: a homography that mirrors (its affine part's
/// determinant negative) is a plane turned past edge-on, and the layer
/// it would draw is the inside of the card - skipped whole, the way a
/// real object hides its own back. Pixels whose source lands behind the
/// eye (a non-positive denominator) are left untouched rather than
/// extrapolated.
pub(crate) fn warp(
    destination: &mut Pixmap,
    source: &Pixmap,
    homography: &[f64; 9],
    blend: Blend,
    opacity: f32,
) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let Some(inverse) = invert3(homography) else {
        return;
    };
    let (width, height) = (destination.width(), destination.height());
    let (source_w, source_h) = (source.width() as f64, source.height() as f64);
    // The source's corners through the homography, divided: the quad the
    // layer covers, and the two facts read from it - whether it faces
    // the viewer at all, and the rows it can touch.
    let corners = [
        (0.0, 0.0),
        (source_w, 0.0),
        (0.0, source_h),
        (source_w, source_h),
    ]
    .map(|(x, y)| project(homography, x, y));
    if corners.iter().all(|corner| corner.2) {
        // Divided and facing: the quad's signed area, positive when the
        // layer's face is toward the viewer. Negative is a backface.
        let quad = corners.map(|corner| (corner.0, corner.1));
        let area = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
            (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
        };
        let facing = area(quad[0], quad[1], quad[2]) + area(quad[1], quad[3], quad[2]);
        if facing <= 0.0 {
            return;
        }
    }
    let min_y = corners
        .iter()
        .filter(|corner| corner.2)
        .map(|corner| corner.1.floor())
        .fold(f64::MAX, f64::min);
    let max_y = corners
        .iter()
        .filter(|corner| corner.2)
        .map(|corner| corner.1.ceil())
        .fold(f64::MIN, f64::max);
    let y_from = min_y.max(0.0) as u32;
    let y_to = max_y.min(f64::from(height)) as u32;
    let from = source.data();
    let into = destination.data_mut();
    for y in y_from..y_to {
        let row = (y as usize) * (width as usize) * 4;
        for x in 0..width {
            // The destination pixel's centre, inverse-mapped with the
            // divide; a point whose denominator is not positive is the
            // far side of the eye, left to whatever it already holds.
            let (sx, sy, in_front) = project(&inverse, f64::from(x) + 0.5, f64::from(y) + 0.5);
            if !in_front {
                continue;
            }
            if sx <= -1.0 || sy <= -1.0 || sx >= source_w + 1.0 || sy >= source_h + 1.0 {
                continue;
            }
            let sampled = bilinear(from, source_w as f32, source_h as f32, sx as f32, sy as f32);
            if sampled.a <= 0.0 {
                continue;
            }
            let at = row + (x as usize) * 4;
            let backdrop =
                Linear::from_premultiplied(into[at], into[at + 1], into[at + 2], into[at + 3]);
            let out = composite_pixel(blend, backdrop, sampled, opacity).to_premultiplied();
            into[at] = out.0;
            into[at + 1] = out.1;
            into[at + 2] = out.2;
            into[at + 3] = out.3;
        }
    }
}

/// One point through a 3x3 homography, with the divide. The third
/// element says whether the point is in front of the eye at all: a
/// non-positive denominator is behind it, and the caller leaves that
/// pixel to whatever it already holds.
fn project(homography: &[f64; 9], x: f64, y: f64) -> (f64, f64, bool) {
    let w = homography[6] * x + homography[7] * y + homography[8];
    if w <= 1e-9 {
        return (x, y, false);
    }
    (
        (homography[0] * x + homography[1] * y + homography[2]) / w,
        (homography[3] * x + homography[4] * y + homography[5]) / w,
        true,
    )
}

/// A 3x3 inverse by the adjugate, the closed form a 3x3 deserves. The
/// determinant near zero is a layer edge-on to the eye - `None`, and the
/// caller skips it.
fn invert3(homography: &[f64; 9]) -> Option<[f64; 9]> {
    let [a, b, c, d, e, f, g, h, i] = *homography;
    let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if determinant.abs() < 1e-12 {
        return None;
    }
    let scale = 1.0 / determinant;
    Some([
        (e * i - f * h) * scale,
        (c * h - b * i) * scale,
        (b * f - c * e) * scale,
        (f * g - d * i) * scale,
        (a * i - c * g) * scale,
        (c * d - a * f) * scale,
        (d * h - e * g) * scale,
        (b * g - a * h) * scale,
        (a * e - b * d) * scale,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixmap_of(width: u32, height: u32, color: tiny_skia::Color) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).expect("a test pixmap allocates");
        pixmap.fill(color);
        pixmap
    }

    fn fill_rect(pixmap: &mut Pixmap, rect: tiny_skia::Rect) {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        let path = tiny_skia::PathBuilder::from_rect(rect);
        pixmap.as_mut().fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    #[test]
    fn a_source_over_composite_covers_a_transparent_destination() {
        let mut destination = pixmap_of(4, 4, tiny_skia::Color::TRANSPARENT);
        let source = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        composite(&mut destination, &source, Blend::Normal, 1.0);
        assert_eq!(destination.pixel(2, 2).unwrap().red(), 255);
        assert_eq!(destination.pixel(2, 2).unwrap().alpha(), 255);
    }

    #[test]
    fn an_opacity_composite_is_a_transparent_tint_of_the_source() {
        let mut destination = pixmap_of(4, 4, tiny_skia::Color::TRANSPARENT);
        let source = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        composite(&mut destination, &source, Blend::Normal, 0.5);
        let pixel = destination.pixel(2, 2).unwrap();
        assert_eq!(pixel.alpha(), 128);
        // Premultiplied red at half alpha.
        assert!((i32::from(pixel.red()) - 127).abs() <= 2, "{pixel:?}");
    }

    #[test]
    fn multiply_over_white_leaves_the_source_and_over_black_leaves_black() {
        let red = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let mut white = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        composite(&mut white, &red, Blend::Multiply, 1.0);
        assert_eq!(white.pixel(1, 1).unwrap().red(), 255);
        assert_eq!(white.pixel(1, 1).unwrap().green(), 0);
        let mut black = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(0, 0, 0, 255));
        composite(&mut black, &red, Blend::Multiply, 1.0);
        assert_eq!(black.pixel(1, 1).unwrap().red(), 0);
    }

    #[test]
    fn add_brightens_beyond_a_screen() {
        let source = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(180, 180, 180, 255));
        let mut screen = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(180, 180, 180, 255));
        composite(&mut screen, &source, Blend::Screen, 1.0);
        let mut add = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(180, 180, 180, 255));
        composite(&mut add, &source, Blend::Add, 1.0);
        assert!(add.pixel(1, 1).unwrap().red() > screen.pixel(1, 1).unwrap().red());
        // Linear light: 180 sRGB is ~0.456 linear, doubled is ~0.912,
        // which encodes back to ~245 - brighter than a screen, short of
        // the 255 a gamma-space add would clamp to.
        assert!(add.pixel(1, 1).unwrap().red() >= 240);
    }

    #[test]
    fn soft_light_follows_the_w3c_table_it_cites() {
        // The dark arm is Cb - (1 - 2*Cs)*Cb*(1 - Cb): the D(Cb) factor
        // belongs to the light arm alone, and borrowing it here blackened
        // a light source where the table says it barely darkens.
        let darkened = blend_straight(Blend::SoftLight, 0.75, 0.25);
        assert!(
            (darkened - 0.656_25).abs() < 1e-4,
            "a quarter-grey source over three-quarters grey: {darkened}"
        );
        // A black source squares the backdrop, not near-blacks it.
        let blackened = blend_straight(Blend::SoftLight, 0.75, 0.0);
        assert!((blackened - 0.5625).abs() < 1e-4, "{blackened}");
        // A white source takes the light arm to D(Cb).
        let whitened = blend_straight(Blend::SoftLight, 0.75, 1.0);
        assert!((whitened - 0.75_f32.sqrt()).abs() < 1e-4, "{whitened}");
    }

    #[test]
    fn a_blit_at_identity_copies_and_at_an_offset_moves() {
        let mut destination = pixmap_of(8, 8, tiny_skia::Color::TRANSPARENT);
        let source = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(0, 255, 0, 255));
        blit(
            &mut destination,
            &source,
            tiny_skia::Transform::from_translate(2.0, 2.0),
            Blend::Normal,
            1.0,
        );
        assert_eq!(destination.pixel(3, 3).unwrap().green(), 255);
        assert_eq!(destination.pixel(0, 0).unwrap().alpha(), 0);
    }

    #[test]
    fn a_scaled_blit_covers_a_larger_area() {
        let mut destination = pixmap_of(16, 16, tiny_skia::Color::TRANSPARENT);
        let source = pixmap_of(4, 4, tiny_skia::Color::from_rgba8(0, 0, 255, 255));
        blit(
            &mut destination,
            &source,
            tiny_skia::Transform::from_scale(4.0, 4.0),
            Blend::Normal,
            1.0,
        );
        // The source's pixel-edge square [0,4] maps to [0,16]: every
        // corner of the destination is covered, and the interpolated
        // middle is blue throughout.
        for (x, y) in [(1, 1), (14, 14), (1, 14), (14, 1), (8, 8)] {
            let pixel = destination.pixel(x, y).unwrap();
            assert!(pixel.blue() > 240, "({x},{y}): {pixel:?}");
            assert_eq!(pixel.alpha(), 255);
        }
    }

    #[test]
    fn a_blur_spreads_an_edge() {
        let mut pixmap = pixmap_of(64, 16, tiny_skia::Color::TRANSPARENT);
        fill_rect(
            &mut pixmap,
            tiny_skia::Rect::from_ltrb(28.0, 0.0, 36.0, 16.0).unwrap(),
        );
        let before = pixmap.pixel(20, 8).unwrap().alpha();
        blur(&mut pixmap, 4.0);
        let after = pixmap.pixel(20, 8).unwrap().alpha();
        assert_eq!(before, 0);
        assert!(after > 0, "the blur should have reached x=20");
        // Two 9-wide box passes over an 8-wide bar leave its centre
        // around two thirds - the honest number, not the 255 a single
        // pass would leave.
        assert!(pixmap.pixel(32, 8).unwrap().alpha() > 120);
    }

    #[test]
    fn a_tinted_alpha_keeps_the_shape_and_takes_the_colour() {
        let mut source = pixmap_of(8, 8, tiny_skia::Color::TRANSPARENT);
        fill_rect(
            &mut source,
            tiny_skia::Rect::from_ltrb(0.0, 0.0, 8.0, 4.0).unwrap(),
        );
        let halo = tinted_alpha(&source, crate::comp::Rgba::new(0.0, 0.0, 1.0, 1.0));
        let top = halo.pixel(4, 1).unwrap();
        assert_eq!(top.blue(), 255);
        assert_eq!(top.red(), 0);
        assert_eq!(halo.pixel(4, 6).unwrap().alpha(), 0);
    }

    #[test]
    fn an_aberration_pulls_red_one_way_and_blue_the_other() {
        // An opaque field, red on the left half and blue on the right:
        // where the channels drift apart, one side of the seam keeps
        // both colours and the other keeps neither.
        let mut pixmap = pixmap_of(32, 32, tiny_skia::Color::TRANSPARENT);
        let mut red = tiny_skia::Paint::default();
        red.set_color(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let mut blue = tiny_skia::Paint::default();
        blue.set_color(tiny_skia::Color::from_rgba8(0, 0, 255, 255));
        let left = tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_ltrb(0.0, 0.0, 16.0, 32.0).unwrap(),
        );
        let right = tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_ltrb(16.0, 0.0, 32.0, 32.0).unwrap(),
        );
        pixmap.as_mut().fill_path(
            &left,
            &red,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        pixmap.as_mut().fill_path(
            &right,
            &blue,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        aberrate(&mut pixmap, 40.0, (16.0, 16.0));
        // Left of the seam both channels sample their own side; right
        // of it, red samples further right (blue territory) and blue
        // further left (red territory), so the colour thins out.
        let near = pixmap.pixel(12, 16).unwrap();
        assert!(near.red() > 200, "{near:?}");
        assert!(near.blue() > 200, "{near:?}");
        let far = pixmap.pixel(20, 16).unwrap();
        assert!(far.red() < 40, "{far:?}");
        assert!(far.blue() < 40, "{far:?}");
        // Green never moves, and there was none to begin with.
        assert_eq!(pixmap.pixel(12, 16).unwrap().green(), 0);
    }

    #[test]
    fn a_zero_blur_and_a_zero_aberration_leave_the_picture_alone() {
        let mut pixmap = pixmap_of(8, 8, tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        let before = pixmap.pixel(4, 4).unwrap();
        blur(&mut pixmap, 0.0);
        aberrate(&mut pixmap, 0.0, (4.0, 4.0));
        assert_eq!(pixmap.pixel(4, 4).unwrap(), before);
    }

    /// A half-and-half field: the seam every displacement filter is
    /// measured against.
    fn seam_pixmap(size: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(size, size).expect("a test pixmap allocates");
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let left = tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_ltrb(0.0, 0.0, size as f32 / 2.0, size as f32).unwrap(),
        );
        pixmap.as_mut().fill_path(
            &left,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        paint.set_color(tiny_skia::Color::from_rgba8(0, 0, 255, 255));
        let right = tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_ltrb(size as f32 / 2.0, 0.0, size as f32, size as f32).unwrap(),
        );
        pixmap.as_mut().fill_path(
            &right,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        pixmap
    }

    #[test]
    fn a_ripple_blurs_the_seam_and_stays_deterministic() {
        let mut first = seam_pixmap(48);
        ripple(&mut first, 6.0, 12.0, 1.0, 0.4);
        let mut again = seam_pixmap(48);
        ripple(&mut again, 6.0, 12.0, 1.0, 0.4);
        assert_eq!(
            first.data(),
            again.data(),
            "the same frame is the same bytes"
        );
        // A seam displaced is a seam moved: some pixel of the seam's own
        // column reads differently than the flat seam's, and the frame
        // still carries ink.
        let flat = seam_pixmap(48);
        let mut moved = 0;
        for y in 0..48usize {
            let at = y * 48 * 4 + 24 * 4;
            if first.data()[at] != flat.data()[at] || first.data()[at + 2] != flat.data()[at + 2] {
                moved += 1;
            }
        }
        assert!(moved > 0, "the ripple moved the seam");
        assert!(first.data().iter().skip(3).step_by(4).any(|a| *a > 0));
    }

    #[test]
    fn a_wave_displaces_along_its_angle() {
        // A vertical wave moves the seam sideways: red appears past the
        // seam's own half, on the blue side.
        let mut pixmap = seam_pixmap(48);
        wave(&mut pixmap, 8.0, 96.0, 0.0, 0.0, 0.0);
        let past = pixmap.pixel(26, 24).unwrap();
        assert!(past.red() > 20, "the wave carried red right: {past:?}");
    }

    #[test]
    fn a_bulge_magnifies_the_centre() {
        // A one-pixel dot at the centre, magnified: its ink spreads over
        // the pixels the lens pulls it across.
        let mut pixmap = Pixmap::new(32, 32).expect("a test pixmap allocates");
        fill_rect(
            &mut pixmap,
            tiny_skia::Rect::from_ltrb(15.0, 15.0, 17.0, 17.0).unwrap(),
        );
        assert!(!inkish(&pixmap, 14, 16), "the rim of the dot is empty");
        bulge(&mut pixmap, 0.8, 12.0);
        assert!(
            inkish(&pixmap, 14, 16),
            "a magnified centre dot spreads to its neighbours"
        );
    }

    /// Whether a pixel carries any ink at all.
    fn inkish(pixmap: &Pixmap, x: u32, y: u32) -> bool {
        pixmap.pixel(x, y).is_some_and(|px| px.alpha() > 0)
    }

    #[test]
    fn pixelation_averages_into_squares() {
        // A quarter of the frame red, mosaiced into 8-pixel squares: the
        // square that straddles the red edge carries the average - half
        // red, half transparent - and every pixel of it matches it.
        let mut pixmap = Pixmap::new(16, 16).expect("a test pixmap allocates");
        fill_rect(
            &mut pixmap,
            tiny_skia::Rect::from_ltrb(0.0, 0.0, 4.0, 16.0).unwrap(),
        );
        pixelate(&mut pixmap, 8.0);
        let a = pixmap.pixel(1, 1).unwrap();
        let b = pixmap.pixel(7, 7).unwrap();
        let c = pixmap.pixel(5, 12).unwrap();
        assert_eq!(a, b, "one block, one colour");
        assert_eq!(a, c, "the block's every pixel is its average");
        assert_eq!(a.alpha(), 127, "half the block's pixels were red");
        assert!(a.red() > 100, "the average carries the red: {a:?}");
        assert_eq!(
            pixmap.pixel(12, 12).unwrap().alpha(),
            0,
            "the empty block stays empty"
        );
    }

    #[test]
    fn scanlines_darken_every_other_band() {
        let mut pixmap = pixmap_of(16, 16, tiny_skia::Color::from_rgba8(200, 200, 200, 255));
        scanlines(&mut pixmap, 8, 0.5);
        let bright = pixmap.pixel(4, 0).unwrap().red();
        let dark = pixmap.pixel(4, 2).unwrap().red();
        assert!(bright > 180, "{bright}");
        assert!(dark < 120, "the darkened band is half the light: {dark}");
        assert_eq!(pixmap.pixel(4, 0).unwrap().alpha(), 255);
    }

    #[test]
    fn tracking_shifts_rows_deterministically() {
        let mut first = seam_pixmap(48);
        tracking(&mut first, 10.0, 8.0, 0.25);
        let mut again = seam_pixmap(48);
        tracking(&mut again, 10.0, 8.0, 0.25);
        assert_eq!(first.data(), again.data(), "the jitter is deterministic");
        // Some row's seam moved: a row's red-blue boundary is not at 24.
        let flat = seam_pixmap(48);
        let mut moved = 0;
        for y in 0..48usize {
            let x = 24usize;
            let at = y * 48 * 4 + x * 4;
            if first.data()[at] != flat.data()[at] {
                moved += 1;
            }
        }
        assert!(moved > 0, "at least one band is displaced");
    }

    #[test]
    fn a_kaleidoscope_mirrors_the_first_wedge() {
        // The bottom half red, the top empty: two segments fold the
        // bottom onto the top, and the frame fills red - the wedge the
        // fold samples from is the one below the centre line.
        let mut pixmap = Pixmap::new(32, 32).expect("a test pixmap allocates");
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let bottom = tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_ltrb(0.0, 16.0, 32.0, 32.0).unwrap(),
        );
        pixmap.as_mut().fill_path(
            &bottom,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        kaleidoscope(&mut pixmap, 2, 0.0);
        assert!(
            pixmap.pixel(16, 4).unwrap().red() > 200,
            "the top half carries the bottom's mirror"
        );
        assert!(
            pixmap.pixel(16, 28).unwrap().red() > 200,
            "the wedge itself stays"
        );
        // Four segments fold every quadrant onto the first: a corner
        // outside a two-segment fold's reach carries ink too.
        let mut quartered = Pixmap::new(32, 32).expect("a test pixmap allocates");
        quartered.as_mut().fill_path(
            &tiny_skia::PathBuilder::from_rect(
                tiny_skia::Rect::from_ltrb(0.0, 16.0, 32.0, 32.0).unwrap(),
            ),
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        kaleidoscope(&mut quartered, 4, 0.0);
        assert!(inkish(&quartered, 4, 4), "four wedges reach the top-left");
    }
}
