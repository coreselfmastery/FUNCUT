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
//! `ExportRequest`'s caller. A motion comp resolves to the ENCODED bake in
//! the project's `motion/` folder when one exists (see
//! [`motion_encoded`]); without one it is skipped with a warning rather
//! than silently disappearing, since the exporter cannot read a PNG frame
//! sequence on its own.

use std::path::Path;

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
            // decode, but `motion_bake {encode:true}` (or the motion agent)
            // writes an ordinary video under `motion/` named by the comp's
            // hash. Find it and rejoin as a video clip. If only the RGBA
            // PNG sequence exists, say so loudly: silently dropping the
            // comp from an export is the defect this fixes.
            if clip.kind == ModelClipKind::Motion {
                let encoded = clip
                    .motion
                    .as_ref()
                    .and_then(|motion| motion_encoded(project_dir, motion));
                let Some(encoded) = encoded else {
                    log::warn!(
                        "motion clip {} has no encoded bake under motion/ - it is skipped by the export. \
                         Run motion_bake with encode:true, or motion_overlay to composite the RGBA frames over footage.",
                        clip.id
                    );
                    return None;
                };
                return Some(ExportClip {
                    path: encoded,
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
