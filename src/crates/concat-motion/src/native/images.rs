// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Stills a comp points at, decoded once per bake: PNG, JPEG and WebP,
//! the three extensions the asset table calls an image, cached by the
//! path they were read from so three hundred frames pay for one decode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tiny_skia::Pixmap;

/// The decoded pictures one bake is working with, keyed by the file
/// each was read from. A bake walks the same comp's nodes frame after
/// frame; the pictures do not change between frames, and re-decoding
/// them per frame would make a bake cost `frames` times what it must.
#[derive(Default)]
pub(crate) struct Pictures {
    cache: HashMap<PathBuf, Option<Pixmap>>,
}

impl Pictures {
    /// The picture at `path`, decoded: `None` when the file is not
    /// there, is not a still this engine reads, or will not decode -
    /// every one a warning the caller names, never a silent hole.
    pub(crate) fn picture(&mut self, path: &Path) -> Option<&Pixmap> {
        let decoded = self
            .cache
            .entry(path.to_path_buf())
            .or_insert_with(|| decode(path));
        decoded.as_ref()
    }
}

/// One still, decoded by its extension: PNG through tiny-skia, JPEG
/// and WebP through their own decoders. A kind the table knows but
/// this reader does not is `None`, and the caller says so.
fn decode(path: &Path) -> Option<Pixmap> {
    let bytes = std::fs::read(path).ok()?;
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Pixmap::decode_png(&bytes).ok(),
        "jpg" | "jpeg" => decode_jpeg(&bytes),
        "webp" => decode_webp(&bytes),
        _ => None,
    }
}

/// A JPEG: no alpha, so straight is premultiplied and the pixmap is
/// the decoder's buffer with an opaque alpha woven in.
fn decode_jpeg(bytes: &[u8]) -> Option<Pixmap> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (info.width as usize, info.height as usize);
    let expected = width.checked_mul(height)?;
    let mut data = Vec::with_capacity(expected * 4);
    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => {
            for grey in pixels {
                data.extend_from_slice(&[grey, grey, grey, 255]);
            }
        }
        jpeg_decoder::PixelFormat::L16 => {
            for pair in pixels.chunks_exact(2) {
                let grey = pair[0];
                data.extend_from_slice(&[grey, grey, grey, 255]);
            }
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            for triple in pixels.chunks_exact(3) {
                data.extend_from_slice(&[triple[0], triple[1], triple[2], 255]);
            }
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            // The decoder has already inverted what needed inverting;
            // the four plates mix the way a print mixes.
            for plate in pixels.chunks_exact(4) {
                let (c, m, y, k) = (
                    f32::from(plate[0]) / 255.0,
                    f32::from(plate[1]) / 255.0,
                    f32::from(plate[2]) / 255.0,
                    f32::from(plate[3]) / 255.0,
                );
                let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                data.extend_from_slice(&[
                    to_u8((1.0 - c) * (1.0 - k)),
                    to_u8((1.0 - m) * (1.0 - k)),
                    to_u8((1.0 - y) * (1.0 - k)),
                    255,
                ]);
            }
        }
    }
    Pixmap::from_vec(
        data,
        tiny_skia::IntSize::from_wh(width as u32, height as u32)?,
    )
}

/// A WebP still: the first frame of an animated one, RGB or RGBA as
/// the file carries it, alpha woven in where it is absent.
fn decode_webp(bytes: &[u8]) -> Option<Pixmap> {
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (width, height) = decoder.dimensions();
    let alpha = decoder.has_alpha();
    let size = decoder.output_buffer_size()?;
    let mut raw = vec![0u8; size];
    decoder.read_image(&mut raw).ok()?;
    let pixels = if alpha {
        raw
    } else {
        let mut woven = Vec::with_capacity(raw.len() / 3 * 4);
        for triple in raw.chunks_exact(3) {
            woven.extend_from_slice(&[triple[0], triple[1], triple[2], 255]);
        }
        woven
    };
    Pixmap::from_vec(pixels, tiny_skia::IntSize::from_wh(width, height)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join(name);
        std::mem::forget(dir);
        path
    }

    #[test]
    fn a_png_decodes_and_is_cached() {
        let mut source = Pixmap::new(6, 4).unwrap();
        source.fill(tiny_skia::Color::from_rgba8(1, 2, 3, 255));
        let path = temp("picture.png");
        source.save_png(&path).unwrap();
        let mut pictures = Pictures::default();
        let first = pictures.picture(&path).expect("the png decodes").clone();
        assert_eq!(first.width(), 6);
        // Once decoded, the cached pixmap answers even if the source is
        // no longer readable. This proves the second access does not decode
        // the file again.
        std::fs::remove_file(&path).expect("remove the source after decoding");
        let second = pictures
            .picture(&path)
            .expect("the cached pixmap survives the source")
            .clone();
        assert_eq!(first.data(), second.data());
    }

    #[test]
    fn a_missing_file_is_none_and_stays_none() {
        let path = temp("not-there.png");
        let mut pictures = Pictures::default();
        assert!(pictures.picture(&path).is_none());
        assert!(pictures.picture(&path).is_none());
    }

    #[test]
    fn an_unknown_extension_is_none() {
        let path = temp("picture.svg");
        std::fs::write(&path, b"<svg></svg>").unwrap();
        let mut pictures = Pictures::default();
        assert!(pictures.picture(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_jpeg_decodes() {
        // A tiny 8x8 grey JPEG, hand-encoded bytes of the smallest
        // baseline table the decoder takes.
        let bytes: &[u8] = &[
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06,
            0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D,
            0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D,
            0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28,
            0x37, 0x29, 0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
            0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x08, 0x00, 0x08,
            0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
            0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10,
            0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00,
            0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
            0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42,
            0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16,
            0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
            0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55,
            0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73,
            0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
            0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
            0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
            0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6,
            0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
            0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08,
            0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xFB, 0xFA, 0x28, 0xA2, 0x8A, 0xFF, 0xD9,
        ];
        let path = temp("picture.jpg");
        std::fs::write(&path, bytes).unwrap();
        let decoded = decode(&path);
        assert!(decoded.is_some(), "the tiny jpeg should decode");
        let _ = std::fs::remove_file(&path);
    }
}
