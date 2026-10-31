// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The contact sheet a reel becomes, so the critic can look at the scene.
//!
//! `motion_reel` with `sheet: true` renders four stills of the reel -
//! a quarter in, half, three quarters, and near the end, where the last
//! frame a scene's exit lives is - and tiles them into one 2x2 image
//! with the engine's own frames, no Python and no second pass. The
//! sheet is returned as an inline image *and* written under the
//! project's `cache/sense/`, which is the path `craft_critique` takes:
//! the creative loop's look-then-revise, closed over the reel's own
//! output.

use std::path::Path;

use serde_json::{Value as Json, json};

/// Tile four RGBA PNGs into one 2x2 sheet, each cell at half the
/// frame's size. The PNG decode and encode are the `png` crate's own -
/// the same one the API's frame method hands back through.
///
/// A frame that is missing or unreadable is a black cell rather than a
/// refusal: a sheet exists to be looked at, and the empty quarter is
/// itself a finding.
pub fn tile(frames: [&Path; 4], width: u32, height: u32) -> Vec<u8> {
    use png::{ColorType, Encoder};

    let cell_w = (width / 2).max(1);
    let cell_h = (height / 2).max(1);
    let mut sheet = vec![0u8; (cell_w * cell_h * 4 * 4) as usize];

    for (at, path) in frames.iter().enumerate() {
        let cell = read_rgba(path, cell_w, cell_h);
        let (ox, oy) = ((at % 2) as u32 * cell_w, (at / 2) as u32 * cell_h);
        for y in 0..cell_h {
            for x in 0..cell_w {
                let from = ((y * cell_w + x) * 4) as usize;
                let to = (((oy + y) * (cell_w * 2) + ox + x) * 4) as usize;
                let (r, g, b, a) = (cell[from], cell[from + 1], cell[from + 2], cell[from + 3]);
                sheet[to] = r;
                sheet[to + 1] = g;
                sheet[to + 2] = b;
                sheet[to + 3] = a;
            }
        }
    }

    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, cell_w * 2, cell_h * 2);
    encoder.set_color(ColorType::Rgba);
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(&sheet);
    }
    bytes
}

/// One PNG, decoded and scaled to the cell by nearest-neighbour - the
/// frames are already the reel's own size, so this only ever shrinks.
fn read_rgba(path: &Path, cell_w: u32, cell_h: u32) -> Vec<u8> {
    use png::Decoder;
    let bytes = std::fs::read(path).unwrap_or_default();
    let decoder = Decoder::new(bytes.as_slice());
    let Ok(mut reader) = decoder.read_info() else {
        return vec![0u8; (cell_w * cell_h * 4) as usize];
    };
    let (w, h): (u32, u32) = reader.info().size();
    let mut raw = vec![0u8; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut raw) else {
        return vec![0u8; (cell_w * cell_h * 4) as usize];
    };
    let channels = info.color_type.samples() as u32;
    let mut cell = vec![0u8; (cell_w * cell_h * 4) as usize];
    for y in 0..cell_h {
        for x in 0..cell_w {
            // Nearest neighbour: the frames are the reel's own pixels,
            // and a sheet's job is to be looked at, not resampled.
            let sx = (x * w / cell_w.max(1)).min(w.saturating_sub(1));
            let sy = (y * h / cell_h.max(1)).min(h.saturating_sub(1));
            let from = ((sy * w + sx) * channels) as usize;
            let to = ((y * cell_w + x) * 4) as usize;
            let rgba = |at: usize| raw.get(from + at).copied().unwrap_or(255);
            cell[to] = rgba(0);
            cell[to + 1] = rgba(1);
            cell[to + 2] = rgba(2);
            cell[to + 3] = if channels == 4 { rgba(3) } else { 255 };
        }
    }
    cell
}

/// The reading that rides with the sheet: what it shows, and how the
/// critic is told to look at it.
pub fn reading(png_path: &Path, duration: f64, at: &[f64]) -> Json {
    json!({
        "sheet": png_path.display().to_string(),
        "pngPath": png_path.display().to_string(),
        "duration": duration,
        "sampledAt": at,
        "note": "Four stills of the reel, a quarter in to near the end. Feed this to \
                 craft_critique (its `sheet` takes this whole reading, or the pngPath \
                 alone) against the creative brief; a score under 7 is a revision, \
                 not a shrug.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny solid RGBA PNG, written with the encoder the sheet itself
    /// uses - the same crate, the same path, no fixtures to carry.
    fn solid(path: &Path, w: u32, h: u32, rgba: [u8; 4]) {
        use png::{ColorType, Encoder};
        let bytes = [rgba[0], rgba[1], rgba[2], rgba[3]].repeat((w * h) as usize);
        let mut out = Vec::new();
        {
            let mut encoder = Encoder::new(&mut out, w, h);
            encoder.set_color(ColorType::Rgba);
            let mut writer = encoder.write_header().expect("a header");
            writer.write_image_data(&bytes).expect("pixels");
        }
        std::fs::write(path, out).expect("written");
    }

    #[test]
    fn four_frames_tile_into_one_2x2_sheet() {
        let dir = tempfile::tempdir().expect("a dir");
        let (w, h) = (64, 48);
        solid(&dir.path().join("a.png"), w, h, [255, 0, 0, 255]);
        solid(&dir.path().join("b.png"), w, h, [0, 255, 0, 255]);
        solid(&dir.path().join("c.png"), w, h, [0, 0, 255, 255]);
        solid(&dir.path().join("d.png"), w, h, [255, 255, 0, 255]);
        let sheet = tile(
            [
                &dir.path().join("a.png"),
                &dir.path().join("b.png"),
                &dir.path().join("c.png"),
                &dir.path().join("d.png"),
            ],
            w,
            h,
        );
        // Decode the sheet back: four cells, each its own colour.
        let decoder = png::Decoder::new(sheet.as_slice());
        let mut reader = decoder.read_info().expect("the sheet decodes");
        let mut raw = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut raw).expect("pixels");
        let info = reader.info();
        assert_eq!(info.width, w, "two cells wide");
        assert_eq!(info.height, h, "two cells tall");
        let at = |x: u32, y: u32| {
            let i = ((y * info.width + x) * 4) as usize;
            [raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]
        };
        assert_eq!(at(2, 2), [255, 0, 0, 255], "top-left: the first frame");
        assert_eq!(at(w - 3, 2), [0, 255, 0, 255], "top-right: the second");
        assert_eq!(at(2, h - 3), [0, 0, 255, 255], "bottom-left: the third");
        assert_eq!(
            at(w - 3, h - 3),
            [255, 255, 0, 255],
            "bottom-right: the fourth"
        );
    }

    #[test]
    fn a_missing_frame_is_a_black_cell_not_a_refusal() {
        let dir = tempfile::tempdir().expect("a dir");
        let (w, h) = (32, 32);
        solid(&dir.path().join("a.png"), w, h, [255, 255, 255, 255]);
        let nowhere = dir.path().join("nowhere.png");
        let sheet = tile(
            [&dir.path().join("a.png"), &nowhere, &nowhere, &nowhere],
            w,
            h,
        );
        let decoder = png::Decoder::new(sheet.as_slice());
        let mut reader = decoder.read_info().expect("the sheet decodes");
        let mut raw = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut raw).expect("pixels");
        let info = reader.info();
        // The empty three quarters are black - the finding is visible,
        // which is what a sheet is for.
        let i = ((info.height - 2) * info.width + info.width - 2) as usize * 4;
        assert_eq!([raw[i], raw[i + 1], raw[i + 2]], [0, 0, 0]);
    }
}
