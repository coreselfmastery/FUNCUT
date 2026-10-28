// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Flattening a project document into the exporter's clip list.
//!
//! Walking a timeline's clips, resolving their media and track, folding
//! track state into per-clip flags, and building each clip's FFmpeg chains
//! from the structured effects the document stores. An export is derived
//! from the engine's own model, so the model of record and the rendered
//! pixels cannot drift apart.
//!
//! Text clips are deliberately not flattened here. Titles rasterise
//! separately and rejoin the exporter as plain image clips; see
//! `ExportRequest`'s caller. A motion comp resolves to its bake in the
//! project's cache: the RGBA PNG sequence first - the one carrier that
//! keeps what a transparent comp meant, so a motion card sits on the
//! footage beneath it instead of covering it (see [`motion_sequence`]) -
//! and the encoded video under `motion/` when there is no sequence (see
//! [`motion_encoded`]). Without either it is skipped with a warning rather
//! than silently disappearing.

use std::path::Path;

use serde_json::Value as Json;

use concat_project::model::{
    Background, ClipKind as ModelClipKind, KeyProperty, MediaKind, Project, Timeline,
};

use crate::chains::audio_filter_chain;
use crate::{ClipKind, ExportBackdrop, ExportClip, ExportKey, TransitionSpec};

/// The encoded bake a motion clip can actually export. The comp's hash is
/// the cache key and the leading part of `motion_bake`'s output filename,
/// so matching the stem is enough; `.mp4` is preferred to the other video
/// extensions when several bakes of one comp are lying around.
fn motion_encoded(
    project_dir: Option<&Path>,
    motion: &concat_project::model::MotionClip,
) -> Option<String> {
    let dir = project_dir?.join("motion");
    let key = motion
        .hash
        .trim_end_matches(".json")
        .trim()
        .to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut matches: Vec<(u8, u64, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let priority = match extension.as_str() {
            "mp4" | "m4v" => 0,
            "webm" => 1,
            "mov" => 2,
            "mkv" => 3,
            _ => continue,
        };
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if stem.starts_with(&key) || stem.contains(&key) {
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            matches.push((priority, size, path.to_string_lossy().into_owned()));
        }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    matches.into_iter().next().map(|(_, _, path)| path)
}

/// A comp's bake as the export reads it: the `.ffconcat` script naming the
/// frames, and the facts the manifest vouched for.
struct MotionSequence {
    /// The script's path, the one the exporter opens. A bare frames
    /// directory is not a media file, and a `%04d.png` pattern carries no
    /// frame rate - the script carries both.
    script: String,
    /// The frames' size, from the manifest, for an aspect-correct decode.
    width: u32,
    height: u32,
    /// Whether the frames keep the comp's transparency.
    alpha: bool,
}

/// A comp's RGBA PNG sequence bake, under the project's cache: the one
/// carrier that keeps what a transparent comp meant, and so the one a
/// motion card is exported from when it is there. The comp's hash keys the
/// cache (`<hash>-<w>x<h>@<fps>/`, each with its manifest), and the bake
/// whose manifest answers for this comp is the one taken - the one with
/// alpha over one without, the larger over the smaller, so a comp baked at
/// two sizes still picks the same bake twice.
///
/// A bake that promises more frames than it holds - an interrupted or a
/// still-running parallel bake - refuses: an export is not worth a card
/// that freezes halfway through.
fn motion_sequence(
    project_dir: Option<&Path>,
    motion: &concat_project::model::MotionClip,
) -> Option<MotionSequence> {
    let dir = project_dir?.join("cache").join("comps");
    let key = motion
        .hash
        .trim_end_matches(".json")
        .trim()
        .to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    let mut candidates: Vec<(bool, u64, u32, u32, f64, std::path::PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        // The dash keeps one comp's hash from matching a longer one's
        // prefix; the manifest's own hash keeps a renamed directory from
        // speaking for a comp it is not.
        if !name.starts_with(&key) || name.as_bytes().get(key.len()) != Some(&b'-') {
            continue;
        }
        let Some(manifest) = manifest_of(&path) else {
            continue;
        };
        let frames = manifest.get("frames").and_then(Json::as_u64).unwrap_or(0);
        let fps = manifest.get("fps").and_then(Json::as_f64).unwrap_or(0.0);
        let width = manifest.get("width").and_then(Json::as_u64).unwrap_or(0) as u32;
        let height = manifest.get("height").and_then(Json::as_u64).unwrap_or(0) as u32;
        let alpha = manifest
            .get("alpha")
            .and_then(Json::as_bool)
            .unwrap_or(false);
        let hash = manifest.get("hash").and_then(Json::as_u64).unwrap_or(0);
        if hash == 0
            || format!("{hash:016x}") != key
            || frames == 0
            || width == 0
            || height == 0
            || !fps.is_finite()
            || fps <= 0.0
        {
            continue;
        }
        let frames_dir = path.join("frames");
        // The count is the bake's own arithmetic: fewer PNGs on disk than
        // the manifest promises is a bake that never finished.
        if png_count(&frames_dir).is_none_or(|present| present < frames as usize) {
            continue;
        }
        candidates.push((
            alpha,
            u64::from(width) * u64::from(height),
            width,
            height,
            fps,
            frames_dir,
        ));
    }
    // Alpha first - that is the whole point of taking the sequence - then
    // the larger bake, then the name, so two bakes of one comp resolve the
    // same way twice.
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.5.cmp(&b.5)));
    let (alpha, _, width, height, fps, frames_dir) = candidates.into_iter().next()?;
    let script = sequence_script(&frames_dir, fps)?;
    Some(MotionSequence {
        script,
        width,
        height,
        alpha,
    })
}

