//! The AUTO EDIT program module.
//! The AUTO EDIT button's whole program: every stage the unattended run
//! can run without a creative brain, written here - beside the adapter
//! that drives it - so the window's pane asks for it and the tests that
//! hold it to its word run where the runner does.

use std::path::PathBuf;
use std::time::Duration;

use concat_director::{
    DerivedStep, EditProgram, FanStep, ProgramStep, ReplyGate, Stage, StageStep, ToolCall,
};
use serde_json::{Value, json};

/// One voiceover file's fixed paths inside the sibling, as the AUTO
/// EDIT's program is written from them: the take the timeline plays, and
/// the enhanced stepping stone the repair writes before the clean.
#[derive(Clone)]
pub struct TakePaths {
    /// The file the sibling's voiceover clips play: the pristine copy
    /// until the repair's clean pass overwrites it with the enhanced,
    /// cleaned chain.
    pub take: String,
    /// The enhanced intermediate: the repair's first pass writes it, its
    /// second reads it. A repair that fails leaves the take as recorded.
    pub enhanced: String,
}

/// One placement of a take on the sibling timeline: the file the clip
/// plays, where on the timeline it starts, the span of the file it plays
/// and the rate it plays at - the verify's expected side, as the run was
/// planned, for when nothing moved it.
#[derive(Clone)]
pub struct TakePlacement {
    /// The sibling file the clip plays.
    pub take: String,
    /// Where the clip starts on the timeline.
    pub start: f64,
    /// Where the clip begins in the file, in file seconds.
    pub source_in: f64,
    /// Where it ends in the file, in file seconds.
    pub source_out: f64,
    /// Playback rate, 1 being unity.
    pub speed: f64,
}

/// What an AUTO EDIT run needs, as its program is written from it: the
/// fixed paths, all inside the sibling it edits.
#[derive(Clone)]
pub struct AutoEditProgramSource {
    /// The sibling project the run edits.
    pub project: String,
    /// One row per distinct voiceover file, in first-play order.
    pub takes: Vec<TakePaths>,
    /// One row per voiceover clip, in first-play order: the placement
    /// the verify's expected transcript falls back to (F-04).
    pub placements: Vec<TakePlacement>,
    /// Where the export lands, inside the sibling's own export folder.
    pub export: String,
    /// The contact sheet's name, relative to the project: the window's
    /// server refuses absolute write paths, and the sheet is written
    /// through it.
    pub sheet: String,
}

/// The last timeline view the run read: the newest reply shaped like one,
/// which after the tighten and the placements is the truth the captions
/// and the verify map their file times onto.
fn latest_view(log: &[Value]) -> Option<&Value> {
    log.iter()
        .rev()
        .find(|reply| reply.pointer("/timeline/clips").is_some())
}

