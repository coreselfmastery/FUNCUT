// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The scene-aware reel cache: one scene, one entry.
//!
//! A reel re-rendered on every edit is a reel nobody iterates on: the
//! whole ten-scene document renders again because one layer of one scene
//! moved ten pixels. This module renders a storyboard **scene by scene**,
//! and caches each scene by a key derived from what actually draws it:
//!
//! - the scene's own JSON - its comp or preset, its layers, its timing,
//!   its operations;
//! - the **bytes** of every asset it names - the footage, the comps, the
//!   fonts - hashed, so a re-saved picture with the same name misses;
//! - the geometry it renders at - width, height, frame rate;
//! - the engine and its version, and the timeline format version;
//! - the curtain at its boundaries - the opening fade for the first
//!   scene, the dip depth at each cut, the closing fade for the last.
//!
//! Nothing else enters the key: an edit to scene seven does not change
//! scene three's key, and scene three's segment is served from the cache
//! untouched. The joined reel is cached by the ordered list of its
//! scenes' keys, so a re-render where nothing changed copies the joined
//! file - byte-identical, in no time at all - and a re-render where one
//! scene changed re-renders that scene, re-joins, and nothing else.
//!
//! Audio beds are reel-level, not scene-level: each bed renders once
//! (audio-only, cached by its own key - the bed's numbers and its
//! file's bytes) and is mixed over the joined picture at the end.
//!
//! ```text
//! render twice, unchanged     →  every scene served, the join served: 0 renders
//! change one layer of scene 3 →  scene 3 renders; 1, 2, 4..n serve; the join re-runs
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::json;

use crate::comp::Asset;
use crate::engine::MotionError;
use crate::geneva::Geneva;
use crate::sequence::{Curtain, Sequence};
use crate::storyboard::Storyboard;

/// One scene's outcome from a cached render.
#[derive(Clone, PartialEq, Debug)]
pub struct SceneOutcome {
    /// The scene's index in the storyboard.
    pub index: usize,
    /// Its cache key, as hex.
    pub key: String,
    /// Whether the segment was served from the cache.
    pub cached: bool,
}

/// A reel render's report: what rendered, what was served, how long it
/// took. The `renders` count is the number of scene segments actually
/// drawn - the number a caller advertises when it says "an unchanged
/// reel re-renders nothing".
#[derive(Clone, PartialEq, Debug)]
pub struct ReelReport {
    /// One entry per scene, in order.
    pub scenes: Vec<SceneOutcome>,
    /// How many scene segments were drawn (the rest were served).
    pub renders: usize,
    /// Whether the joined reel itself was served whole from the cache.
    pub joined_from_cache: bool,
    /// Whether the audio bed was drawn (false when there is none, or it
    /// was served).
    pub audio_rendered: bool,
    /// Wall-clock seconds the whole call took.
    pub seconds: f64,
    /// The file written.
    pub out: PathBuf,
}

/// The FNV-1a the crate's own hashes use: not a security hash, a name.
fn fnv(bytes: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(b"concat-motion/reel-cache/1");
    for chunk in bytes {
        eat(chunk);
    }
    hash
}

/// The bytes of a file, hashed: the *content* an asset carries, not the
/// name it answers to - a re-saved picture misses, a renamed one does
/// not have to.
fn file_hash(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    Some(fnv(&[&bytes]))
}