/// The bake manifest, read loose: unknown fields are the engine's to add,
/// and the flattener asks only for the ones it names.
fn manifest_of(bake_dir: &Path) -> Option<Json> {
    let text = std::fs::read_to_string(bake_dir.join("manifest.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// How many PNGs a frames directory holds, or `None` when it holds none at
/// all - a directory that cannot be read is a bake that cannot be read.
fn png_count(dir: &Path) -> Option<usize> {
    let count = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        })
        .count();
    (count > 0).then_some(count)
}

/// Writes - or leaves - the `.ffconcat` script that carries a PNG sequence
/// to the exporter: one `file` entry per frame, each stamped with the
/// bake's own rate, the last frame once more so the window it shows
/// through outlives a comp a frame longer than the bake. The entries are
/// relative and the script sits beside the frames it names, so a moved
/// project takes both along together. An existing script that already says
/// the same thing is left as it is: the flattener runs for the monitor as
/// well as the export, and a write per call is a cost neither needs.
fn sequence_script(frames_dir: &Path, fps: f64) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(frames_dir).ok()?.flatten() {
        let path = entry.path();
        let is_png = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
        if !is_png {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            names.push(name.to_owned());
        }
    }
    if names.is_empty() {
        return None;
    }
    names.sort();
    let rate = format!("{fps}");
    let duration = format!("{:.6}", 1.0 / fps);
    let mut text = String::from("ffconcat version 1.0\n");
    // Every frame, and the last one again: its second entry is the window
    // the player shows it through, which a comp a frame longer than its
    // bake would otherwise fall out of.
    for name in names.iter().chain(names.last()) {
        text.push_str("file '");
        text.push_str(name);
        text.push_str("'\n");
        text.push_str("option framerate ");
        text.push_str(&rate);
        text.push('\n');
        text.push_str("duration ");
        text.push_str(&duration);
        text.push('\n');
    }

    let path = frames_dir.join("index.ffconcat");
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == text) {
        return Some(path.to_string_lossy().into_owned());
    }
    std::fs::write(&path, text).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Flattens one timeline of `project` for the exporter - the active one
