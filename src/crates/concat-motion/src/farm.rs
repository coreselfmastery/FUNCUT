// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The resumable farm: a long render, cut into parts that survive their
//! own failure.
//!
//! `geneva farm` is the engine's own answer - it hands the parts to
//! `geneva worker` processes, here and elsewhere, and joins them. What it
//! does not do is *remember*: a farm whose worker dies at part seven of
//! ten starts again at part one, and a forty-minute render that dies at
//! minute thirty-nine is a rerun, not a resume.
//!
//! This module is the other half. The engine's own `plan` verb splits the
//! render (it knows where the good boundaries are - it refuses parts
//! shorter than two seconds and never cuts a frame in half), and this
//! side renders each part with a plain `render --frames` call, **writes
//! the part to the farm's own directory**, and marks it done. A re-run
//! serves every finished part and draws only the missing ones, so a
//! failed worker costs its own part, and nothing else. The parts and the
//! staged timeline live together in a self-contained folder - the unit of
//! work a worker on another machine is handed whole (sync the folder,
//! run the one part command back, and the render directory needs nothing
//! from this machine) - and the join puts them together with the
//! timeline's audio, without a re-encode.
//!
//! ```text
//! first run, 10 parts, dies at 7   →  parts 1..6 are marked done and stay
//! second run                       →  parts 1..6 served, 7..10 drawn, join
//! an unchanged plan, re-run         →  every part served, the join re-runs
//! one edited scene                  →  a new timeline, every part re-rendered
//! ```
//!
//! A part is keyed by the *staged timeline's bytes* and its frame range,
//! so a changed plan - a re-graded scene, a moved layer, a new cut - is a
//! changed key and a re-render, exactly like the reel cache beside it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value as Json, json};

use crate::cache;
use crate::engine::MotionError;
use crate::geneva::Geneva;
use crate::sequence::Plan;

/// The outcome of one part: what it covered, and whether this call drew
/// it or the cache had it whole.
#[derive(Clone, PartialEq, Debug)]
pub struct PartOutcome {
    /// The part's index, in timeline order.
    pub index: usize,
    /// First frame, 0-based as the engine's own plan numbers them.
    pub from: u32,
    /// One past the last frame, as the engine's own plan numbers them.
    pub to: u32,
    /// How many frames the part covers.
    pub frames: u32,
    /// Whether this call drew the part (false when the cache served it).
    pub rendered: bool,
}

/// What a farm render did: every part's outcome, the counts an agent
/// wants to read at a glance, and where the finished file landed.
#[derive(Clone, PartialEq, Debug)]
pub struct FarmReport {
    /// The picture parts, in timeline order.
    pub parts: Vec<PartOutcome>,
    /// The audio part, when the timeline carries sound.
    pub audio: Option<PartOutcome>,
    /// How many parts this call drew (the rest were served).
    pub rendered: usize,
    /// How many parts were served from the farm's own directory.
    pub served: usize,
    /// Wall-clock seconds the whole call took.
    pub seconds: f64,
    /// The file written.
    pub out: PathBuf,
}

/// The key of one part: the staged timeline's bytes and the part's own
/// range. The timeline file *is* the identity - it carries the size, the
/// rate, the crf and every scene - so a part that survives an edit is a
/// part that was rendered from a different document.
fn part_key(timeline: u64, from: u32, to: u32) -> String {
    format!("{timeline:016x}.{from:016x}.{to:016x}",)
}

/// The key a part's marker names, whatever wrote it: the first field.
fn marker_key(text: &str) -> Option<String> {
    text.split_whitespace().next().map(ToString::to_string)
}