/// The key of one scene: everything that draws it, hashed. See the
/// module docs for the list; the order of the list is the order of the
/// hash, and a field added to it is a cache miss for every scene once -
/// which is the honest cost of a key that forgot something.
pub fn scene_key(board: &Storyboard, root: &Path, index: usize) -> String {
    let scene = board.scenes.get(index).expect("the scene exists");
    let mut parts: Vec<Vec<u8>> = Vec::new();

    // The scene's own document, canonically written: the same scene, the
    // same bytes, whatever order the author's JSON carried.
    let document = json!({
        "scene": {
            "preset": scene.preset,
            "comp": scene.comp,
            "at": scene.at,
            "hold": scene.hold,
            "plate": scene.plate,
            "zoom": scene.zoom,
            "layers": scene.layers,
            "ops": scene.ops,
        },
        "geometry": [board.width, board.height, board.fps],
        "format": [crate::geneva::TIMELINE_VERSION, crate::geneva::TESTED_VERSION],
        "curtain": board.curtain.as_ref().map(|c| json!([
            c.open, c.close, c.dip, c.depth, c.color,
            // The boundaries this scene sits at: the first scene carries
            // the opening fade, the last the closing one, and every cut
            // carries its dip.
            index == 0,
            index + 1 == board.scenes.len(),
        ])),
    });
    parts.push(
        serde_json::to_string(&document)
            .unwrap_or_default()
            .into_bytes(),
    );

    // The bytes of every asset the scene names: plate, layers, comp
    // file. (Fonts a comp names come through the assets map too.)
    let mut named: Vec<String> = Vec::new();
    if let Some(plate) = &scene.plate {
        named.push(plate.clone());
    }
    for layer in &scene.layers {
        match layer.source() {
            Ok(crate::storyboard::LayerSource::Asset(asset)) => named.push(asset),
            Ok(crate::storyboard::LayerSource::Scene(name)) => {
                // A nested scene's content is the referenced scene's own
                // key: its document, its assets, recursively.
                if let Some(at) = board
                    .scenes
                    .iter()
                    .position(|c| c.id.as_deref() == Some(name.as_str()))
                {
                    named.push(scene_key(board, root, at));
                } else {
                    named.push(name.clone());
                }
            }
            Err(_) => named.push(String::new()),
        }
    }
    for id in &named {
        if let Some(src) = board.assets.get(id)
            && let Some(hash) = file_hash(&root.join(src))
        {
            parts.push(id.as_bytes().to_vec());
            parts.push(hash.to_le_bytes().to_vec());
        }
    }
    if let Some(comp) = &scene.comp
        && let Some(hash) = file_hash(&root.join(comp))
    {
        parts.push(comp.as_bytes().to_vec());
        parts.push(hash.to_le_bytes().to_vec());
    }

    format!(
        "{:016x}",
        fnv(&parts.iter().map(|p| p.as_slice()).collect::<Vec<&[u8]>>())
    )
}

/// The key of the joined reel: the ordered keys of its scenes, and the
/// encode the caller asked for. One scene changing changes the join's
/// key; nothing changing changes nothing.
fn join_key(keys: &[String], crf: u8, name: &str) -> String {
    let mut parts: Vec<Vec<u8>> = keys.iter().map(|k| k.as_bytes().to_vec()).collect();
    parts.push(vec![crf]);
    parts.push(name.as_bytes().to_vec());
    format!(
        "{:016x}",
        fnv(&parts.iter().map(|p| p.as_slice()).collect::<Vec<&[u8]>>())
    )
}

/// The key of one audio bed: its numbers and its file's bytes.
fn bed_key(board: &Storyboard, root: &Path, index: usize) -> String {
    let bed = board.audio.get(index).expect("the bed exists");
    let mut parts: Vec<Vec<u8>> = vec![serde_json::to_string(bed).unwrap_or_default().into_bytes()];
    if let Some(src) = board.assets.get(&bed.asset)
        && let Some(hash) = file_hash(&root.join(src))
    {
        parts.push(bed.asset.as_bytes().to_vec());
        parts.push(hash.to_le_bytes().to_vec());
    }
    format!(
        "{:016x}",
        fnv(&parts.iter().map(|p| p.as_slice()).collect::<Vec<&[u8]>>())
    )
}