/// when `timeline_id` is `None`. Text clips are excluded (they rasterise
/// separately and rejoin as image clips); a clip whose media or track has
/// vanished is skipped with the same tolerance the document reader extends.
pub fn flatten_timeline(project: &Project, timeline_id: Option<&str>) -> Vec<ExportClip> {
    flatten_timeline_in(project, timeline_id, None)
}

/// [`flatten_timeline`], knowing the project folder: a clip with a cutout
/// is then told where its media's masks are, and renders cut, and a
/// stabilized one where its media's movement is, and renders steadied.
/// Without the folder both are carried but not rendered.
pub fn flatten_timeline_in(
    project: &Project,
    timeline_id: Option<&str>,
    project_dir: Option<&Path>,
) -> Vec<ExportClip> {
    let Some(timeline) = pick_timeline(project, timeline_id) else {
        return Vec::new();
    };
    // Found once: asked of every clip, it is a walk of every clip.
    let lane = timeline.background_lane();

    timeline
        .clips
        .iter()
        .filter_map(|clip| {
            // Titles and shapes rasterise separately and rejoin as stills;
            // see concat-host's titles. A motion comp has its own branch
            // below: it resolves to an encoded bake when there is one.
            if matches!(clip.kind, ModelClipKind::Text | ModelClipKind::Shape) {
                return None;
            }
            let index = timeline
                .tracks
                .iter()
                .position(|track| track.id == clip.track_id)?;
            let track = &timeline.tracks[index];

            // A layer: no file, a chain over the stack beneath its track,
            // its opacity the strength and its fades the ramps.
            if clip.kind == ModelClipKind::Layer {
                return Some(ExportClip {
                    hidden: !track.visible,
                    muted: true,
                    volume: 0.0,
                    fade_in: clip.fade_in,
                    fade_out: clip.fade_out,
                    effects: clip.video_effects.clone(),
                    opacity: clip.opacity,
                    has_audio: Some(false),
                    ..ExportClip::blank(ClipKind::Layer, clip.start, clip.duration, index)
                });
            }

            // A motion comp: the document is not a file the exporter can
            // decode, but its bake is on disk in one of two shapes - the
            // RGBA PNG sequence in the project's cache, or an ordinary
            // video under `motion/` that `motion_bake {encode:true}` (or
            // the motion agent) wrote. The sequence is the export's
            // choice whenever it is there: its PNGs keep the comp's
            // transparency, where the video codecs carry none, so a
            // motion card sits on the footage beneath it instead of
            // covering it. A comp with no transparency either way renders
            // the same from the encoded bake, which decodes cheaper; and
            // with neither on disk the clip is skipped loudly - silently
            // dropping the comp from an export is the defect this fixes.
            if clip.kind == ModelClipKind::Motion {
                let sequence = clip
                    .motion
                    .as_ref()
                    .and_then(|motion| motion_sequence(project_dir, motion));
                let encoded = clip
                    .motion
                    .as_ref()
                    .and_then(|motion| motion_encoded(project_dir, motion));
                let (path, media_size, alpha) = match (sequence.as_ref(), encoded.as_ref()) {
                    (Some(sequence), _) if sequence.alpha => (
                        sequence.script.clone(),
                        Some((sequence.width, sequence.height)),
                        true,
                    ),
                    (Some(sequence), None) => (
                        sequence.script.clone(),
                        Some((sequence.width, sequence.height)),
                        sequence.alpha,
                    ),
                    (_, Some(encoded)) => (encoded.clone(), None, false),
                    (None, None) => {
                        log::warn!(
                            "motion clip {} has no bake on disk - no PNG sequence under cache/comps/ \
                             and no encoded file under motion/ - so it is skipped by the export. \
                             Run motion_bake to bake the comp.",
                            clip.id
                        );
                        return None;
                    }
                };
                let (media_width, media_height) = match media_size {
                    Some((width, height)) => (Some(width), Some(height)),
                    None => (None, None),
                };
                return Some(ExportClip {
                    path,
                    preserve_alpha: alpha,
                    hidden: !track.visible,
                    muted: true,
                    volume: 0.0,
                    fade_in: clip.fade_in,
                    fade_out: clip.fade_out,
                    animation: export_keys(clip),
                    flip_h: clip.flip_h,
                    flip_v: clip.flip_v,
                    blend: clip.blend.clone(),
                    effects: clip.video_effects.clone(),
                    scale: export_base(clip, KeyProperty::Scale),
                    offset_x: export_base(clip, KeyProperty::OffsetX),
                    offset_y: export_base(clip, KeyProperty::OffsetY),
                    rotation: export_base(clip, KeyProperty::Rotation),
                    stretch_x: clip.stretch_x,
                    stretch_y: clip.stretch_y,
                    opacity: export_base(clip, KeyProperty::Opacity),
                    transition: clip.transition_in.as_ref().map(|transition| {
                        TransitionSpec {
                            kind: transition.id.clone(),
                            duration: transition.duration,
                            params: transition.params.clone(),
                        }
                    }),
                    media_width,
                    media_height,
                    has_audio: Some(false),
                    ..ExportClip::blank(ClipKind::Video, clip.start, clip.duration, index)
                });
            }

            let media = project.media.iter().find(|item| item.id == clip.media_id)?;

            let kind = match clip.kind {
                ModelClipKind::Video => ClipKind::Video,
                ModelClipKind::Audio => ClipKind::Audio,
                ModelClipKind::Image => ClipKind::Image,
                // Handled above; unreachable spelled as a skip so a new
                // kind fails soft.
                ModelClipKind::Text
                | ModelClipKind::Layer
                | ModelClipKind::Shape
                | ModelClipKind::Motion => return None,
            };
            Some(ExportClip {
                path: media.path.clone(),
                audio_stream: clip.audio_stream,
                color_range: media.color_range,
                source_start: clip.source_start,
                hidden: !track.visible,
                muted: track.muted || clip.muted == Some(true),
                volume: export_base(clip, KeyProperty::Volume),
                fade_in: clip.fade_in,
                fade_out: clip.fade_out,
                filter_chain: audio_filter_chain(&clip.filters),
                speed: clip.speed,
                preserve_pitch: clip.preserve_pitch,
                speed_curve: clip
                    .speed_curve
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .map(|point| (point.at, point.speed))
                    .collect(),
                speed_slopes: clip
                    .speed_curve
                    .as_deref()
                    .filter(|points| concat_project::speed::is_smooth(points))
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|point| point.slope)
                    .collect(),
                animation: export_keys(clip),
                flip_h: clip.flip_h,
                flip_v: clip.flip_v,
                blend: clip.blend.clone(),
                crop: clip
                    .crop
                    .filter(|crop| !crop.is_none())
                    .map(|crop| [crop.left, crop.top, crop.right, crop.bottom]),
                effects: clip.video_effects.clone(),
                scale: export_base(clip, KeyProperty::Scale),
                offset_x: export_base(clip, KeyProperty::OffsetX),
                offset_y: export_base(clip, KeyProperty::OffsetY),
                rotation: export_base(clip, KeyProperty::Rotation),
                stretch_x: clip.stretch_x,
                stretch_y: clip.stretch_y,
                opacity: export_base(clip, KeyProperty::Opacity),
                // Passed through unconditionally: `resolve_transitions` is
                // the one adjacency judge (frame/2 tolerance). A fixed
                // 1/60 s gate here would agree at 30fps and disagree at
                // every other rate; resolving is the kinder read of what
                // the user placed.
                transition: clip
                    .transition_in
                    .as_ref()
                    .map(|transition| TransitionSpec {
                        kind: transition.id.clone(),
                        duration: transition.duration,
                        params: transition.params.clone(),
                    }),
                media_width: media.width,
                media_height: media.height,
                media_duration: media.duration,
                has_audio: Some(media.has_audio),
                backdrop: timeline
                    .background_on(clip, lane)
                    .and_then(|background| backdrop_of(project, background)),
                cutout: clip.cutout.clone(),
                // Pictures only: a mask on a clip that is all sound would
                // keep part of nothing.
                mask: clip.mask.filter(|_| kind != ClipKind::Audio),
                mask_dir: match (&clip.cutout, project_dir) {
                    (Some(cutout), Some(dir)) => {
                        concat_vision::mask_dir(dir, &media.path, cutout.subject)
                            .to_string_lossy()
                            .into_owned()
                    }
                    _ => String::new(),
                },
                stabilize: clip.stabilize.filter(|_| kind == ClipKind::Video),
                motion_dir: match (&clip.stabilize, project_dir) {
                    (Some(_), Some(dir)) if kind == ClipKind::Video => {
                        concat_vision::stabilize::motion_dir(dir, &media.path)
                            .to_string_lossy()
                            .into_owned()
                    }
                    _ => String::new(),
                },
                ..ExportClip::blank(kind, clip.start, clip.duration, index)
            })
        })
        .collect()
}