/// Ask the engine where the good part boundaries are: `geneva plan`,
/// the same split its own farm hands its workers. The engine refuses
/// parts shorter than two seconds and says so in a note, so the number
/// asked for is a ceiling, not a promise - the report that comes back is
/// the truth.
fn ask_for_parts(
    geneva: &Geneva,
    dir: &Path,
    parts: u32,
    out: &Path,
) -> Result<(Vec<(u32, u32)>, bool), MotionError> {
    let name = out
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "reel.mp4".into());
    let report = geneva.run_in(
        dir,
        &[
            "--format".into(),
            "json".into(),
            "plan".into(),
            "timeline.json".into(),
            "--parts".into(),
            parts.max(1).to_string(),
            "--output".into(),
            name,
        ],
    )?;
    let mut ranges = Vec::new();
    for part in report
        .get("parts")
        .and_then(Json::as_array)
        .unwrap_or(&Vec::new())
    {
        let Some(frames) = part.get("frames").and_then(Json::as_array) else {
            continue;
        };
        let (Some(from), Some(to)) = (
            frames.first().and_then(Json::as_u64),
            frames.get(1).and_then(Json::as_u64),
        ) else {
            continue;
        };
        ranges.push((from as u32, to as u32));
    }
    let has_audio = report
        .get("audio")
        .is_some_and(|audio| audio.get("output").is_some());
    Ok((ranges, has_audio))
}