/// Render a storyboard scene by scene, caching each scene by its key.
///
/// `cache` is the directory segments and joins live in (created when
/// missing); `out` is the reel file written. The engine is asked for a
/// scene only when its segment is not there; the join is copied from the
/// cache when every scene in it was served and the joined file for
/// exactly these keys exists.
pub fn render_reel(
    geneva: &Geneva,
    board: &Storyboard,
    root: &Path,
    out: &Path,
    cache: &Path,
    crf: u8,
) -> Result<ReelReport, MotionError> {
    let started = Instant::now();
    board.validate().map_err(|problems| MotionError::Engine {
        code: None,
        message: problems
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join("; "),
        path: None,
        help: None,
    })?;
    let segments = cache.join("scenes");
    let joins = cache.join("joins");
    std::fs::create_dir_all(&segments).map_err(|source| MotionError::Io {
        doing: "creating the scene cache".into(),
        path: segments.clone(),
        source,
    })?;
    std::fs::create_dir_all(&joins).map_err(|source| MotionError::Io {
        doing: "creating the join cache".into(),
        path: joins.clone(),
        source,
    })?;

    // One key per scene, one segment per key. A hit is a manifest that
    // names the same key and a file that is still there.
    let mut outcomes: Vec<SceneOutcome> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut renders = 0_usize;
    for index in 0..board.scenes.len() {
        let key = scene_key(board, root, index);
        let segment = segments.join(format!("{key}.mp4"));
        let served = segment.is_file()
            && std::fs::read_to_string(segment.with_extension("done"))
                .map(|text| text.trim() == key)
                .unwrap_or(false);
        if !served {
            render_scene_segment(geneva, board, root, index, &segment, crf)?;
            std::fs::write(segment.with_extension("done"), &key).map_err(|source| {
                MotionError::Io {
                    doing: "writing the scene's cache marker".into(),
                    path: segment.with_extension("done"),
                    source,
                }
            })?;
            renders += 1;
        }
        outcomes.push(SceneOutcome {
            index,
            key: key.clone(),
            cached: served,
        });
        keys.push(key);
    }

    // The join: served whole when exactly these keys were joined before
    // and every scene in them was served.
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let joined_key = join_key(&keys, crf, &name);
    let joined_cache = joins.join(format!("{joined_key}.mp4"));
    let every_scene_served = outcomes.iter().all(|o| o.cached);
    let mut joined_from_cache = false;
    if every_scene_served && joined_cache.is_file() {
        // The join is served, not re-run: byte-identical by construction,
        // because it is the same file.
        std::fs::copy(&joined_cache, out).map_err(|source| MotionError::Io {
            doing: "serving the joined reel from the cache".into(),
            path: joined_cache.clone(),
            source,
        })?;
        joined_from_cache = true;
    } else {
        join_segments(
            geneva,
            &outcomes
                .iter()
                .map(|o| segments.join(format!("{}.mp4", o.key)))
                .collect::<Vec<PathBuf>>(),
            out,
        )?;
        let _ = std::fs::copy(out, &joined_cache);
    }

    // The beds, when there are any: each renders once, audio-only, and
    // mixes over the joined picture, one at a time, the last one landing
    // back on `out`.
    let mut audio_rendered = false;
    if !board.audio.is_empty() {
        let beds = cache.join("beds");
        std::fs::create_dir_all(&beds).map_err(|source| MotionError::Io {
            doing: "creating the bed cache".into(),
            path: beds.clone(),
            source,
        })?;
        let mut current = out.to_path_buf();
        for index in 0..board.audio.len() {
            let key = bed_key(board, root, index);
            let bed_audio = beds.join(format!("{key}.m4a"));
            if !bed_audio.is_file() {
                render_bed(geneva, board, root, index, &bed_audio)?;
                audio_rendered = true;
            }
            let last = index + 1 == board.audio.len();
            let next = if last {
                out.with_extension("mixing.mp4")
            } else {
                out.with_extension(format!("mix-{index}.mp4"))
            };
            mix_audio(geneva, &current, &bed_audio, &next)?;
            if current != *out {
                let _ = std::fs::remove_file(&current);
            }
            current = next;
        }
        // The last mix is the reel.
        std::fs::rename(&current, out).map_err(|source| MotionError::Io {
            doing: "moving the mixed reel into place".into(),
            path: current.clone(),
            source,
        })?;
    }

    Ok(ReelReport {
        scenes: outcomes,
        renders,
        joined_from_cache,
        audio_rendered,
        seconds: started.elapsed().as_secs_f64(),
        out: out.to_path_buf(),
    })
}

/// Render one scene as a segment: a single-scene sequence with the
/// curtain marks its boundaries carry - the opening fade when it is
/// first, a dip at each cut, the closing fade when it is last.
fn render_scene_segment(
    geneva: &Geneva,
    board: &Storyboard,
    root: &Path,
    index: usize,
    segment: &Path,
    crf: u8,
) -> Result<(), MotionError> {
    let mut sub = Sequence::new(board.width, board.height, board.fps);
    sub.source_dir = root.to_path_buf();
    // The whole storyboard's cards, so a layer naming a scene finds it
    // and the segment carries the nested composition.
    sub.scene_cards = board.scenes.clone();
    for (id, src) in &board.assets {
        sub.asset(
            id.clone(),
            Asset {
                src: src.clone(),
                kind: None,
            },
        );
    }
    let scene = board
        .scenes
        .get(index)
        .expect("the scene exists")
        .scene(index, root)
        .map_err(|message| MotionError::Engine {
            code: None,
            message,
            path: None,
            help: None,
        })?;
    let first = index == 0;
    let last = index + 1 == board.scenes.len();
    if let Some(curtain) = &board.curtain {
        sub.curtain = Some(Curtain {
            color: crate::comp::Rgba::hex(&curtain.color),
            // A middle scene dips at both ends; the first opens, the last
            // closes, and the dip half-width stays what the author wrote.
            open: if first { curtain.open } else { curtain.dip },
            close: if last { curtain.close } else { curtain.dip },
            dip: curtain.dip,
            depth: curtain.depth,
        });
    }
    sub.push(scene);
    let planned = crate::sequence::plan(geneva, &sub)?;
    let mut document = planned.document;
    document["output"]["encode"]["video"]["crf"] = json!(crf);
    let staged = segment.with_extension("stage");
    let render = crate::sequence::Plan {
        document,
        files: planned.files,
        warnings: planned.warnings,
        duration: planned.duration,
        scenes: 1,
        source_root: root.to_path_buf(),
    };
    render.render(geneva, &staged, segment)?;
    // The staged timeline is scratch; the segment and its marker are the
    // cache's pair, and the marker is written only after the render's own
    // report said ok - render() has already refused otherwise.
    let _ = std::fs::remove_dir_all(&staged);
    Ok(())
}