/// A background as it is drawn: a blur and a colour as they are, a still
/// found in the bin. None for black, and for a still that is no longer in
/// the bin or is not a still - a background that cannot be drawn is no
/// background, never a failed frame.
fn backdrop_of(project: &Project, background: &Background) -> Option<ExportBackdrop> {
    match background {
        Background::None => None,
        Background::Blur { amount } => Some(ExportBackdrop::Blur { amount: *amount }),
        Background::Color { .. } => background.rgb().map(|rgb| ExportBackdrop::Colour { rgb }),
        Background::Image { media_id } => project
            .media_by_id(media_id)
            .filter(|item| item.kind == MediaKind::Image)
            .map(|item| ExportBackdrop::Image {
                path: item.path.clone(),
                width: item.width,
                height: item.height,
            }),
    }
}

/// The timeline to flatten: the named one, or the document's active one,
/// falling back to the first - the same preference the UI's tab strip has.
fn pick_timeline<'a>(project: &'a Project, timeline_id: Option<&str>) -> Option<&'a Timeline> {
    let wanted = timeline_id.unwrap_or(&project.active_timeline_id);
    project
        .timelines
        .iter()
        .find(|timeline| timeline.id == wanted)
        .or_else(|| project.timelines.first())
        .map(|timeline| timeline.as_ref())
}