/// Render one part into `to`, atomically: the engine writes the temp file
/// beside the destination, and the rename is the moment the part exists.
/// A part is never visible half-written, so a worker that reads the farm
/// mid-render sees whole parts or missing ones, and nothing else.
fn render_part(
    geneva: &Geneva,
    dir: &Path,
    from: u32,
    to: u32,
    to_path: &Path,
    audio_only: bool,
) -> Result<(), MotionError> {
    let part = cache::part_path(to_path);
    let mut args: Vec<String> = vec![
        "--format".into(),
        "json".into(),
        "render".into(),
        "timeline.json".into(),
    ];
    if audio_only {
        args.push("--audio-only".into());
    } else {
        args.push("--frames".into());
        args.push(format!("{from}..{to}"));
    }
    args.push("-o".into());
    args.push(part.display().to_string());
    geneva.run_in(dir, &args)?;
    cache::publish(&part, to_path).map_err(|source| MotionError::Io {
        doing: "publishing a farm part".into(),
        path: to_path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Render a plan in resumable parts, under `farm_dir`.
///
/// `dir` is the render directory the timeline was staged into (the
/// folder is self-contained by [`Plan::write`]'s own contract, so it is
/// also the folder a worker elsewhere is handed); `farm_dir` is where
/// the parts and their markers live, and it survives the call - that is
/// its job. `out` is the joined file. `parts` is a ceiling: the engine's
/// plan verb decides the real split, and a timeline too short to split
/// is one part, honestly reported.
pub fn render(
    geneva: &Geneva,
    plan: &Plan,
    dir: &Path,
    farm_dir: &Path,
    out: &Path,
    parts: u32,
) -> Result<FarmReport, MotionError> {
    let started = Instant::now();
    plan.write(dir)?;
    std::fs::create_dir_all(farm_dir).map_err(|source| MotionError::Io {
        doing: "creating the farm's part directory".into(),
        path: farm_dir.to_path_buf(),
        source,
    })?;
    let out = crate::sequence::absolute(out);
    let (ranges, has_audio) = ask_for_parts(geneva, dir, parts, &out)?;
    let timeline = cache::stream_hash("concat-motion/farm/1", &dir.join("timeline.json"))
        .ok_or_else(|| MotionError::Engine {
            code: None,
            message: "the staged timeline could not be read back for the farm key".into(),
            path: None,
            help: None,
        })?;

    let mut report = FarmReport {
        parts: Vec::new(),
        audio: None,
        rendered: 0,
        served: 0,
        seconds: 0.0,
        out: out.clone(),
    };
    let mut part_paths: Vec<PathBuf> = Vec::new();
    for (index, (from, to)) in ranges.iter().copied().enumerate() {
        let key = part_key(timeline, from, to);
        let path = farm_dir.join(format!("part-{index:03}.mp4"));
        let marker = path.with_extension("done");
        let served = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|text| marker_key(&text))
            .is_some_and(|found| found == key)
            && path.is_file();
        if !served {
            render_part(geneva, dir, from, to, &path, false)?;
            cache::mark_done(&path, &key, 0.0).map_err(|source| MotionError::Io {
                doing: "marking a farm part done".into(),
                path: marker.clone(),
                source,
            })?;
        } else {
            cache::touch(&path);
        }
        report.parts.push(PartOutcome {
            index,
            from,
            to,
            frames: to.saturating_sub(from),
            rendered: !served,
        });
        part_paths.push(path);
        if served {
            report.served += 1;
        } else {
            report.rendered += 1;
        }
    }
    // The audio part, when the timeline carries sound: rendered once,
    // cached by its own key, joined without a re-encode.
    if has_audio {
        let key = part_key(timeline, 0, 0);
        let path = farm_dir.join("audio.mp4");
        let marker = path.with_extension("done");
        let served = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|text| marker_key(&text))
            .is_some_and(|found| found == key)
            && path.is_file();
        if !served {
            render_part(geneva, dir, 0, 0, &path, true)?;
            cache::mark_done(&path, &key, 0.0).map_err(|source| MotionError::Io {
                doing: "marking the farm's audio part done".into(),
                path: marker,
                source,
            })?;
        } else {
            cache::touch(&path);
        }
        report.audio = Some(PartOutcome {
            index: report.parts.len(),
            from: 0,
            to: 0,
            frames: 0,
            rendered: !served,
        });
        if served {
            report.served += 1;
        } else {
            report.rendered += 1;
        }
    }
    if report.parts.is_empty() {
        return Err(MotionError::Engine {
            code: None,
            message: "the engine's plan named no parts, so there is nothing to farm".into(),
            path: None,
            help: Some(
                "render the plan plainly; a timeline too short to split is not a farm's job".into(),
            ),
        });
    }

    // The join: the engine's own `join`, which stitches parts - and the
    // audio part - back together without re-encoding a frame.
    let mut args: Vec<String> = vec![
        "--format".into(),
        "json".into(),
        "join".into(),
        "timeline.json".into(),
    ];
    for part in &part_paths {
        args.push(part.display().to_string());
    }
    if let Some(_audio) = &report.audio {
        args.push("--audio".into());
        args.push(farm_dir.join("audio.mp4").display().to_string());
    }
    args.push("-o".into());
    args.push(out.display().to_string());
    geneva.run_in(dir, &args)?;

    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

/// The report as one JSON document, for the agent binary and any caller
/// that answers to a tool.
pub fn report_json(report: &FarmReport) -> Json {
    json!({
        "ok": true,
        "out": report.out.display().to_string(),
        "parts": report
            .parts
            .iter()
            .map(|part| json!({
                "index": part.index,
                "from": part.from,
                "to": part.to,
                "frames": part.frames,
                "rendered": part.rendered,
            }))
            .collect::<Vec<Json>>(),
        "audio": report.audio.as_ref().map(|part| json!({
            "rendered": part.rendered,
        })),
        "rendered": report.rendered,
        "served": report.served,
        "seconds": report.seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::{self, Sequence};
    use serde_json::Value as Json;

    /// A stub engine for the farm: it answers `plan` with a fixed split,
    /// writes a marker's worth of bytes for a `render`, and records every
    /// call. The same shape as the reel cache's stub - see the tests
    /// there for the pattern's reasoning.
    #[cfg(unix)]
    mod with_a_stub {
        use super::*;
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
                    "cmd=\"\"; out=\"\"; prev=\"\"\n",
                    "for arg in \"$@\"; do\n",
                    "  case \"$arg\" in plan|render|join) cmd=\"$arg\" ;; esac\n",
                    "  if [ \"$prev\" = \"-o\" ] || [ \"$prev\" = \"--output\" ]; then out=\"$arg\"; fi\n",
                    "  prev=\"$arg\"\n",
                    "done\n",
                    "echo \"$cmd $*\" >> \"$(dirname \"$0\")/calls.log\"\n",
                    "case \"$cmd\" in\n",
                    "  plan)\n",
                    "    cat <<'JSON'\n",
                    "{\"ok\":true,\"parts\":[{\"frames\":[0,30],\"output\":\"a.mp4\"},{\"frames\":[30,60],\"output\":\"b.mp4\"}],\"audio\":{\"output\":\"c.mp4\"}}\n",
                    "JSON\n",
                    "    exit 0 ;;\n",
                    "  render)\n",
                    "    case \"$*\" in *--audio-only*) printf 'audio-part' > \"$out\";; *) printf 'picture-part' > \"$out\";; esac\n",
                    "    exit 0 ;;\n",
                    "  join)\n",
                    "    printf 'joined' > \"$out\" ;;\n",
                    "esac\n",
                    "echo '{\"ok\":true,\"diagnostics\":[],\"summary\":{}}'\n",
                ),
            )
            .expect("the stub is written");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("the stub is executable");
            Geneva::at(&path)
        }

        fn plan(geneva: &Geneva) -> Plan {
            let mut reel = Sequence::new(320, 180, 30.0);
            let comp = crate::presets::build("funcut.sting.wordmark").expect("a preset");
            let mut scene = sequence::Scene::new(comp);
            scene.comp.duration = 3.0;
            reel.push(scene);
            sequence::plan(geneva, &reel).expect("a plan")
        }

        /// A first run renders every part and marks each; a second run
        /// serves every part and joins again - the parts survived.
        #[test]
        fn a_failed_run_costs_only_its_missing_parts() {
            let _serial = serial();
            let dir = TempDir::new().unwrap();
            let geneva = stub_geneva(&dir);
            let plan = plan(&geneva);
            let render_dir = dir.path().join("render");
            let farm_dir = dir.path().join("farm");
            let out = dir.path().join("reel.mp4");

            let first = render(&geneva, &plan, &render_dir, &farm_dir, &out, 2).expect("farms");
            assert_eq!(first.parts.len(), 2);
            assert!(
                first.parts.iter().all(|part| part.rendered),
                "both picture parts draw: {:?}",
                first.parts
            );
            assert!(first.audio.is_some(), "the plan named an audio part");
            assert_eq!(first.rendered, 3, "the audio part draws too");
            let calls: String = std::fs::read_to_string(dir.path().join("calls.log")).unwrap();
            assert!(
                calls.contains("--frames 0..30"),
                "the first part is a range"
            );
            assert!(calls.contains("--frames 30..60"));
            assert!(calls.contains("--audio-only"));
            assert!(calls.contains(" join "), "the join runs");

            let second = render(&geneva, &plan, &render_dir, &farm_dir, &out, 2).expect("farms");
            assert_eq!(second.served, 3, "every part was served, nothing drawn");
            assert_eq!(second.rendered, 0);
            assert_eq!(
                std::fs::read(&out).unwrap(),
                b"joined",
                "the join runs again on the served parts"
            );

            // A worker that died after the first part: the second part's
            // marker never arrived, so only the second part re-renders.
            let _ = std::fs::remove_file(farm_dir.join("part-001.done"));
            let third = render(&geneva, &plan, &render_dir, &farm_dir, &out, 2).expect("farms");
            assert_eq!(third.rendered, 1, "only the lost part is redrawn");
            assert_eq!(third.served, 2);
        }

        /// A changed plan is a changed timeline, so every part re-renders:
        /// the farm never serves a part baked from another document.
        #[test]
        fn a_changed_plan_is_a_changed_farm() {
            let _serial = serial();
            let dir = TempDir::new().unwrap();
            let geneva = stub_geneva(&dir);
            let first_plan = plan(&geneva);
            let render_dir = dir.path().join("render");
            let farm_dir = dir.path().join("farm");
            let out = dir.path().join("reel.mp4");
            render(&geneva, &first_plan, &render_dir, &farm_dir, &out, 2).expect("farms");

            let mut changed = plan(&geneva);
            changed.document["output"]["encode"]["video"]["crf"] = Json::from(28u32);
            let rerun = render(&geneva, &changed, &render_dir, &farm_dir, &out, 2).expect("farms");
            assert_eq!(rerun.rendered, 3, "a new timeline re-renders every part");
        }
    }
}
