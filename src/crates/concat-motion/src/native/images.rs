// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Stills a comp points at, decoded once per bake: PNG, JPEG and WebP,
//! the three extensions the asset table calls an image, cached by the
//! path they were read from so three hundred frames pay for one decode.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tiny_skia::Pixmap;
use tlottie::{CPURenderer, Composition, Limits, RenderOptions};
use zip::ZipArchive;

const MAX_LOTTIE_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_DOTLOTTIE_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DOTLOTTIE_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_LOTTIE_DIMENSION: u32 = 4096;
const MAX_LOTTIE_PIXELS: usize = 16 * 1024 * 1024;

/// A parsed Lottie composition and its reusable CPU frame buffer.
struct Lottie {
    renderer: CPURenderer,
    width: u32,
    height: u32,
    frame_rate: f32,
    in_point: f32,
    out_point: f32,
    pixels: Vec<u32>,
}

impl Lottie {
    fn load(path: &Path) -> Result<Self, String> {
        let json = match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("json") => read_bounded_file(path, MAX_LOTTIE_JSON_BYTES as u64)?,
            Some("lottie") => read_dotlottie_json(path)?,
            _ => return Err("expected a .json Lottie or .lottie dotLottie asset".into()),
        };
        ensure_supported_lottie(&json)?;

        let mut limits = Limits::default();
        limits.max_input_bytes = MAX_LOTTIE_JSON_BYTES;
        limits.max_dimension = MAX_LOTTIE_DIMENSION;
        limits.max_render_pixels = MAX_LOTTIE_PIXELS;
        limits.max_render_bytes = 128 * 1024 * 1024;
        let composition = Composition::parse(&json, &limits)
            .map_err(|error| format!("Lottie parse failed: {error}"))?;
        let (width, height) = (composition.width, composition.height);
        let frame_rate = composition.frame_rate;
        let (in_point, out_point) = (composition.in_point, composition.out_point);
        if width == 0
            || height == 0
            || width > MAX_LOTTIE_DIMENSION
            || height > MAX_LOTTIE_DIMENSION
            || !frame_rate.is_finite()
            || frame_rate <= 0.0
            || !in_point.is_finite()
            || !out_point.is_finite()
            || out_point <= in_point
        {
            return Err("Lottie canvas or frame range is outside the supported limits".into());
        }
        let pixel_count = (width as usize)
            .checked_mul(height as usize)
            .filter(|count| *count <= MAX_LOTTIE_PIXELS)
            .ok_or_else(|| "Lottie canvas exceeds the pixel budget".to_owned())?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(pixel_count)
            .map_err(|error| format!("cannot allocate Lottie frame buffer: {error}"))?;
        pixels.resize(pixel_count, 0);

        Ok(Self {
            renderer: CPURenderer::new(composition),
            width,
            height,
            frame_rate,
            in_point,
            out_point,
            pixels,
        })
    }

    fn frame(&mut self, at: f64, looped: bool) -> Result<Pixmap, String> {
        if !at.is_finite() {
            return Err("Lottie frame time must be finite".into());
        }
        let duration = f64::from(self.out_point - self.in_point) / f64::from(self.frame_rate);
        if !duration.is_finite() || duration <= 0.0 {
            return Err("Lottie animation has no playable duration".into());
        }
        let last_frame_time = ((f64::from(self.out_point - self.in_point) - 1.0).max(0.0)
            / f64::from(self.frame_rate))
        .max(0.0);
        let local_time = if looped {
            at.max(0.0).rem_euclid(duration)
        } else {
            at.clamp(0.0, last_frame_time)
        };
        let frame = (self.in_point + (local_time * f64::from(self.frame_rate)) as f32)
            .min(self.out_point - 1.0)
            .max(self.in_point);
        self.renderer
            .render(
                frame,
                &mut self.pixels,
                self.width,
                self.height,
                RenderOptions::default(),
            )
            .map_err(|error| format!("Lottie frame render failed: {error}"))?;

        // tlottie writes premultiplied RGBA words in little-endian memory,
        // which is exactly tiny-skia's Pixmap byte order.
        let byte_count = self
            .pixels
            .len()
            .checked_mul(4)
            .ok_or_else(|| "Lottie output byte count overflowed".to_owned())?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(byte_count)
            .map_err(|error| format!("cannot allocate Lottie output pixels: {error}"))?;
        for pixel in &self.pixels {
            rgba.extend_from_slice(&pixel.to_le_bytes());
        }
        let size = tiny_skia::IntSize::from_wh(self.width, self.height)
            .ok_or_else(|| "Lottie produced an invalid canvas size".to_owned())?;
        Pixmap::from_vec(rgba, size)
            .ok_or_else(|| "Lottie produced an invalid RGBA frame".to_owned())
    }
}

/// Cache Lottie parsers across every frame of a bake, just like still assets.
#[derive(Default)]
struct Lotties {
    cache: HashMap<PathBuf, Result<Lottie, String>>,
}