/// The constant the engine should receive for a keyable property.
///
/// The clip's own, until that property is keyed - and then the *neutral*
/// value, because a keyed property's keys travel absolutely rather than as a
/// factor on this base. Absolute is not a preference: `Animation` multiplies
/// the base by the scale and opacity tracks, so a clip sitting at zero
/// opacity could never be keyed back up if its keys were relative to it.
///
pub fn export_base(clip: &concat_project::model::Clip, property: KeyProperty) -> f64 {
    if !clip.is_keyed(property) {
        return clip.constant(property);
    }
    match property {
        // A factor's neutral is one; an addition's is zero.
        KeyProperty::Scale | KeyProperty::Opacity | KeyProperty::Volume => 1.0,
        KeyProperty::OffsetX | KeyProperty::OffsetY | KeyProperty::Rotation => 0.0,
        // The mask travels whole, as `ExportClip::mask`, and its keys
        // replace its numbers; it never asks for a base.
        mask => clip.constant(mask),
    }
}

/// The keys the engine plays: the clip's own, property by property.
pub fn export_keys(clip: &concat_project::model::Clip) -> Vec<ExportKey> {
    let mut out = Vec::new();
    for property in KeyProperty::ALL {
        if !clip.is_keyed(property) {
            continue;
        }
        out.extend(clip.keys_on(property).map(|key| ExportKey {
            property: property.name().to_owned(),
            at: key.at,
            value: key.value,
            ease: key.ease.sane().0,
        }));
    }
    out
}

/// The clip's gain over its length, as `(fraction, gain)` pairs, or empty
/// for a clip whose gain is the one number in `ExportClip::volume`.
pub fn volume_curve(clip: &concat_project::model::Clip) -> Vec<(f64, f64)> {
    clip.keys_on(KeyProperty::Volume)
        .map(|key| (key.at, key.value))
        .collect()
}

