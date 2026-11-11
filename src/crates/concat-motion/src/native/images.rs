// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Stills a comp points at, decoded once per bake: PNG, JPEG and WebP,
//! the three extensions the asset table calls an image, cached by the
//! path they were read from so three hundred frames pay for one decode.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use rasterlottie::{
    Animation as LottieAnimation, Asset as LottieAsset, PreparedAnimation, RasterlottieError,
    RenderConfig, Renderer,
};
use serde_json::Value;
use tiny_skia::Pixmap;
use zip::{ZipArchive, result::ZipError};


const MAX_LOTTIE_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOTTIE_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DOTLOTTIE_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DOTLOTTIE_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_DOTLOTTIE_ENTRIES: usize = 512;
const MAX_DOTLOTTIE_UNCOMPRESSED_BYTES: u64 = 128 * 1024 * 1024;

/// A preprocessed Lottie animation whose parsed scene is reused for every frame.
struct Lottie {
    prepared: PreparedAnimation,
    width: u32,
    height: u32,
    frame_rate: f32,
    in_point: f32,
    out_point: f32,
}

impl Lottie {
    fn load(path: &Path) -> Result<Self, String> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        let animation = match extension.as_deref() {
            Some("json") => {
                let bytes = read_bounded_file(path, MAX_LOTTIE_JSON_BYTES as u64)?;
                let json = std::str::from_utf8(&bytes)
                    .map_err(|error| format!("Lottie JSON is not UTF-8: {error}"))?;
                LottieAnimation::from_json_str(json)
                    .map_err(|error| format!("Lottie JSON parse failed: {error}"))?
            }
            Some("lottie") => {
                let bytes = read_bounded_file(path, MAX_DOTLOTTIE_ARCHIVE_BYTES)?;
                read_dotlottie_animation(&bytes)?
            }
            _ => return Err("expected a .json Lottie or .lottie dotLottie asset".into()),
        };

        let (width, height, frame_rate, in_point, out_point) = (
            animation.width,
            animation.height,
            animation.frame_rate,
            animation.in_point,
            animation.out_point,
        );
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || width as u64 * height as u64 > 16 * 1024 * 1024
            || !frame_rate.is_finite()
            || frame_rate <= 0.0
            || !in_point.is_finite()
            || !out_point.is_finite()
            || out_point <= in_point
        {
            return Err("Lottie canvas or frame range is outside the supported limits".into());
        }

        let renderer = Renderer::target_corpus();
        let prepared = if extension.as_deref() == Some("json") {
            let base_dir = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
            let resolver = |asset: &LottieAsset| resolve_external_image(&base_dir, asset);
            renderer
                .prepare_with_resolver(&animation, &resolver)
                .map_err(|error| format!("Lottie support or image validation failed: {error}"))?
        } else {
            renderer
                .prepare(&animation)
                .map_err(|error| format!("dotLottie support validation failed: {error}"))?
        };

        Ok(Self {
            prepared,
            width,
            height,
            frame_rate,
            in_point,
            out_point,
        })
    }

    fn frame(&self, at: f64, looped: bool) -> Result<Pixmap, String> {
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
        let frame_number = self.in_point + (local_time * f64::from(self.frame_rate)) as f32;
        let frame_number = frame_number.min(self.out_point - 1.0).max(self.in_point);
        let frame = self
            .prepared
            .render_frame(frame_number, RenderConfig::default())
            .map_err(|error| format!("Lottie frame render failed: {error}"))?;
        let size = tiny_skia::IntSize::from_wh(frame.width, frame.height)
            .ok_or_else(|| "Lottie produced an invalid canvas size".to_owned())?;
        Pixmap::from_vec(frame.pixels, size)
            .ok_or_else(|| "Lottie produced an invalid RGBA frame".to_owned())
    }
}

/// Cache parsed animations, including failures, so a bad asset is not reparsed
/// on every frame of a preview or export.
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
    let file = File::open(path)
        .map_err(|error| format!("cannot open asset {}: {error}", path.display()))?;
    let size = file
        .metadata()
        .map_err(|error| format!("cannot inspect asset {}: {error}", path.display()))?
        .len();
    if size > maximum {
        return Err(format!("asset exceeds the {maximum}-byte input limit"));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read asset {}: {error}", path.display()))?;
    if bytes.len() as u64 > maximum {
        return Err(format!("asset exceeds the {maximum}-byte input limit"));
    }
    Ok(bytes)
}