/// Render one audio bed, audio-only, through a timeline of its own.
fn render_bed(
    geneva: &Geneva,
    board: &Storyboard,
    root: &Path,
    index: usize,
    bed_audio: &Path,
) -> Result<(), MotionError> {
    let bed = board.audio.get(index).expect("the bed exists");
    let duration: f64 = board
        .scenes
        .iter()
        .enumerate()
        .map(|(i, card)| {
            let at = card.at.unwrap_or(0.0);
            let _ = i;
            at + card.hold.unwrap_or(6.0)
        })
        .fold(f64::MIN, f64::max)
        .max(1.0);
    let mut document = json!({
        "geneva": crate::geneva::TIMELINE_VERSION,
        "output": {
            "width": board.width, "height": board.height, "fps": board.fps,
            "duration": format!("{duration:.3}s"),
            "audio": { "codec": "aac" },
        },
        "assets": { bed.asset.clone(): {
            "src": format!("assets/{}", board.assets.get(&bed.asset).cloned().unwrap_or_default()),
            "kind": "audio",
        }},
        "audio": [{
            "id": format!("bed.{}", bed.asset),
            "clips": [{
                "asset": bed.asset,
                "start": "0s",
                "duration": format!("{duration:.3}s"),
                "gain_db": bed.gain_db,
                "fade_in": format!("{}s", bed.fade_in.unwrap_or(0.0).min(duration / 3.0)),
                "fade_out": format!("{}s", bed.fade_out.unwrap_or(0.0).min(duration / 3.0)),
            }],
        }],
    });
    let _ = &mut document;
    let dir = bed_audio.with_extension("stage");
    std::fs::create_dir_all(dir.join("assets")).map_err(|source| MotionError::Io {
        doing: "creating the bed's render directory".into(),
        path: dir.clone(),
        source,
    })?;
    if let Some(src) = board.assets.get(&bed.asset) {
        let from = root.join(src);
        let name = Path::new(src)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "bed".into());
        let to = dir.join("assets").join(&name);
        std::fs::copy(&from, &to).map_err(|source| MotionError::Io {
            doing: "copying the bed's audio into the render".into(),
            path: from,
            source,
        })?;
        // The timeline names the staged file.
        let staged_src = format!("assets/{name}");
        document["assets"][&bed.asset]["src"] = json!(staged_src);
    }
    std::fs::write(
        dir.join("timeline.json"),
        serde_json::to_string_pretty(&document).unwrap_or_default(),
    )
    .map_err(|source| MotionError::Io {
        doing: "writing the bed's timeline".into(),
        path: dir.join("timeline.json"),
        source,
    })?;
    geneva
        .run_in(
            &dir,
            &[
                "--format".into(),
                "json".into(),
                "render".into(),
                "timeline.json".into(),
                "--audio-only".into(),
                "-o".into(),
                bed_audio.display().to_string(),
            ],
        )
        .map(|_| ())
}

/// A path's parent, or `.` when it has none worth naming: "reel.mp4"'s
/// parent is the empty string, and a child asked to run in the empty
/// string's directory is refused with the one error that looks like the
/// executable is missing.
fn working_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Join the segments, back to back, in order. A reel of one scene is
/// its own join - the engine's concat takes two inputs or more, and a
/// copy is faster and exactly the bytes anyway.
fn join_segments(geneva: &Geneva, segments: &[PathBuf], out: &Path) -> Result<(), MotionError> {
    if segments.len() == 1 {
        return std::fs::copy(&segments[0], out)
            .map(|_| ())
            .map_err(|source| MotionError::Io {
                doing: "copying the one scene's segment into place".into(),
                path: segments[0].clone(),
                source,
            });
    }
    let dir = working_dir(out);
    let mut args: Vec<String> = vec![
        "--format".into(),
        "json".into(),
        "concat".into(),
        "-o".into(),
        out.display().to_string(),
    ];
    for segment in segments {
        // concat runs from the output's directory; the segments are named
        // in full so they resolve wherever they live.
        let _ = &dir;
        args.push(segment.display().to_string());
    }
    geneva.run_in(&dir, &args).map(|_| ())
}