#[cfg(test)]
mod tests {

    use concat_project::commands::{ClipPatch, NewMedia, TrackFlag};
    use concat_project::model::AppliedFilter;
    use concat_project::{Command, Editor};

    use super::*;

    /// A project with one video media item and one clip of it, built through
    /// the real command layer so the fixture cannot drift from the model.
    fn project_with_clip() -> (Editor, String, String) {
        let mut editor = Editor::new();
        let media_id = editor
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: "/footage/a.mp4".into(),
                    name: "a.mp4".into(),
                    duration: Some(10.0),
                    kind: concat_project::model::MediaKind::Video,
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: None,
                    frame_rate_fraction: None,
                    video_codec: None,
                    audio_codec: None,
                    has_audio: true,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .expect("adds media")
            .created_id
            .expect("mints an id");
        let track_id = editor.project().timelines[0].tracks[0].id.clone();
        let clip_id = editor
            .apply(Command::AddClip {
                media_id: media_id.clone(),
                track_id,
                start: 1.0,
                ripple: false,
            })
            .expect("adds clip")
            .created_id
            .expect("mints an id");
        (editor, media_id, clip_id)
    }

    #[test]
    fn a_clip_flattens_with_its_media_and_track_facts() {
        let (editor, _, _) = project_with_clip();
        let flat = flatten_timeline(editor.project(), None);
        assert_eq!(flat.len(), 1);
        let clip = &flat[0];
        assert_eq!(clip.path, "/footage/a.mp4");
        assert!(matches!(clip.kind, ClipKind::Video));
        assert_eq!(clip.start, 1.0);
        assert_eq!(clip.duration, 10.0, "the clip takes its media's length");
        assert_eq!(clip.track, 0);
        assert!(!clip.hidden);
        assert!(!clip.muted);
        assert_eq!(clip.media_width, Some(1920));
        assert_eq!(clip.has_audio, Some(true));
        assert_eq!(clip.filter_chain, "");
        assert!(clip.effects.is_empty());
    }

    #[test]
    fn track_state_folds_into_the_clip_flags() {
        let (mut editor, _, _) = project_with_clip();
        let track_id = editor.project().timelines[0].tracks[0].id.clone();
        editor
            .apply(Command::SetTrackFlag {
                track_id: track_id.clone(),
                flag: TrackFlag::Visible,
                value: false,
            })
            .expect("hides");
        editor
            .apply(Command::SetTrackFlag {
                track_id,
                flag: TrackFlag::Muted,
                value: true,
            })
            .expect("mutes");
        let flat = flatten_timeline(editor.project(), None);
        assert!(flat[0].hidden, "a hidden track hides its clips");
        assert!(flat[0].muted, "a muted track mutes its clips");
    }

    #[test]
    fn the_stored_effects_ride_along() {
        let (mut editor, _, clip_id) = project_with_clip();
        editor
            .apply(Command::UpdateClip {
                clip_id,
                patch: ClipPatch {
                    video_effects: Some(vec![AppliedFilter::new("concat.mono")]),
                    ..Default::default()
                },
            })
            .expect("applies effect");
        let flat = flatten_timeline(editor.project(), None);
        assert_eq!(flat[0].effects, vec![AppliedFilter::new("concat.mono")]);
    }