fn resolve_external_image(
    base_dir: &Path,
    asset: &LottieAsset,
) -> Result<Option<Vec<u8>>, RasterlottieError> {
    let Some(path) = asset.path.as_deref() else {
        return Ok(None);
    };
    let Some(relative_path) = safe_relative_path(path) else {
        return Err(RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail: "image path must stay inside the animation asset directory".into(),
        });
    };
    let mut relative = PathBuf::new();
    if let Some(base) = asset.base_path.as_deref() {
        let Some(base) = safe_relative_path(base) else {
            return Err(RasterlottieError::InvalidImageAsset {
                id: asset.id.clone(),
                detail: "image base path must stay inside the animation asset directory".into(),
            });
        };
        relative.push(base);
    }
    relative.push(relative_path);
    let full_path = base_dir.join(relative);
    read_bounded_file(&full_path, MAX_LOTTIE_IMAGE_BYTES)
        .map(Some)
        .map_err(|detail| RasterlottieError::InvalidImageAsset {
            id: asset.id.clone(),
            detail,
        })
}

fn safe_relative_path(path: &str) -> Option<PathBuf> {
    let normalized = path.replace('\\', "/");
    let candidate = Path::new(&normalized);
    if candidate.is_absolute() {
        return None;
    }
    let mut safe = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(safe)
}

fn read_dotlottie_animation(bytes: &[u8]) -> Result<LottieAnimation, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("invalid dotLottie ZIP archive: {error}"))?;
    validate_dotlottie_archive(&mut archive)?;

    let manifest_bytes = read_zip_entry(
        &mut archive,
        "manifest.json",
        MAX_DOTLOTTIE_MANIFEST_BYTES,
    )?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("invalid dotLottie manifest: {error}"))?;
    let selected_id = manifest
        .get("initial")
        .and_then(|initial| initial.get("animation"))
        .and_then(Value::as_str)
        .or_else(|| manifest.get("activeAnimationId").and_then(Value::as_str))
        .or_else(|| {
            manifest
                .get("animations")
                .and_then(Value::as_array)
                .and_then(|animations| animations.first())
                .and_then(|animation| animation.get("id"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| "dotLottie manifest does not select or list an animation".to_owned())?;
    if selected_id.is_empty()
        || matches!(selected_id, "." | "..")
        || !selected_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | ' ')
        })
    {
        return Err("dotLottie animation id contains unsafe path characters".into());
    }

    let id = selected_id.trim_end_matches(".json");
    let candidates = [
        selected_id.to_owned(),
        format!("{id}.json"),
        format!("a/{id}.json"),
        format!("animations/{id}.json"),
    ];
    let mut animation_json = None;
    for candidate in candidates {
        match read_zip_entry(&mut archive, &candidate, MAX_LOTTIE_JSON_BYTES) {
            Ok(bytes) => {
                animation_json = Some(bytes);
                break;
            }
            Err(error) if error.starts_with("missing archive entry") => {}
            Err(error) => return Err(error),
        }
    }
    let animation_json = animation_json
        .ok_or_else(|| "dotLottie archive does not contain the selected animation JSON".to_owned())?;
    let json = std::str::from_utf8(&animation_json)
        .map_err(|error| format!("dotLottie animation JSON is not UTF-8: {error}"))?;
    let mut animation = LottieAnimation::from_json_str(json)
        .map_err(|error| format!("dotLottie animation JSON parse failed: {error}"))?;
    embed_dotlottie_images(&mut archive, &mut animation)?;
    Ok(animation)
}

fn validate_dotlottie_archive(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<(), String> {
    if archive.len() > MAX_DOTLOTTIE_ENTRIES {
        return Err("dotLottie archive contains too many entries".into());
    }
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect dotLottie entry: {error}"))?;
        if safe_relative_path(file.name()).is_none() {
            return Err("dotLottie archive contains an unsafe entry path".into());
        }
        let size = file.size();
        if size > MAX_LOTTIE_IMAGE_BYTES {
            return Err("dotLottie archive entry exceeds the 32 MiB limit".into());
        }
        total_size = total_size
            .checked_add(size)
            .filter(|size| *size <= MAX_DOTLOTTIE_UNCOMPRESSED_BYTES)
            .ok_or_else(|| "dotLottie archive exceeds the total uncompressed-size limit".to_owned())?;
    }
    Ok(())
}