/// Mix one bed's audio over the picture.
fn mix_audio(geneva: &Geneva, video: &Path, audio: &Path, out: &Path) -> Result<(), MotionError> {
    let dir = out
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    geneva
        .run_in(
            &dir,
            &[
                "--format".into(),
                "json".into(),
                "audio".into(),
                "--mix".into(),
                audio.display().to_string(),
                "-o".into(),
                out.display().to_string(),
                video.display().to_string(),
            ],
        )
        .map(|_| ())
}

/// The engine's own probe, for a caller deciding what to draw with; see
/// [`EnginePreference::choose`], which folds the breaker into the choice.
pub fn sidecar_state(geneva: Option<&Geneva>) -> crate::engine::Sidecar {
    match geneva {
        None => crate::engine::Sidecar::Missing,
        Some(geneva) => match geneva.breaker() {
            crate::geneva::Breaker::Closed => {
                if geneva.probe().available {
                    crate::engine::Sidecar::Ready
                } else {
                    crate::engine::Sidecar::Missing
                }
            }
            crate::geneva::Breaker::Open { failures } => {
                crate::engine::Sidecar::Failing { failures }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cache's own logic is tested against a stub engine (see the
    // guard tests in geneva.rs for the same pattern): a script that
    // records every invocation and renders deterministically, so the
    // tests assert what was rendered and what was served, not what a
    // real encoder happened to write.
    #[cfg(unix)]
    mod with_a_stub {
        use super::*;
        use crate::storyboard::Storyboard;
        use std::os::unix::fs::PermissionsExt;
        use tempfile::TempDir;

        fn serial() -> std::sync::MutexGuard<'static, ()> {
            let guard = crate::geneva::BREAKER_TEST_LOCK
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            crate::geneva::reset_breaker_for_tests();
            guard
        }

        fn stub_geneva(dir: &TempDir) -> Geneva {
            let path = dir.path().join("geneva-stub");
            std::fs::write(
                &path,
                concat!(
                    "#!/bin/sh\n",
                    "# The verb follows the options; find it.\n",
                    "cmd=\"\"\n",
                    "out=\"\"\n",
                    "prev=\"\"\n",
                    "for arg in \"$@\"; do\n",
                    "  case \"$arg\" in render|concat|audio|frame) cmd=\"$arg\" ;; esac\n",
                    "  if [ \"$prev\" = \"-o\" ]; then out=\"$arg\"; fi\n",
                    "  prev=\"$arg\"\n",
                    "done\n",
                    "echo \"$cmd $*\" >> \"$(dirname \"$0\")/calls.log\"\n",
                    "case \"$cmd\" in\n",
                    "  concat)\n",
                    "    : > \"$out\"\n",
                    "    for input in \"$@\"; do\n",
                    "      [ \"$input\" = \"$out\" ] && continue\n",
                    "      [ -f \"$input\" ] && cat \"$input\" >> \"$out\"\n",
                    "    done\n",
                    "    exit 0 ;;\n",
                    "  audio)\n",
                    "    cp \"${@: -1}\" \"$out\" 2>/dev/null || : > \"$out\"\n",
                    "    exit 0 ;;\n",
                    "  render)\n",
                    "    # A deterministic segment: its bytes come from the\n",
                    "    # timeline it was asked to render, hashed through the\n",
                    "    # only tool every Unix has.\n",
                    "    sum=$(cksum < timeline.json 2>/dev/null || echo 0)\n",
                    "    printf 'segment %s\\n' \"$sum\" > \"$out\"\n",
                    "    exit 0 ;;\n",
                    "esac\n",
                    "echo '{\"ok\":true,\"diagnostics\":[],\"summary\":{}}'\n",
                ),
            )
            .expect("the stub is written");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("the stub is executable");
            let log = dir.path().join("calls.log");
            let _ = std::fs::remove_file(&log);
            Geneva::at(&path)
        }

        fn board_text() -> String {
            r##"{
              "assets": { "back": "back.png", "card": "card.png" },
              "scenes": [
                { "preset": "funcut.sting.wordmark", "hold": 2.0,
                  "layers": [ { "asset": "back", "zoom": 0.04 } ] },
                { "preset": "funcut.chapter.chip", "hold": 2.0,
                  "layers": [ { "asset": "card", "parallax": 1.5 } ] }
              ]
            }"##
            .to_owned()
        }

        fn setup(dir: &TempDir) -> (Storyboard, PathBuf) {
            let root = dir.path().join("project");
            std::fs::create_dir_all(root.join("assets")).unwrap();
            std::fs::write(root.join("back.png"), b"back-bytes-v1").unwrap();
            std::fs::write(root.join("card.png"), b"card-bytes-v1").unwrap();

            let board = Storyboard::parse(&board_text()).expect("a storyboard");
            (board, root)
        }

        #[test]
        fn a_second_render_of_an_unchanged_reel_renders_nothing_and_is_byte_identical() {
            let _serial = serial();
            let dir = TempDir::new().unwrap();
            let geneva = stub_geneva(&dir);
            let (board, root) = setup(&dir);
            let cache = dir.path().join("cache");
            let out = dir.path().join("reel.mp4");

            let first = render_reel(&geneva, &board, &root, &out, &cache, 18).expect("renders");
            assert_eq!(first.renders, 2, "both scenes draw the first time");
            assert!(!first.joined_from_cache);
            let first_bytes = std::fs::read(&out).unwrap();

            let (board, root) = setup(&dir);
            let second = render_reel(&geneva, &board, &root, &out, &cache, 18).expect("renders");
            assert_eq!(second.renders, 0, "an unchanged reel re-renders nothing");
            assert!(second.joined_from_cache, "the join is served whole");
            assert_eq!(
                std::fs::read(&out).unwrap(),
                first_bytes,
                "byte-identical, because it is the same file"
            );
        }

        #[test]
        fn one_layer_changed_re_renders_only_its_scene() {
            let _serial = serial();
            let dir = TempDir::new().unwrap();
            let geneva = stub_geneva(&dir);
            let (board, root) = setup(&dir);
            let cache = dir.path().join("cache");
            let out = dir.path().join("reel.mp4");
            render_reel(&geneva, &board, &root, &out, &cache, 18).expect("renders");

            // Scene 1's layer changes: the parallax rate. Scene 0's key
            // must not move.
            let key0_before = scene_key(&board, &root, 0);
            let key1_before = scene_key(&board, &root, 1);
            let text = board_text().replace("\"parallax\": 1.5", "\"parallax\": 2.5");
            let changed = Storyboard::parse(&text).expect("a storyboard");
            assert_eq!(
                scene_key(&changed, &root, 0),
                key0_before,
                "scene 0's key does not move"
            );
            assert_ne!(
                scene_key(&changed, &root, 1),
                key1_before,
                "scene 1's key moves"
            );

            let second = render_reel(&geneva, &changed, &root, &out, &cache, 18).expect("renders");
            assert_eq!(second.renders, 1, "only the changed scene draws");
            assert!(second.scenes[0].cached, "scene 0 is served");
            assert!(!second.scenes[1].cached, "scene 1 is drawn");
            assert!(!second.joined_from_cache, "the join re-runs");
        }

        #[test]
        fn an_assets_bytes_are_part_of_its_scene_key() {
            let _serial = serial();
            let dir = TempDir::new().unwrap();
            let (board, root) = setup(&dir);
            let before = scene_key(&board, &root, 0);
            // The same name, different bytes: a re-saved picture.
            std::fs::write(root.join("back.png"), b"back-bytes-v2").unwrap();
            let after = scene_key(&board, &root, 0);
            assert_ne!(before, after, "content, not names, is what caches");
            // And it is scene 0's asset: scene 1's key does not move.
            let one = scene_key(&board, &root, 1);
            std::fs::write(root.join("back.png"), b"back-bytes-v3").unwrap();
            assert_eq!(one, scene_key(&board, &root, 1));
        }

        #[test]
        fn the_geometry_and_the_engine_are_part_of_the_key() {
            let _serial = serial();
            let dir = TempDir::new().unwrap();
            let (board, root) = setup(&dir);
            let before = scene_key(&board, &root, 0);
            let mut other = board.clone();
            other.fps = 60.0;
            assert_ne!(
                before,
                scene_key(&other, &root, 0),
                "the frame rate is in the key"
            );
        }
    }
}