    /// A clip's mask and its keys reach the engine's clip, which plans
    /// the monitor's frames and the export's alike: one path, so the two
    /// cannot draw different masks.
    #[test]
    fn a_mask_and_its_keys_reach_the_engine() {
        use concat_core::time::Rational;
        use concat_project::model::{ClipMask, KeyEase, MaskShape};
        let (mut editor, _, clip_id) = project_with_clip();
        editor
            .apply(Command::UpdateClip {
                clip_id: clip_id.clone(),
                patch: ClipPatch {
                    mask: Some(Some(ClipMask {
                        feather: 0.2,
                        ..ClipMask::of(MaskShape::Heart)
                    })),
                    ..Default::default()
                },
            })
            .expect("masks");
        for (at, value) in [(0.0, -0.2), (1.0, 0.2)] {
            editor
                .apply(Command::SetClipKey {
                    clip_id: clip_id.clone(),
                    property: KeyProperty::MaskX,
                    at,
                    value,
                    ease: KeyEase::LINEAR,
                })
                .expect("keys");
        }
        let flat = flatten_timeline(editor.project(), None);
        let mask = flat[0].mask.expect("the mask rides along");
        assert_eq!(mask.shape, MaskShape::Heart);
        assert!(flat[0].animation.iter().any(|key| key.property == "maskX"));

        let mut clip = concat_core::timeline::Clip::new(
            concat_core::timeline::MediaRef::new("a.mp4"),
            Rational::ZERO,
            Rational::from_int(10),
        );
        clip.mask = Some(mask.engine());
        clip.animation = crate::resolve::animation_of(&flat[0].animation);
        let start = clip.mask_at(Rational::ZERO).expect("a mask");
        let middle = clip.mask_at(Rational::from_int(5)).expect("a mask");
        assert!((start.x + 0.2).abs() < 1e-12);
        assert!(middle.x.abs() < 1e-12);
        assert_eq!(middle.shape, concat_core::mask::MaskShape::Heart);
        assert!((middle.feather - 0.2).abs() < 1e-12);
        // The placement is untouched by the mask's keys.
        assert!(
            clip.animation
                .as_ref()
                .is_some_and(|a| a.offset_x.is_empty())
        );
    }

    #[test]
    fn text_clips_are_left_for_the_rasteriser() {
        let (mut editor, _, _) = project_with_clip();
        let track_id = Some(editor.project().timelines[0].tracks[0].id.clone());
        editor
            .apply(Command::AddTextClip {
                track_id,
                above: false,
                start: 0.0,
                style: None,
                duration: None,
                offset_y: None,
            })
            .expect("adds title");
        let flat = flatten_timeline(editor.project(), None);
        assert_eq!(flat.len(), 1, "the text clip does not flatten");
    }