fn read_zip_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    maximum: usize,
) -> Result<Vec<u8>, String> {
    let mut file = archive.by_name(path).map_err(|error| match error {
        ZipError::FileNotFound => format!("missing archive entry {path:?}"),
        other => format!("cannot open dotLottie entry {path:?}: {other}"),
    })?;
    if file.size() > maximum as u64 {
        return Err(format!("dotLottie entry {path:?} exceeds the {maximum}-byte limit"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read dotLottie entry {path:?}: {error}"))?;
    if bytes.len() > maximum {
        return Err(format!("dotLottie entry {path:?} exceeds the {maximum}-byte limit"));
    }
    Ok(bytes)
}

fn embed_dotlottie_images(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    animation: &mut LottieAnimation,
) -> Result<(), String> {
    for asset in &mut animation.assets {
        if !asset.is_image_asset() || asset.is_embedded_image_asset() {
            continue;
        }
        let candidates = archive_image_candidates(asset)?;
        let mut image_bytes = None;
        for candidate in candidates {
            match read_zip_entry(archive, &candidate, MAX_LOTTIE_IMAGE_BYTES as usize) {
                Ok(bytes) => {
                    image_bytes = Some((candidate, bytes));
                    break;
                }
                Err(error) if error.starts_with("missing archive entry") => {}
                Err(error) => return Err(error),
            }
        }
        let (path, bytes) = image_bytes.ok_or_else(|| {
            format!("dotLottie image asset {:?} is missing from the archive", asset.id)
        })?;
        let media_type = media_type_for_path(&path);
        asset.base_path = None;
        asset.embedded = Some(1);
        asset.path = Some(format!("data:{media_type};base64,{}", STANDARD.encode(bytes)));
    }
    Ok(())
}

fn archive_image_candidates(asset: &LottieAsset) -> Result<Vec<String>, String> {
    let Some(path) = asset.path.as_deref() else {
        return Ok(Vec::new());
    };
    let relative = safe_relative_path(path)
        .ok_or_else(|| format!("dotLottie image path for {:?} is unsafe", asset.id))?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    if relative.is_empty() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    if let Some(base) = asset.base_path.as_deref() {
        let base = safe_relative_path(base)
            .ok_or_else(|| format!("dotLottie image base path for {:?} is unsafe", asset.id))?;
        let base = base.to_string_lossy().replace('\\', "/");
        if !base.is_empty() {
            candidates.push(format!("{base}/{relative}"));
        }
    }
    candidates.push(relative.clone());
    candidates.push(format!("i/{relative}"));
    candidates.push(format!("images/{relative}"));
    if let Some(file_name) = relative.rsplit('/').next()
        && file_name != relative
    {
        candidates.push(format!("i/{file_name}"));
        candidates.push(format!("images/{file_name}"));
    }
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

fn media_type_for_path(path: &str) -> &'static str {
    let extension = path.rsplit('.').next().unwrap_or_default();
    if extension.eq_ignore_ascii_case("png") {
        "image/png"
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        "image/jpeg"
    } else if extension.eq_ignore_ascii_case("webp") {
        "image/webp"
    } else if extension.eq_ignore_ascii_case("gif") {
        "image/gif"
    } else if extension.eq_ignore_ascii_case("bmp") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
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
    fn invalid_lottie_json_is_rejected_with_a_diagnostic() {
        let path = temp("invalid.json");
        std::fs::write(&path, r#"{"v":"5.7.4","fr":30,"ip":0,"op":30,"w":32,"h":32,"layers":[{"ty":99}]}"#).unwrap();
        assert!(Lottie::load(&path).is_err());
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
        let animation = read_dotlottie_animation(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!((animation.width, animation.height), (32, 32));
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
        let animation = read_dotlottie_animation(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!((animation.width, animation.height), (32, 32));

        let unsafe_manifest = br#"{"version":"2","initial":{"animation":"../escape"},"animations":[{"id":"../escape"}]}"#;
        let manifest_path = temp("unsafe.lottie");
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("manifest.json", options).unwrap();
        writer.write_all(unsafe_manifest).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        std::fs::write(&manifest_path, bytes).unwrap();
        assert!(read_dotlottie_animation(&std::fs::read(&manifest_path).unwrap()).is_err());
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