impl Lotties {
    fn frame(&mut self, path: &Path, at: f64, looped: bool) -> Result<Pixmap, String> {
        let loaded = self
            .cache
            .entry(path.to_path_buf())
            .or_insert_with(|| Lottie::load(path));
        match loaded {
            Ok(animation) => animation.frame(at, looped),
            Err(error) => Err(error.clone()),
        }
    }
}

fn read_bounded_file(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("cannot inspect Lottie asset {}: {error}", path.display()))?;
    if metadata.len() > maximum {
        return Err(format!(
            "Lottie asset exceeds the {}-byte input limit",
            maximum
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("cannot read Lottie asset {}: {error}", path.display()))?;
    if bytes.len() as u64 > maximum {
        return Err(format!("Lottie asset exceeds the {}-byte input limit", maximum));
    }
    Ok(bytes)
}

/// Read only the manifest-selected animation from a v1 or v2 dotLottie ZIP.
/// Entry names are constructed from a validated id; archive paths are never
/// extracted to disk and decompression is bounded before reading.
fn read_dotlottie_json(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = read_bounded_file(path, MAX_DOTLOTTIE_ARCHIVE_BYTES)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("invalid dotLottie ZIP archive: {error}"))?;
    let mut manifest_file = archive
        .by_name("manifest.json")
        .map_err(|error| format!("dotLottie manifest.json is missing: {error}"))?;
    if manifest_file.size() > MAX_DOTLOTTIE_MANIFEST_BYTES {
        return Err("dotLottie manifest exceeds the 1 MiB limit".into());
    }
    let mut manifest_bytes = Vec::new();
    manifest_file
        .by_ref()
        .take(MAX_DOTLOTTIE_MANIFEST_BYTES + 1)
        .read_to_end(&mut manifest_bytes)
        .map_err(|error| format!("cannot read dotLottie manifest: {error}"))?;
    if manifest_bytes.len() as u64 > MAX_DOTLOTTIE_MANIFEST_BYTES {
        return Err("dotLottie manifest exceeds the 1 MiB limit".into());
    }
    drop(manifest_file);

    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("invalid dotLottie manifest: {error}"))?;
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| "dotLottie manifest has no string version".to_owned())?;
    let (folder, preferred_id) = if version.starts_with('2') {
        (
            "a",
            manifest
                .get("initial")
                .and_then(|initial| initial.get("animation"))
                .and_then(Value::as_str),
        )
    } else if version.starts_with('1') {
        (
            "animations",
            manifest.get("activeAnimationId").and_then(Value::as_str),
        )
    } else {
        return Err(format!(
            "unsupported dotLottie manifest version {version:?}"
        ));
    };
    let animations = manifest
        .get("animations")
        .and_then(Value::as_array)
        .ok_or_else(|| "dotLottie manifest has no animations array".to_owned())?;
    let first_id = animations
        .first()
        .and_then(|animation| animation.get("id"))
        .and_then(Value::as_str);
    let id = preferred_id
        .or(first_id)
        .ok_or_else(|| "dotLottie manifest contains no animation id".to_owned())?;
    if id.is_empty()
        || matches!(id, "." | "..")
        || !id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | ' ')
        })
    {
        return Err("dotLottie animation id contains unsafe path characters".into());
    }
    let entry_name = format!("{folder}/{id}.json");
    let mut animation_file = archive
        .by_name(&entry_name)
        .map_err(|error| format!("dotLottie animation {entry_name:?} is missing: {error}"))?;
    if animation_file.size() > MAX_LOTTIE_JSON_BYTES as u64 {
        return Err("dotLottie animation JSON exceeds the 16 MiB limit".into());
    }
    let mut json = Vec::new();
    animation_file
        .by_ref()
        .take(MAX_LOTTIE_JSON_BYTES as u64 + 1)
        .read_to_end(&mut json)
        .map_err(|error| format!("cannot read dotLottie animation: {error}"))?;
    if json.len() > MAX_LOTTIE_JSON_BYTES {
        return Err("dotLottie animation JSON exceeds the 16 MiB limit".into());
    }
    Ok(json)
}

/// The in-process CPU player currently renders vector, solid, null and
/// precomposition layers. Image and text layers are refused explicitly
/// rather than being silently omitted by the renderer.
fn ensure_supported_lottie(json: &[u8]) -> Result<(), String> {
    fn inspect_layers(layers: &[Value]) -> Result<(), String> {
        for layer in layers {
            let kind = layer
                .get("ty")
                .and_then(Value::as_u64)
                .ok_or_else(|| "Lottie layer has no numeric type".to_owned())?;
            match kind {
                0 | 1 | 3 | 4 => {}
                2 => {
                    return Err(
                        "Lottie image layers are not yet supported by the CPU player".into(),
                    );
                }
                5 => {
                    return Err("Lottie text layers are not yet supported by the CPU player".into());
                }
                _ => {
                    return Err(format!(
                        "Lottie layer type {kind} is not supported by the CPU player"
                    ));
                }
            }
        }
        Ok(())
    }

    let value: Value =
        serde_json::from_slice(json).map_err(|error| format!("invalid Lottie JSON: {error}"))?;
    if let Some(layers) = value.get("layers").and_then(Value::as_array) {
        inspect_layers(layers)?;
    }
    if let Some(assets) = value.get("assets").and_then(Value::as_array) {
        for asset in assets {
            if asset.get("p").is_some() {
                return Err("Lottie image assets are not yet supported by the CPU player".into());
            }
            if let Some(layers) = asset.get("layers").and_then(Value::as_array) {
                inspect_layers(layers)?;
            }
        }
    }
    Ok(())
}