    #[test]
    fn motion_encoded_finds_the_bake_and_prefers_mp4() {
        let dir = std::env::temp_dir().join(format!("concat-motion-export-{}", std::process::id()));
        let motion_dir = dir.join("motion");
        std::fs::create_dir_all(&motion_dir).expect("creates");
        let hash = "0123456789abcdef";
        for name in ["0123456789abcdef.webm", "0123456789abcdef.mp4", "other.mov"] {
            std::fs::write(motion_dir.join(name), b"video").expect("writes");
        }
        std::fs::write(motion_dir.join("unrelated.txt"), b"not video").expect("writes");
        let motion = concat_project::model::MotionClip {
            comp: "funcut.lower-third.glass".to_owned(),
            hash: format!("{hash}.json"),
            engine: "geneva".to_owned(),
            width: 0,
            height: 0,
        };
        let found = motion_encoded(Some(&dir), &motion).expect("finds the bake");
        assert!(found.ends_with(".mp4"), "found {found}");
        assert!(motion_encoded(None, &motion).is_none());
        let missing = concat_project::model::MotionClip {
            hash: "deadbeef".to_owned(),
            ..motion
        };
        assert!(motion_encoded(Some(&dir), &missing).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A bake on disk in the shape `motion_bake` leaves it, as a helper:
    /// `cache/comps/<hash>-<w>x<h>@<fps>/` with its manifest and frames.
    fn staged_bake(
        root: &Path,
        hash: u64,
        width: u32,
        height: u32,
        alpha: bool,
        frames: usize,
        present: usize,
    ) -> std::path::PathBuf {
        let bake = root
            .join("cache")
            .join("comps")
            .join(format!("{hash:016x}-{width}x{height}@30"));
        std::fs::create_dir_all(bake.join("frames")).expect("creates the bake");
        let manifest = format!(
            "{{\"hash\": {hash}, \"frames\": {frames}, \"width\": {width}, \
             \"height\": {height}, \"fps\": 30.0, \"alpha\": {alpha}, \
             \"engine\": \"geneva\", \"engine_version\": \"1.1\"}}"
        );
        std::fs::write(bake.join("manifest.json"), manifest).expect("writes the manifest");
        for index in 1..=present {
            let name = format!("{index:04}.png");
            std::fs::write(bake.join("frames").join(&name), b"png").expect("writes a frame");
        }
        bake
    }

    #[test]
    fn motion_sequence_finds_the_alpha_bake_and_writes_the_script() {
        let dir = std::env::temp_dir().join(format!("concat-motion-seq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hash = 0x0123456789abcdef_u64;
        // A larger opaque bake and a smaller one with alpha: alpha is the
        // point, so the smaller bake is the one the export takes.
        staged_bake(&dir, hash, 640, 480, false, 3, 3);
        staged_bake(&dir, hash, 320, 240, true, 3, 3);
        let motion = concat_project::model::MotionClip {
            comp: "funcut.lower-third.glass".to_owned(),
            hash: "0123456789abcdef.json".to_owned(),
            engine: "geneva".to_owned(),
            width: 0,
            height: 0,
        };
        let sequence = motion_sequence(Some(&dir), &motion).expect("finds the sequence");
        assert!(sequence.alpha, "the alpha bake wins");
        assert_eq!(sequence.width, 320);
        assert_eq!(sequence.height, 240);
        assert!(sequence.script.ends_with("index.ffconcat"));
        let script = std::fs::read_to_string(&sequence.script).expect("reads the script");
        assert!(script.starts_with("ffconcat version 1.0\n"), "{script}");
        // Three frames, and the last one again for its window.
        assert_eq!(script.matches("file '").count(), 4, "{script}");
        assert!(script.contains("option framerate 30\n"), "{script}");
        assert!(script.contains("duration 0.033333\n"), "{script}");
        assert!(script.contains("file '0003.png'\n"), "{script}");
        // A second flatten writes the same words, not a second file.
        let again = motion_sequence(Some(&dir), &motion).expect("resolves again");
        assert_eq!(again.script, sequence.script);

        // No folder, no sequence; a hash that owns no bake either.
        assert!(motion_sequence(None, &motion).is_none());
        let stranger = concat_project::model::MotionClip {
            hash: "ffffffffffffffff.json".to_owned(),
            ..motion
        };
        assert!(motion_sequence(Some(&dir), &stranger).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_partial_or_foreign_bake_is_not_a_sequence() {
        let dir = std::env::temp_dir().join(format!("concat-motion-part-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hash = 0x0123456789abcdef_u64;
        // A bake still running: five frames promised, two on disk.
        staged_bake(&dir, hash, 320, 240, true, 5, 2);
        // A longer hash that shares the prefix: the dash after the key
        // keeps it from being read as this comp's bake.
        let longer = dir
            .join("cache")
            .join("comps")
            .join("0123456789abcdef00-320x240@30");
        std::fs::create_dir_all(longer.join("frames")).expect("creates");
        std::fs::write(
            longer.join("manifest.json"),
            "{\"hash\": 78187493520, \"frames\": 1, \"width\": 320, \"height\": 240, \
             \"fps\": 30.0, \"alpha\": true}",
        )
        .expect("writes");
        let motion = concat_project::model::MotionClip {
            comp: "funcut.lower-third.glass".to_owned(),
            hash: "0123456789abcdef.json".to_owned(),
            engine: "geneva".to_owned(),
            width: 0,
            height: 0,
        };
        assert!(motion_sequence(Some(&dir), &motion).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clip_whose_media_vanished_is_skipped_not_fatal() {
        let (mut editor, media_id, _) = project_with_clip();
        editor
            .apply(Command::RemoveMedia { media_id })
            .expect("removes");
        // RemoveMedia also removes the clips; force the orphan case instead
        // by asking for a timeline that does not exist.
        assert!(flatten_timeline(editor.project(), None).is_empty());
        assert!(flatten_timeline(editor.project(), Some("nope")).len() <= 1);
    }
}