/// The bin's media rows, id to path, from a timeline view.
fn media_paths(view: &Value) -> Vec<(String, String)> {
    view["media"]
        .as_array()
        .map(|media| {
            media
                .iter()
                .filter_map(|item| {
                    Some((
                        item["id"].as_str()?.to_owned(),
                        item["path"].as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A clip's file span and rate: where it begins in its file, where it
/// ends, and how fast it plays what it shows.
fn clip_file_span(clip: &Value) -> (f64, f64, f64) {
    let source_start = clip["sourceStart"].as_f64().unwrap_or(0.0);
    let duration = clip["duration"].as_f64().unwrap_or(0.0);
    let speed = clip["speed"].as_f64().unwrap_or(1.0).max(1e-6);
    (source_start, source_start + duration * speed, speed)
}

/// Where a file-second sits on the timeline, through the clip pieces the
/// current view carries: the piece whose file span holds the moment maps
/// it onto its own timeline span. A moment no piece plays (cut away, or
/// never placed) answers None.
fn timeline_at(view: &Value, media: &[(String, String)], take: &str, moment: f64) -> Option<f64> {
    for clip in view["timeline"]["clips"].as_array()? {
        let media_id = clip["media"].as_str()?;
        let path = media
            .iter()
            .find(|(id, _)| id == media_id)
            .map(|(_, path)| path.as_str())?;
        if path != take {
            continue;
        }
        let (source_in, source_out, speed) = clip_file_span(clip);
        if (source_in..source_out).contains(&moment) {
            let start = clip["start"].as_f64()?;
            return Some(start + (moment - source_in) / speed);
        }
    }
    None
}

/// The dead-air cuts the measured pauses ask for, as one `edit_apply`
/// per clip: only the `dead_air` pauses the voiceSignals stage classified
/// and marked not worth keeping, each shrunk 80 ms a side so the breath
/// survives at the sentence end (the skill's own padding rule, applied),
/// ripple on - the lane closes behind the cut. The spans are file
/// seconds mapped through the clip that plays them.
fn dead_air_cuts(log: &[Value], takes: &[TakePaths], project: &str) -> Vec<ToolCall> {
    let Some(view) = log
        .iter()
        .find(|reply| reply.pointer("/timeline/clips").is_some())
    else {
        return Vec::new();
    };
    let media = media_paths(view);
    // The pauses each take's reading measured, keyed by the take file.
    let mut pauses: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
    for reply in log {
        let Some(path) = reply.pointer("/audio/path").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(rows) = reply["pauses"].as_array() else {
            continue;
        };
        let spans: Vec<(f64, f64)> = rows
            .iter()
            .filter(|pause| pause["class"] == json!("dead_air") && pause["keep"] != json!(true))
            .filter_map(|pause| {
                let start = pause["start"].as_f64()?;
                let end = pause["end"].as_f64()?;
                // Only spans worth cutting, and only after the padding
                // rule leaves something to cut.
                if end - start < 0.5 {
                    return None;
                }
                let shrunk = (start + 0.08, end - 0.08);
                (shrunk.1 > shrunk.0).then_some(shrunk)
            })
            .collect();
        pauses.push((path.to_owned(), spans));
    }
    let mut cuts = Vec::new();
    for clip in view["timeline"]["clips"].as_array().into_iter().flatten() {
        let Some(media_id) = clip["media"].as_str() else {
            continue;
        };
        let Some(path) = media
            .iter()
            .find(|(id, _)| id == media_id)
            .map(|(_, path)| path.as_str())
        else {
            continue;
        };
        if !takes.iter().any(|take| take.take == path) {
            continue;
        }
        let (source_in, source_out, speed) = clip_file_span(clip);
        let clip_id = clip["id"].as_str().unwrap_or_default().to_owned();
        let mut spans: Vec<Value> = Vec::new();
        for (take, rows) in &pauses {
            if take != path {
                continue;
            }
            for (start, end) in rows {
                // The pause clipped to the span this clip plays, in this
                // clip's own seconds.
                let from = ((start - source_in) / speed).max(0.0);
                let to = ((end - source_in) / speed).min((source_out - source_in) / speed);
                if to > from + 0.05 {
                    spans.push(json!([round3(from), round3(to)]));
                }
            }
        }
        if spans.is_empty() {
            continue;
        }
        cuts.push(ToolCall {
            tool: "edit_apply".to_owned(),
            arguments: json!({
                "path": project,
                "command": {
                    "op": "removeSpans",
                    "clipId": clip_id,
                    "spans": spans,
                    "ripple": true,
                },
            }),
        });
    }
    cuts
}

/// Three decimals, the way the timeline's own numbers read.
fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The transcript each take's wait left behind: the take file and the
/// segments its words timed. A wait that never landed is not here.
fn transcripts_of(log: &[Value], takes: &[TakePaths]) -> Vec<(String, Vec<Value>)> {
    let mut found = Vec::new();
    for reply in log {
        let Some(path) = reply["path"].as_str() else {
            continue;
        };
        if !takes.iter().any(|take| take.take == path) {
            continue;
        }
        let Some(output) = reply["output"].as_str() else {
            continue;
        };
        let Ok(transcript) = serde_json::from_str::<Value>(output) else {
            continue;
        };
        let Some(segments) = transcript["segments"].as_array() else {
            continue;
        };
        found.push((path.to_owned(), segments.clone()));
    }
    found
}

/// The AUTO EDIT's whole program: every stage the unattended run can run
/// without a creative brain, each standing on what the run itself
/// measured. The static spine repairs, transcribes, reads the voice and
/// gates the music; the fans run the takes' work at the same time; the
/// derived stages act on the replies - the dead air cut, the captions
/// placed on the measured words, the music bed shopped and ducked, the
/// accents landed on the emphasis peaks - and the verify reads the
/// timeline as it ended up, not as it was planned. The creative layer -
/// B-roll, the grade, motion design - stays the Director agent's.
pub fn auto_edit_program(source: &AutoEditProgramSource) -> EditProgram {
    let project = source.project.clone();
    let export = source.export.clone();
    let takes = source.takes.clone();
    let call = |tool: &str, arguments: Value| ToolCall {
        tool: tool.to_owned(),
        arguments,
    };
    let mut steps: Vec<ProgramStep> = Vec::new();
    // 1. The handoff, read back: the sibling is opened on the window's
    //    server - the same register a forge run's edit sits in - and the
    //    voiceover lane is read as the run's own ground truth.
    steps.push(
        StageStep::run(
            Stage::HandoffVoiceover,
            vec![
                call("project_open", json!({ "path": project.clone() })),
                call("timeline_read", json!({ "path": project.clone() })),
            ],
        )
        .into(),
    );
    // 2. The repair, per distinct take, on its own lane: the enhanced
    //    chain lands on the take the timeline plays, so the export ships
    //    the sound the repair made. One file's failure warns and skips
    //    the rest - the take ships as recorded.
    let repair: Vec<Vec<ToolCall>> = takes
        .iter()
        .map(|take| {
            vec![
                call(
                    "sense_audio_repair",
                    json!({
                        "action": "enhance_speech",
                        "audio": take.take,
                        "out": take.enhanced,
                        "overwrite": true,
                    }),
                ),
                call(
                    "sense_audio_repair",
                    json!({
                        "action": "remove_hiss_hum_rumble",
                        "audio": take.enhanced,
                        "out": take.take,
                        "overwrite": true,
                    }),
                ),
            ]
        })
        .collect();
    steps.push(FanStep::fan(Stage::AudioRepair, repair).warn().into());
    // 3. The words: every take transcribed at the same time, one lane
    //    each - the transcriptions of a five-take voiceover land in the
    //    time of the slowest, not the sum. Two waits per lane cover the
    //    transcriber's own 900-second budget.
    let words: Vec<Vec<ToolCall>> = takes
        .iter()
        .map(|take| {
            vec![
                call("transcribe", json!({ "path": take.take })),
                call(
                    "analysis_wait",
                    json!({ "path": take.take, "timeoutSeconds": 590 }),
                ),
                call(
                    "analysis_wait",
                    json!({ "path": take.take, "timeoutSeconds": 590 }),
                ),
            ]
        })
        .collect();
    steps.push(FanStep::fan(Stage::Transcribe, words).into());
    // 4. The delivery: pauses, emphasis, the read itself - one lane per
    //    take, each reply carrying its own audio path so the fan's
    //    landing order is no one's guess.
    let signals: Vec<Vec<ToolCall>> = takes
        .iter()
        .map(|take| vec![call("voice_signals", json!({ "audio": take.take }))])
        .collect();
    steps.push(FanStep::fan(Stage::VoiceSignals, signals).into());
    // 5. The music gate: a take that already carries music gets no bed
    //    stacked under it - the record the gate leaves behind, which the
    //    music stage reads before it shops.
    let gate: Vec<ToolCall> = takes
        .iter()
        .map(|take| {
            call(
                "sense_audio_repair",
                json!({ "action": "detect_music_segments", "audio": take.take }),
            )
        })
        .collect();
    steps.push(StageStep::run(Stage::Understand, gate).into());
    // 6. The tighten: only the dead air the reading classified, each span
    //    shrunk the padding rule's 80 ms, cut through `edit_apply` so the
    //    rollback and the resume both see it - then the timeline read
    //    back as it now stands, the truth every later stage maps through.
    //    No dead air is a tighten that succeeded.
    let tighten_source = takes.clone();
    let tighten_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Timeline,
            Box::new({
                let mut queue: Option<Vec<ToolCall>> = None;
                move |log| {
                    let queue = queue.get_or_insert_with(|| {
                        let mut calls = dead_air_cuts(log, &tighten_source, &tighten_project);
                        calls.push(call("timeline_read", json!({ "path": tighten_project })));
                        calls
                    });
                    if queue.is_empty() {
                        None
                    } else {
                        Some(queue.remove(0))
                    }
                }
            }),
        )
        .warn()
        .into(),
    );
    // 7. The captions: the transcript's own measured words, placed where
    //    the timeline now plays them - through the pieces the tighten
    //    left, so a cut that moved a word moves its caption too - as one
    //    undoable batch of titles in the reading's house style.
    let caption_source = takes.clone();
    let caption_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Captions,
            Box::new({
                let mut batch: Option<ToolCall> = None;
                move |log| {
                    if batch.is_some() {
                        return batch.take();
                    }
                    let view = latest_view(log)?;
                    let media = media_paths(view);
                    let mut captions: Vec<Value> = Vec::new();
                    for (take, segments) in transcripts_of(log, &caption_source) {
                        for segment in segments {
                            let words = segment["words"].as_array();
                            let start = words
                                .and_then(|words| words.first())
                                .and_then(|word| word["start"].as_f64())
                                .or_else(|| segment["start"].as_f64());
                            let end = words
                                .and_then(|words| words.last())
                                .and_then(|word| word["end"].as_f64())
                                .or_else(|| segment["end"].as_f64());
                            let text = segment["text"].as_str().unwrap_or_default().trim();
                            let (Some(start), Some(end)) = (start, end) else {
                                continue;
                            };
                            if text.is_empty() || end - start < 0.3 {
                                continue;
                            }
                            let (Some(at), Some(to)) = (
                                timeline_at(view, &media, &take, start),
                                timeline_at(view, &media, &take, end),
                            ) else {
                                continue;
                            };
                            if to - at < 0.3 {
                                continue;
                            }
                            captions.push(json!({
                                "start": round3(at),
                                "end": round3(to),
                                "text": text,
                            }));
                        }
                    }
                    if captions.is_empty() {
                        return None;
                    }
                    // Overlaps cannot happen on a clean transcript, but a
                    // merged one can carry them: the later caption waits
                    // for the one before it to leave.
                    let mut kept: Vec<Value> = Vec::new();
                    let mut until = 0.0;
                    for caption in captions {
                        if caption["start"].as_f64().unwrap_or(0.0) < until - 0.05 {
                            continue;
                        }
                        until = caption["end"].as_f64().unwrap_or(0.0);
                        kept.push(caption);
                    }
                    kept.truncate(240);
                    let commands: Vec<Value> = kept
                        .iter()
                        .map(|caption| {
                            json!({
                                "op": "addTextClip",
                                "trackId": null,
                                "above": true,
                                "start": caption["start"],
                                "duration": round3(
                                    caption["end"].as_f64().unwrap_or(0.0)
                                        - caption["start"].as_f64().unwrap_or(0.0)
                                ),
                                "style": {
                                    "fontFamily": "Hanken Grotesk",
                                    "fontSize": 0.04,
                                    "fontWeight": 600,
                                    "content": caption["text"],
                                },
                                "offsetY": 0.35,
                            })
                        })
                        .collect();
                    batch = Some(ToolCall {
                        tool: "edit_apply".to_owned(),
                        arguments: json!({
                            "path": caption_project,
                            "command": { "op": "batch", "commands": commands },
                        }),
                    });
                    batch.take()
                }
            }),
        )
        .warn()
        .into(),
    );
    // 8. The music bed, only where the gate said the take carries none:
    //    the shelf prepared and polled, the track shopped (the reading's
    //    own genre hint for the query when it named one), the bed
    //    imported at zero, the timeline read back, and the bed ducked
    //    under the voice. The CC0 shelf only - an unattended run never
    //    ships the non-commercial one.
    let music_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Music,
            Box::new({
                let mut polls = 0u32;
                move |log| {
                    // The gate's own answer first: music already in the
                    // take means no bed, ever.
                    if log
                        .iter()
                        .any(|reply| reply.get("musicPresent") == Some(&json!(true)))
                    {
                        return None;
                    }
                    let Some(last) = log.last() else {
                        return Some(call("bgm_status", json!({})));
                    };
                    if last.get("freepd").is_some() {
                        // A bgm_status reply: prepare the missing shelf,
                        // poll the loading one, shop the ready one.
                        return match last["freepd"].as_str() {
                            Some("ready") => {
                                let hint = log
                                    .iter()
                                    .filter_map(|reply| reply.pointer("/language/musicGenreHint"))
                                    .filter_map(|hint| hint.as_str())
                                    .find(|hint| !hint.trim().is_empty());
                                let mut arguments = json!({
                                    "source": "freepd",
                                    "limit": 5,
                                    "sort": "relevance",
                                });
                                if let Some(hint) = hint {
                                    arguments["query"] = json!(hint);
                                }
                                Some(call("bgm_search", arguments))
                            }
                            Some("missing") => {
                                polls += 1;
                                Some(call("bgm_prepare", json!({ "source": "freepd" })))
                            }
                            Some("loading") => {
                                polls += 1;
                                if polls > 200 {
                                    return None;
                                }
                                // The shelf's one-time fetch is real work:
                                // the poll waits between looks.
                                std::thread::sleep(Duration::from_millis(3000));
                                Some(call("bgm_status", json!({})))
                            }
                            _ => None,
                        };
                    }
                    if last.get("tracks").is_some() {
                        // A bgm_search reply: the first track is the bed.
                        let id = last
                            .pointer("/tracks/0/id")
                            .and_then(|id| id.as_str())
                            .map(str::to_owned);
                        return id.map(|id| {
                            call(
                                "bgm_import",
                                json!({
                                    "ids": [id],
                                    "start": 0.0,
                                    "monetized": true,
                                }),
                            )
                        });
                    }
                    if last.pointer("/timeline/clips").is_some() {
                        // A timeline read after the import: the bed is the
                        // clip whose media landed in the project's bgm
                        // folder. Duck it under the voice and be done.
                        let media = media_paths(last);
                        let bed = last["timeline"]["clips"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|clip| {
                                clip["media"]
                                    .as_str()
                                    .and_then(|id| media.iter().find(|(mid, _)| mid == id))
                                    .map(|(_, path)| path.contains("/bgm/"))
                                    .unwrap_or(false)
                            });
                        return bed.map(|bed| {
                            call(
                                "clip_audio_levels_db",
                                json!({
                                    "path": music_project,
                                    "clipId": bed["id"],
                                    "baseDb": -20.0,
                                    "replaceAutomation": true,
                                }),
                            )
                        });
                    }
                    if last.get("state").is_some() && last.get("path").is_none() {
                        // The import's own reply: read the timeline back
                        // for the bed's clip id.
                        return Some(call("timeline_read", json!({ "path": music_project })));
                    }
                    None
                }
            }),
        )
        .warn()
        .into(),
    );
    // 9. The accents: the reading's own emphasis peaks, the strongest
    //    six, each with a library sound landed on the moment the peak
    //    measured - shopped one at a time so the search's answer picks
    //    the sound, not the program's guess.
    let sfx_source = takes.clone();
    let sfx_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Sfx,
            Box::new({
                let mut peaks: Option<Vec<(f64, f64)>> = None;
                let mut placed = 0usize;
                move |log| {
                    let peaks = peaks.get_or_insert_with(|| {
                        let mut found: Vec<(f64, f64)> = Vec::new();
                        for reply in log {
                            let Some(path) = reply.pointer("/audio/path").and_then(|v| v.as_str())
                            else {
                                continue;
                            };
                            if !sfx_source.iter().any(|take| take.take == path) {
                                continue;
                            }
                            for line in reply["lines"].as_array().into_iter().flatten() {
                                for word in line
                                    .pointer("/prosody/emphasisWords")
                                    .and_then(|w| w.as_array())
                                    .into_iter()
                                    .flatten()
                                {
                                    let (Some(time), Some(strength)) =
                                        (word["t"].as_f64(), word["strength"].as_f64())
                                    else {
                                        continue;
                                    };
                                    found.push((time, strength));
                                }
                            }
                        }
                        // The strongest six, earliest first: the run's
                        // accent budget, spent where the read peaked.
                        found.sort_by(|left, right| right.1.total_cmp(&left.1));
                        found.truncate(6);
                        found.sort_by(|left, right| left.0.total_cmp(&right.0));
                        found
                    });
                    if placed >= peaks.len() {
                        return None;
                    }
                    let view = latest_view(log)?;
                    let media = media_paths(view);
                    let take = sfx_source
                        .iter()
                        .map(|take| take.take.clone())
                        .find(|take| timeline_at(view, &media, take, peaks[placed].0).is_some());
                    let Some(take) = take else {
                        placed += 1;
                        return None;
                    };
                    let Some(at) = timeline_at(view, &media, &take, peaks[placed].0) else {
                        placed += 1;
                        return None;
                    };
                    // What the last call was decides what this one is: the
                    // search hands the id the add takes.
                    let last_was_search = log
                        .last()
                        .and_then(|reply| reply.get("sounds").map(|_| ()))
                        .is_some();
                    if last_was_search {
                        let id = log
                            .last()
                            .and_then(|reply| reply.pointer("/sounds/0/id"))
                            .and_then(|id| id.as_str())
                            .map(str::to_owned);
                        placed += 1;
                        return id.map(|id| {
                            call(
                                "sounds_add",
                                json!({ "path": sfx_project, "id": id, "start": round3(at) }),
                            )
                        });
                    }
                    const QUERIES: [&str; 6] =
                        ["impact", "whoosh", "riser", "boom", "swish", "hit"];
                    Some(call(
                        "sounds_search",
                        json!({
                            "query": QUERIES[placed % QUERIES.len()],
                            "limit": 3,
                        }),
                    ))
                }
            }),
        )
        .warn()
        .into(),
    );
    // 10. The render, waited on by its project folder: the job's name is
    //     minted when the export starts, and a program written before that
    //     moment asks for the work by what it was started on.
    steps.push(
        StageStep::run(
            Stage::Export,
            vec![
                call(
                    "export_start",
                    json!({
                        "path": project.clone(),
                        "output": export.clone(),
                        "crf": 20,
                        "codec": "h264"
                    }),
                ),
                call(
                    "export_wait",
                    json!({ "path": project.clone(), "timeoutSeconds": 590 }),
                ),
                call(
                    "export_wait",
                    json!({ "path": project, "timeoutSeconds": 590 }),
                ),
            ],
        )
        .into(),
    );
    // 11. The verify: the sanity reads first - collisions, and the
    //     timeline as the placements left it - then the words re-read
    //     from the render, the loudness measured against the short-form
    //     target, and the re-read words COMPARED with the words the edit
    //     places now, each piece of the voiceover a take of its own
    //     (F-04). The expected side is derived from the timeline the run
    //     ended with, not the one it planned: a tighten that moved a word
    //     is verified where the word is. This reply is the verification
    //     the run's terminal states carry, and the gates are the
    //     measurements' own word.
    let verify_source = takes.clone();
    let verify_export = source.export.clone();
    let verify_placements = source.placements.clone();
    let verify_project_path = source.project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Verify,
            Box::new({
                let mut phase = 0u8;
                move |log| {
                    if phase == 0 {
                        phase = 1;
                        return Some(call(
                            "collision_check",
                            json!({ "path": verify_project_path }),
                        ));
                    }
                    if phase == 1 {
                        phase = 2;
                        return Some(call(
                            "timeline_read",
                            json!({ "path": verify_project_path }),
                        ));
                    }
                    if phase == 2 {
                        phase = 3;
                        return Some(call("transcribe", json!({ "path": verify_export })));
                    }
                    if phase == 3 {
                        phase = 4;
                        return Some(call(
                            "analysis_wait",
                            json!({ "path": verify_export, "timeoutSeconds": 590 }),
                        ));
                    }
                    if phase == 4 {
                        phase = 5;
                        return Some(call(
                            "analysis_wait",
                            json!({ "path": verify_export, "timeoutSeconds": 590 }),
                        ));
                    }
                    if phase == 5 {
                        phase = 6;
                        return Some(call("measure_loudness", json!({ "path": verify_export })));
                    }
                    if phase == 6 {
                        phase = 7;
                        // The expected words: the timeline's own voiceover
                        // pieces, each mapped as the take it plays - the
                        // placements the run began with when nothing
                        // moved them, the pieces the tighten left when it
                        // did.
                        let expected: Vec<Value> =
                            match latest_view(log).map(|view| {
                                let media = media_paths(view);
                                view["timeline"]["clips"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter(|clip| {
                                        clip["media"].as_str().is_some_and(|id| {
                                            media.iter().any(|(mid, path)| {
                                                mid == id
                                                    && verify_source
                                                        .iter()
                                                        .any(|take| take.take == *path)
                                            })
                                        })
                                    })
                                    .map(|clip| {
                                        let (source_in, source_out, speed) = clip_file_span(clip);
                                        json!({
                                            "path": media
                                                .iter()
                                                .find(|(mid, _)| Some(mid.as_str()) == clip["media"].as_str())
                                                .map(|(_, path)| path.clone())
                                                .unwrap_or_default(),
                                            "start": clip["start"].as_f64().unwrap_or(0.0),
                                            "sourceIn": source_in,
                                            "sourceOut": source_out,
                                            "speed": speed,
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            }) {
                                Some(rows) if !rows.is_empty() => rows,
                                // No view the closure can read: the
                                // placements the run was written from are
                                // the expected side, as they always were.
                                _ => verify_placements
                                    .iter()
                                    .map(|placed| {
                                        json!({
                                            "path": placed.take,
                                            "start": placed.start,
                                            "sourceIn": placed.source_in,
                                            "sourceOut": placed.source_out,
                                            "speed": placed.speed,
                                        })
                                    })
                                    .collect(),
                            };
                        return Some(call(
                            "emotion",
                            json!({
                                "action": "verify",
                                "expectedTakes": expected,
                                "actualPath": verify_export,
                                "toleranceMs": 60,
                            }),
                        ));
                    }
                    None
                }
            }),
        )
        .gate(ReplyGate::True {
            field: "platformTargetMet",
            what: "the mix missed the platform loudness target",
        })
        .gate(ReplyGate::Word {
            field: "verdict",
            equals: "pass",
            what: "the export's words are not the words the edit placed",
        })
        .into(),
    );
    // 12. The watch, warn-policy: the whole cut as one contact sheet,
    //     written beside the export, then read and scored. A score under
    //     the bar is a flaw the edit ships with - recorded, yellow - not a
    //     run thrown away after the render already landed. The gate is
    //     the craft bar the headless runner ships on (7.0).
    steps.push(
        StageStep::run(
            Stage::Watch,
            vec![
                call(
                    "watch_render",
                    json!({
                        "path": source.export,
                        "project": source.project,
                        "out": source.sheet,
                        "audio": true,
                    }),
                ),
                call(
                    "craft_critique",
                    json!({
                        "pngPath": std::path::Path::new(&source.project)
                            .join(&source.sheet)
                            .display()
                            .to_string(),
                        "project": source.project,
                    }),
                ),
            ],
        )
        .warn()
        .gate(ReplyGate::AtLeast {
            field: "overall",
            minimum: 7.0,
            what: "the cut scored under the craft bar",
        })
        .into(),
    );
    EditProgram::mixed(PathBuf::from(&source.export), steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The program the button runs: the whole unattended spine in the
    /// runner's register - the static handoff, the fanned repair and
    /// transcription (every take at the same time), the derived tighten,
    /// captions, music bed and accents (each standing on what the run
    /// measured), the export, the verify that reads the timeline as it
    /// ended up, and the watch. The B-roll, grade and motion design stay
    /// the Director agent's: those stages' registers are simply absent.
    #[test]
    fn the_auto_edit_program_is_the_watchers_spine_over_fixed_paths() {
        use concat_director::{ErrorPolicy, FanStep, ProgramStep};

        let source = AutoEditProgramSource {
            project: "/edits/Reel edit".to_owned(),
            takes: vec![
                TakePaths {
                    take: "/edits/Reel edit/voiceover/take-000-a.wav".to_owned(),
                    enhanced: "/edits/Reel edit/voiceover/enhanced-000-a.wav".to_owned(),
                },
                TakePaths {
                    take: "/edits/Reel edit/voiceover/take-001-b.wav".to_owned(),
                    enhanced: "/edits/Reel edit/voiceover/enhanced-001-b.wav".to_owned(),
                },
            ],
            placements: vec![
                TakePlacement {
                    take: "/edits/Reel edit/voiceover/take-000-a.wav".to_owned(),
                    start: 0.0,
                    source_in: 0.0,
                    source_out: 2.0,
                    speed: 1.0,
                },
                TakePlacement {
                    take: "/edits/Reel edit/voiceover/take-001-b.wav".to_owned(),
                    start: 3.5,
                    source_in: 0.5,
                    source_out: 3.38,
                    speed: 1.2,
                },
            ],
            export: "/edits/Reel edit/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
        };
        let program = super::auto_edit_program(&source);

        // The register, in the run's own order: the fanned repair and
        // transcription, the derived tighten/captions/music/sfx, and the
        // static export/verify-gates/watch. The creative layer's stages
        // (Plan, Gather, Motion, Preview, Revise) are absent - the
        // fraction the page shows counts only these.
        let order: Vec<&str> = program
            .steps
            .iter()
            .map(|step| step.stage().label())
            .collect();
        assert_eq!(
            order,
            vec![
                "handoff-voiceover",
                "audio-repair",
                "transcribe",
                "voice-signals",
                "understand",
                "timeline",
                "captions",
                "music",
                "sfx",
                "export",
                "verify",
                "watch",
            ]
        );
        // The warn-policy stages, and only them: a take that cannot be
        // repaired ships as recorded, a tighten or caption or bed or
        // accent that cannot land is a flaw the edit ships with, and a
        // low craft score is recorded yellow, not thrown away.
        let warn: Vec<&str> = program
            .steps
            .iter()
            .filter(|step| step.policy() == ErrorPolicy::Warn)
            .map(|step| step.stage().label())
            .collect();
        assert_eq!(
            warn,
            vec![
                "audio-repair",
                "timeline",
                "captions",
                "music",
                "sfx",
                "watch"
            ]
        );

        // The export is promised where the program says it lands.
        assert_eq!(
            program.export,
            std::path::Path::new("/edits/Reel edit/export/auto-edit.mp4")
        );

        // The repair is fanned, one lane per take, and its chain lands on
        // the take the timeline plays: the export ships the repaired
        // sound, the watcher's own promise.
        let ProgramStep::Fanned(repair) = &program.steps[1] else {
            panic!("the repair is fanned");
        };
        assert_eq!(repair.groups.len(), 2, "one lane per take");
        assert_eq!(repair.groups[0][0].tool, "sense_audio_repair");
        assert_eq!(repair.groups[0][0].arguments["action"], "enhance_speech");
        assert_eq!(
            repair.groups[0][0].arguments["out"],
            source.takes[0].enhanced.as_str()
        );
        assert_eq!(
            repair.groups[0][1].arguments["action"],
            "remove_hiss_hum_rumble"
        );
        assert_eq!(
            repair.groups[0][1].arguments["audio"],
            source.takes[0].enhanced.as_str()
        );
        assert_eq!(
            repair.groups[0][1].arguments["out"],
            source.takes[0].take.as_str()
        );

        // The transcription is fanned too: both takes transcribe at the
        // same time, each lane waiting on its own file.
        let ProgramStep::Fanned(words) = &program.steps[2] else {
            panic!("the transcription is fanned");
        };
        assert_eq!(words.groups.len(), 2);
        assert_eq!(words.groups[0].len(), 3, "transcribe and two waits");
        assert_eq!(
            words.groups[1][0].arguments["path"],
            source.takes[1].take.as_str()
        );

        // The tighten, the captions, the bed and the accents are derived:
        // their calls are the run's own replies to answer for, and the
        // runner's reactive queue asks for them one at a time.
        for at in [5usize, 6, 7, 8] {
            assert!(
                matches!(program.steps[at], ProgramStep::Derived(_)),
                "step {at} is derived"
            );
        }

        // The export is waited on by its project folder - the job's name
        // is minted at run time - and the watch's sheet is a project-
        // relative name, because the window's server refuses absolute
        // write paths.
        let export = program.steps[9].as_static();
        assert_eq!(export.calls[0].tool, "export_start");
        assert_eq!(export.calls[0].arguments["output"], source.export.as_str());
        assert_eq!(export.calls[1].tool, "export_wait");
        assert_eq!(export.calls[1].arguments["path"], source.project.as_str());
        let watch = program.steps[11].as_static();
        assert_eq!(watch.calls[0].arguments["out"], "auto-edit-sheet.png");
        assert_eq!(
            watch.calls[1].arguments["pngPath"],
            "/edits/Reel edit/auto-edit-sheet.png"
        );

        // The verify's gates: the loudness target, and the words (F-04) -
        // on the derived step that reads the timeline as it ended up.
        use concat_director::ReplyGate;
        assert!(matches!(program.steps[10], ProgramStep::Derived(_)));
        assert_eq!(
            program.steps[10].gates(),
            vec![
                ReplyGate::True {
                    field: "platformTargetMet",
                    what: "the mix missed the platform loudness target",
                },
                ReplyGate::Word {
                    field: "verdict",
                    equals: "pass",
                    what: "the export's words are not the words the edit placed",
                },
            ]
        );
        assert_eq!(
            program.steps[11].gates(),
            vec![ReplyGate::AtLeast {
                field: "overall",
                minimum: 7.0,
                what: "the cut scored under the craft bar",
            }]
        );
        // No other step gates its replies.
        for (at, step) in program.steps.iter().enumerate() {
            if at != 10 && at != 11 {
                assert!(step.gates().is_empty(), "step {at} gates its replies");
            }
        }
        // The fan compile-time proof the type system wanted: the repair
        // grouping is the FanStep the runner fans out.
        let _: &FanStep = repair;
    }
}