/// The decoded pictures one bake is working with, keyed by the file
/// each was read from. A bake walks the same comp's nodes frame after
/// frame; the pictures do not change between frames, and re-decoding
/// them per frame would make a bake cost `frames` times what it must.
#[derive(Default)]
pub(crate) struct Pictures {
    cache: HashMap<PathBuf, Option<Pixmap>>,
    lotties: Lotties,
}

impl Pictures {
    /// The requested frame of a Lottie or dotLottie asset, parsed once per bake.
    /// Errors are cached too, so a broken asset is not re-read on every frame.
    pub(crate) fn lottie_frame(
        &mut self,
        path: &Path,
        at: f64,
        looped: bool,
    ) -> Result<Pixmap, String> {
        self.lotties.frame(path, at, looped)
    }

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

    const SOLID_LOTTIE: &str = r##"{
        "v":"5.7.4","fr":30,"ip":0,"op":30,"w":32,"h":32,"nm":"solid","ddd":0,
        "assets":[],
        "layers":[{"ddd":0,"ind":1,"ty":1,"nm":"Red","sr":1,
          "ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},"p":{"a":0,"k":[16,16,0]},
                "a":{"a":0,"k":[8,8,0]},"s":{"a":0,"k":[100,100,100]}},
          "ao":0,"sw":16,"sh":16,"sc":"#ff0000","ip":0,"op":30,"st":0,"bm":0}]
    }"##;

    #[test]
    fn a_lottie_json_renders_and_its_parser_is_cached() {
        let path = temp("animation.json");
        std::fs::write(&path, SOLID_LOTTIE).unwrap();
        let mut pictures = Pictures::default();
        let first = pictures
            .lottie_frame(&path, 0.0, false)
            .expect("Lottie frame");
        assert_eq!((first.width(), first.height()), (32, 32));
        let center = (16 * 32 + 16) * 4;
        let pixel = first.data().get(center..center + 4).unwrap();
        assert!(pixel[3] > 0, "the Lottie solid should be visible");
        assert!(pixel[0] > pixel[1], "the Lottie solid should be red");
        std::fs::remove_file(&path).unwrap();
        let second = pictures
            .lottie_frame(&path, 0.5, false)
            .expect("cached Lottie parser");
        assert_eq!(first.data(), second.data());
    }

    #[test]
    fn unsupported_lottie_image_and_text_layers_are_reported() {
        for kind in [2, 5, 99] {
            let json = format!(r#"{{"layers":[{{"ty":{kind}}}]}}"#);
            assert!(ensure_supported_lottie(json.as_bytes()).is_err());
        }
    }

    #[test]
    fn dotlottie_v2_uses_the_manifest_initial_animation() {
        use std::io::Write;
        let path = temp("selected.lottie");
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("manifest.json", options.clone()).unwrap();
        writer
            .write_all(
                br#"{"version":"2","initial":{"animation":"selected"},"animations":[{"id":"first"},{"id":"selected"}]}"#,
            )
            .unwrap();
        writer
            .start_file("a/selected.json", options.clone())
            .unwrap();
        writer.write_all(SOLID_LOTTIE.as_bytes()).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(read_dotlottie_json(&path).unwrap(), SOLID_LOTTIE.as_bytes());
    }

    #[test]
    fn dotlottie_v1_uses_the_active_animation_and_rejects_path_traversal() {
        use std::io::Write;
        let path = temp("legacy.lottie");
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("manifest.json", options.clone()).unwrap();
        writer
            .write_all(
                br#"{"version":"1","activeAnimationId":"selected","animations":[{"id":"first"},{"id":"selected"}]}"#,
            )
            .unwrap();
        writer
            .start_file("animations/selected.json", options.clone())
            .unwrap();
        writer.write_all(SOLID_LOTTIE.as_bytes()).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(read_dotlottie_json(&path).unwrap(), SOLID_LOTTIE.as_bytes());

        let unsafe_manifest = br#"{"version":"2","initial":{"animation":"../escape"},"animations":[{"id":"../escape"}]}"#;
        let manifest_path = temp("unsafe.lottie");
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("manifest.json", options).unwrap();
        writer.write_all(unsafe_manifest).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        std::fs::write(&manifest_path, bytes).unwrap();
        assert!(read_dotlottie_json(&manifest_path).is_err());
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
