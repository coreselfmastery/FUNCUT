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
    /// The configured live stock shelf for B-roll. AUTO EDIT never imports
    /// OpenVid's research-only dataset because it has no per-clip licence.
    pub stock_provider: String,
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
        // A clip with no media - a title or an effect layer - plays no
        // file moment, so it is stepped over, not the end of the scan:
        // reading it with `?` would answer None for the whole view the
        // moment any caption the run placed sorts first.
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

/// The timeline moment the accents stage should place next, stepping
/// `placed` over any peak the timeline does not play and answering
/// `None` only once the budget is spent. The Sfx derive returns `None`
/// to end its stage, so a peak that no clip plays - the tighten cut it
/// away, or the voiceover never reached it - must advance the cursor
/// and look again rather than end the stage and drop every later peak.
fn next_played_peak(
    placed: &mut usize,
    peaks: &[(f64, f64)],
    log: &[Value],
    takes: &[TakePaths],
) -> Option<f64> {
    let view = latest_view(log)?;
    let media = media_paths(view);
    while *placed < peaks.len() {
        let moment = peaks[*placed].0;
        let found = takes
            .iter()
            .map(|take| take.take.clone())
            .find_map(|take| timeline_at(view, &media, &take, moment));
        if let Some(at) = found {
            return Some(at);
        }
        *placed += 1;
    }
    None
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

/// One B-roll slot: a stretch of the narration with no words in it, the
/// take it was read from, and the words that describe what the picture
/// should show there.
struct BrollSlot {
    /// The take file the gap's neighbours were read from.
    take: String,
    /// Where the gap begins, in the take's own seconds.
    from: f64,
    /// Where it ends, in the take's own seconds.
    to: f64,
    /// The query the shelf is shopped with.
    query: String,
}

/// The last `n` words of a line: the words closest to the gap a shot
/// fills, which is what its search is shopped with.
fn tail_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let from = words.len().saturating_sub(n);
    words[from..].join(" ")
}

/// The B-roll slots a run should fill: the spoken spans read from the
/// transcripts, the gaps between them long enough to hold a shot and
/// short enough to be one, each carrying the tail of the words before it
/// as the query its search is shopped with. A narration with no such gap
/// has no slots, and the picture stage has nothing to add.
fn broll_slots(log: &[Value], takes: &[TakePaths]) -> Vec<BrollSlot> {
    const SHORTEST: f64 = 1.2;
    const LONGEST: f64 = 8.0;
    const MOST: usize = 6;
    const QUERY_WORDS: usize = 5;

    let mut slots: Vec<BrollSlot> = Vec::new();
    for (take, segments) in transcripts_of(log, takes) {
        let mut spoken: Vec<(f64, f64, String)> = segments
            .iter()
            .filter_map(|segment| {
                let start = segment["start"].as_f64()?;
                let end = segment["end"].as_f64()?;
                let text = segment["text"].as_str().unwrap_or_default().trim();
                (end > start && !text.is_empty()).then(|| (start, end, text.to_owned()))
            })
            .collect();
        spoken.sort_by(|left, right| left.0.total_cmp(&right.0));
        // The gaps are between one spoken span's end and the next one's
        // start; `cursor` carries the furthest end read so far, so an
        // overlapping segment can never open a gap inside the speech.
        let mut cursor: Option<f64> = None;
        for (start, end, _) in &spoken {
            if let Some(previous) = cursor {
                let gap = start - previous;
                if (SHORTEST..=LONGEST).contains(&gap) {
                    let before = spoken
                        .iter()
                        .rev()
                        .find(|(_, segment_end, _)| *segment_end <= *start)
                        .map(|(_, _, text)| tail_words(text, QUERY_WORDS))
                        .unwrap_or_default();
                    if !before.is_empty() {
                        slots.push(BrollSlot {
                            take: take.clone(),
                            from: previous,
                            to: *start,
                            query: before,
                        });
                    }
                }
            }
            cursor = Some(cursor.map_or(*end, |previous| previous.max(*end)));
        }
    }
    slots.truncate(MOST);
    slots
}

/// The titles the entrance pass keys: every text clip the timeline now
/// holds that is long enough to hold an entrance and is not already
/// keyed. The captions stage places them a batch earlier, so they are
/// the words the run just landed - the one place a one-click run can
/// give its type motion without a designer.
fn entrance_targets(log: &[Value]) -> Vec<(String, f64)> {
    let Some(view) = latest_view(log) else {
        return Vec::new();
    };
    view["timeline"]["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|clip| clip["kind"] == json!("text"))
        .filter(|clip| {
            !clip["keyframed"].as_array().is_some_and(|keys| {
                keys.iter()
                    .any(|key| key == &json!("scale") || key == &json!("offsetY"))
            })
        })
        .filter_map(|clip| {
            let id = clip["id"].as_str()?.to_owned();
            let duration = clip["duration"].as_f64().unwrap_or(0.0);
            (duration >= 0.4).then_some((id, duration))
        })
        .collect()
}

/// One title's entrance, as the batch of keys that draws it: the words
/// rise a little and settle as they arrive (scale 0.94 -> 1.0, offsetY
/// 0.05 -> 0), the keys placed as fractions of the title's own span so a
/// caption re-timed by the tighten still enters where it now begins. The
/// settle curve is the engine's own ease-out; a title too short to hold
/// an entrance is left still, and one already keyed is left alone.
fn entrance_keys(clip_id: &str, duration: f64) -> Vec<Value> {
    let span = (0.35 / duration).clamp(0.05, 0.6);
    let ease = json!([0.16, 1.0, 0.3, 1.0]);
    vec![
        json!({
            "op": "setClipKey", "clipId": clip_id, "property": "scale",
            "at": 0.0, "value": 0.94, "ease": ease,
        }),
        json!({
            "op": "setClipKey", "clipId": clip_id, "property": "scale",
            "at": round3(span), "value": 1.0, "ease": ease,
        }),
        json!({
            "op": "setClipKey", "clipId": clip_id, "property": "offsetY",
            "at": 0.0, "value": 0.05, "ease": ease,
        }),
        json!({
            "op": "setClipKey", "clipId": clip_id, "property": "offsetY",
            "at": round3(span), "value": 0.0, "ease": ease,
        }),
    ]
}

/// The AUTO EDIT's whole program: every stage the unattended run can run
/// without a creative brain, each standing on what the run itself
/// measured. The static spine repairs, transcribes, reads the voice and
/// gates the music; the fans run the takes' work at the same time; the
/// derived stages act on the replies - the dead air cut, the grade over
/// the whole runtime, the B-roll the narration's gaps are shopped for,
/// the captions placed on the measured words, the titles given an
/// entrance, the music bed shopped and ducked, the accents landed on the
/// emphasis peaks - and the verify reads the timeline as it ended up,
/// not as it was planned. The picture is built on the timeline, not
/// merely exported: a one-click run now leaves an editable cut, B-roll,
/// a look and moving titles included, for the window to tweak. The
/// judgement stages that still need a brain - the plan, the stock chosen
/// by eye, the baked motion comps, the preview/revise loop - stay the
/// Director agent's.
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
    // 6b. The grade: one look over the whole runtime, the skill's own
    //     default (an effect layer spanning the cut, never a five-second
    //     layer - the duration is the timeline the tighten just read, and
    //     the layer lands above the picture so it treats it). The look is
    //     the blockbuster one the shelf calls the most requested; a run
    //     with no runtime to read grades nothing.
    let grade_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Timeline,
            Box::new(move |log| {
                let view = latest_view(log)?;
                let runtime = view["timeline"]["clips"]
                    .as_array()?
                    .iter()
                    .map(|clip| {
                        clip["start"].as_f64().unwrap_or(0.0)
                            + clip["duration"].as_f64().unwrap_or(0.0)
                    })
                    .fold(0.0_f64, f64::max);
                if runtime < 0.5 {
                    return None;
                }
                Some(call(
                    "edit_apply",
                    json!({
                        "path": grade_project,
                        "command": {
                            "op": "addLayerClip",
                            "trackId": null,
                            "start": 0.0,
                            "duration": round3(runtime),
                            "effectId": "concat.teal-orange",
                            "name": "The look",
                        },
                    }),
                ))
            }),
        )
        .warn()
        .into(),
    );
    // 6c. The picture: the narration's own gaps, filled from the live
    //     Pexels/Pixabay shelves. The selected provider's licence travels
    //     with every search hit and import reply; the research-only OpenVid
    //     index is never used by AUTO EDIT. A missing key or empty shelf is
    //     a named warning and leaves that gap unfilled, not an unlicensed
    //     clip silently added to a project.
    let gather_source = takes.clone();
    let gather_project = project.clone();
    let gather_provider = source.stock_provider.clone();
    steps.push(
        DerivedStep::react(
            Stage::Gather,
            Box::new({
                let mut slots: Option<Vec<BrollSlot>> = None;
                let mut placed = 0usize;
                let mut phase = 0u8;
                let mut pending_media: Option<String> = None;
                let mut pending_len = 0.0f64;
                move |log| loop {
                    let slots = slots.get_or_insert_with(|| broll_slots(log, &gather_source));
                    if placed >= slots.len() {
                        return None;
                    }
                    let slot = &slots[placed];
                    match phase {
                        0 => {
                            phase = 1;
                            return Some(call(
                                "stock_search",
                                json!({
                                    "provider": gather_provider,
                                    "query": slot.query.clone(),
                                    "limit": 5,
                                }),
                            ));
                        }
                        1 => {
                            // Only a live-shelf hit with an explicit provider
                            // licence is eligible. OpenVid has no per-clip
                            // licence record and is never imported here.
                            let hit = log.last().and_then(|reply| reply.pointer("/hits/0"));
                            let provider = hit.and_then(|hit| hit["provider"].as_str());
                            let id = hit.and_then(|hit| hit["id"].as_str());
                            let url = hit.and_then(|hit| hit["url"].as_str());
                            let license = hit.and_then(|hit| hit["license"].as_str());
                            match (provider, id, url, license) {
                                (Some(provider), Some(id), Some(url), Some(license))
                                    if provider == gather_provider.as_str()
                                        && !id.trim().is_empty()
                                        && !url.trim().is_empty()
                                        && !license.trim().is_empty() =>
                                {
                                    phase = 2;
                                    return Some(call(
                                        "stock_import",
                                        json!({
                                            "path": gather_project,
                                            "provider": provider,
                                            "id": id,
                                            "url": url,
                                        }),
                                    ));
                                }
                                _ => {
                                    placed += 1;
                                    phase = 0;
                                }
                            }
                        }
                        2 => {
                            // The import's own reply: the clip is in the bin
                            // and the view it carries maps the gap onto the
                            // timeline. Lay the clip there.
                            let media_id = log
                                .last()
                                .and_then(|reply| reply.pointer("/mediaIds/0"))
                                .and_then(|id| id.as_str())
                                .map(str::to_owned);
                            let at = latest_view(log).and_then(|view| {
                                let media = media_paths(view);
                                timeline_at(view, &media, &slot.take, slot.from)
                            });
                            let to = latest_view(log).and_then(|view| {
                                let media = media_paths(view);
                                timeline_at(view, &media, &slot.take, slot.to)
                            });
                            match (media_id, at, to) {
                                (Some(media_id), Some(at), Some(to)) if to - at > 0.4 => {
                                    pending_media = Some(media_id.clone());
                                    pending_len = round3(to - at);
                                    phase = 3;
                                    return Some(call(
                                        "edit_apply",
                                        json!({
                                            "path": gather_project,
                                            "command": {
                                                "op": "addClipAtFirstFree",
                                                "mediaId": media_id,
                                                "start": round3(at),
                                            },
                                        }),
                                    ));
                                }
                                _ => {
                                    placed += 1;
                                    phase = 0;
                                }
                            }
                        }
                        3 => {
                            // The placement's own reply: the clip the import
                            // just added, by the media it plays. Trim it to
                            // the gap, fill the frame, drift it and dissolve
                            // it in - one undoable batch.
                            let media_id = pending_media.clone();
                            let found = latest_view(log).and_then(|view| {
                                view["timeline"]["clips"]
                                    .as_array()?
                                    .iter()
                                    .find(|clip| clip["media"].as_str() == media_id.as_deref())
                                    .map(|clip| {
                                        (
                                            clip["id"].as_str().unwrap_or_default().to_owned(),
                                            clip["duration"].as_f64().unwrap_or(0.0),
                                        )
                                    })
                            });
                            placed += 1;
                            phase = 0;
                            let Some((clip_id, current)) = found else {
                                continue;
                            };
                            let mut commands: Vec<Value> = Vec::new();
                            let delta = pending_len - current;
                            if delta < -0.05 {
                                commands.push(json!({
                                    "op": "trimClip",
                                    "clipId": clip_id,
                                    "edge": "end",
                                    "delta": round3(delta),
                                }));
                            }
                            commands.push(json!({
                                "op": "frameClip",
                                "clipId": clip_id,
                                "framing": "fill",
                            }));
                            commands.push(json!({
                                "op": "setClipKey",
                                "clipId": clip_id,
                                "property": "scale",
                                "at": 0.0,
                                "value": 1.0,
                                "ease": [0.0, 0.0, 0.58, 1.0],
                            }));
                            commands.push(json!({
                                "op": "setClipKey",
                                "clipId": clip_id,
                                "property": "scale",
                                "at": 1.0,
                                "value": 1.06,
                                "ease": [0.0, 0.0, 0.58, 1.0],
                            }));
                            commands.push(json!({
                                "op": "updateClip",
                                "clipId": clip_id,
                                "patch": {
                                    "transitionIn": { "id": "concat.dissolve", "duration": 0.3 },
                                },
                            }));
                            return Some(call(
                                "edit_apply",
                                json!({
                                    "path": gather_project,
                                    "command": { "op": "batch", "commands": commands },
                                }),
                            ));
                        }
                        _ => return None,
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
                // The captions are one edit, made once: the edit's own
                // reply is a timeline view carrying the titles just
                // placed, and re-deriving from it would place every
                // caption a second time.
                let mut issued = false;
                move |log| {
                    if issued {
                        return None;
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
                    issued = true;
                    Some(ToolCall {
                        tool: "edit_apply".to_owned(),
                        arguments: json!({
                            "path": caption_project,
                            "command": { "op": "batch", "commands": commands },
                        }),
                    })
                }
            }),
        )
        .warn()
        .into(),
    );
    // 8. The entrance: the titles the captions just landed, given motion
    //    the way the skill's own words ask for - the words rise a little
    //    and settle as they arrive. One undoable batch keys every title
    //    the timeline now holds; a title already keyed, or too short to
    //    hold an entrance, is left still. This is the type motion a
    //    one-click run can give itself; the baked motion comps (a comp
    //    rendered through the Geneva sidecar and composited) still need
    //    the Director's eye and stay its stage.
    let motion_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Motion,
            Box::new({
                let mut issued = false;
                move |log| {
                    if issued {
                        return None;
                    }
                    let targets = entrance_targets(log);
                    if targets.is_empty() {
                        return None;
                    }
                    let mut commands: Vec<Value> = Vec::new();
                    for (id, duration) in &targets {
                        commands.extend(entrance_keys(id, *duration));
                    }
                    issued = true;
                    Some(call(
                        "edit_apply",
                        json!({
                            "path": motion_project,
                            "command": { "op": "batch", "commands": commands },
                        }),
                    ))
                }
            }),
        )
        .warn()
        .into(),
    );
    // 9. The music bed, only where the gate said the take carries none:
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
                // How many replies the run had drawn before this stage's
                // first call: everything before that index belongs to the
                // earlier stages, and the stage reads only what it drew
                // itself. The captions stage leaves a timeline view behind,
                // and reading it as this stage's own is how the bed was
                // never shopped.
                let mut base: Option<usize> = None;
                // The bed is ducked once; the duck's own reply is another
                // timeline view, and the stage must not duck it again.
                let mut ducked = false;
                move |log| {
                    // The gate's own answer first: music already in the
                    // take means no bed, ever.
                    if log
                        .iter()
                        .any(|reply| reply.get("musicPresent") == Some(&json!(true)))
                    {
                        return None;
                    }
                    if ducked {
                        return None;
                    }
                    let base = *base.get_or_insert(log.len());
                    if log.len() == base {
                        // The stage's own first call: the shelf's state.
                        return Some(call("bgm_status", json!({})));
                    }
                    let last = log.last().expect("the log grew since base");
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
                                if polls > 200 {
                                    return None;
                                }
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
                    if last.get("mediaIds").is_some() {
                        // The import's own reply (`{mediaIds, view}`): read
                        // the timeline back for the bed's clip id.
                        return Some(call("timeline_read", json!({ "path": music_project })));
                    }
                    if last.pointer("/timeline/clips").is_some() {
                        // The timeline read after the import: the bed is
                        // the clip whose media landed in the project's bgm
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
                            ducked = true;
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
                    None
                }
            }),
        )
        .warn()
        .into(),
    );
    // 10. The accents: the reading's own emphasis peaks, the strongest
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
                    // The next peak the timeline actually plays: a peak
                    // cut away or never placed is stepped over, not the
                    // end of the stage. The runner reads `None` as "the
                    // accents are done", so answering it for one unplayed
                    // peak would drop every peak after it - the run would
                    // quietly lose the accents it was written to land.
                    let at = next_played_peak(&mut placed, peaks, log, &sfx_source)?;
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
                        if let Some(id) = id {
                            placed += 1;
                            return Some(call(
                                "sounds_add",
                                json!({ "path": sfx_project, "id": id, "start": round3(at) }),
                            ));
                        }
                        // The shelf answered with nothing for this peak:
                        // step to the next peak the timeline plays and
                        // shop it, rather than ending the stage and
                        // dropping every accent after it.
                        placed += 1;
                        next_played_peak(&mut placed, peaks, log, &sfx_source)?;
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
    // 11. The render, waited on by its project folder: the job's name is
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
    // 12. The verify: the sanity reads first - collisions, and the
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
    // 13. The watch, warn-policy: the whole cut as one contact sheet,
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
    /// grade, licensed live-stock B-roll, captions, title entrances,
    /// music bed and accents, the export, the verify and the watch. The
    /// plan, eye-picked stock, baked motion comps and preview/revise loop
    /// remain the Director agent's.
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
            stock_provider: "pexels".to_owned(),
        };
        let program = super::auto_edit_program(&source);

        // The register, in the run's own order: the fanned repair and
        // transcription, the derived tighten, grade, B-roll, captions,
        // title entrances, bed and accents, and the static
        // export/verify-gates/watch. The creative layer's remaining
        // stages (Plan, the baked motion comps, Preview, Revise) are
        // absent - the fraction the page shows counts only these.
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
                "timeline",
                "gather",
                "captions",
                "motion",
                "music",
                "sfx",
                "export",
                "verify",
                "watch",
            ]
        );
        // The warn-policy stages, and only them: a take that cannot be
        // repaired ships as recorded, a tighten or grade or B-roll slot or
        // caption or title entrance or bed or accent that cannot land is a
        // flaw the edit ships with, and a low craft score is recorded
        // yellow, not thrown away.
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
                "timeline",
                "gather",
                "captions",
                "motion",
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

        // The tighten, the grade, the B-roll, the captions, the title
        // entrances, the bed and the accents are derived: their calls are
        // the run's own replies to answer for, and the runner's reactive
        // queue asks for them one at a time.
        for at in [5usize, 6, 7, 8, 9, 10, 11] {
            assert!(
                matches!(program.steps[at], ProgramStep::Derived(_)),
                "step {at} is derived"
            );
        }

        // The export is waited on by its project folder - the job's name
        // is minted at run time - and the watch's sheet is a project-
        // relative name, because the window's server refuses absolute
        // write paths.
        let export = program.steps[12].as_static();
        assert_eq!(export.calls[0].tool, "export_start");
        assert_eq!(export.calls[0].arguments["output"], source.export.as_str());
        assert_eq!(export.calls[1].tool, "export_wait");
        assert_eq!(export.calls[1].arguments["path"], source.project.as_str());
        let watch = program.steps[14].as_static();
        assert_eq!(watch.calls[0].arguments["out"], "auto-edit-sheet.png");
        assert_eq!(
            watch.calls[1].arguments["pngPath"],
            "/edits/Reel edit/auto-edit-sheet.png"
        );

        // The verify's gates: the loudness target, and the words (F-04) -
        // on the derived step that reads the timeline as it ended up.
        use concat_director::ReplyGate;
        assert!(matches!(program.steps[13], ProgramStep::Derived(_)));
        assert_eq!(
            program.steps[13].gates(),
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
            program.steps[14].gates(),
            vec![ReplyGate::AtLeast {
                field: "overall",
                minimum: 7.0,
                what: "the cut scored under the craft bar",
            }]
        );
        // No other step gates its replies.
        for (at, step) in program.steps.iter().enumerate() {
            if at != 13 && at != 14 {
                assert!(step.gates().is_empty(), "step {at} gates its replies");
            }
        }
        // The fan compile-time proof the type system wanted: the repair
        // grouping is the FanStep the runner fans out.
        let _: &FanStep = repair;
    }

    /// The accents stage steps over a peak the timeline does not play
    /// instead of ending on it: the runner reads a derive's `None` as
    /// "the stage is done", so answering `None` for one unplayed peak
    /// would drop every peak after it.
    #[test]
    fn the_accents_step_over_a_peak_the_timeline_does_not_play() {
        let takes = vec![TakePaths {
            take: "/takes/a.wav".to_owned(),
            enhanced: "/takes/a-enhanced.wav".to_owned(),
        }];
        // One clip playing file seconds 0..10 of the take.
        let log = vec![json!({
            "media": [{ "id": "m1", "path": "/takes/a.wav" }],
            "timeline": { "clips": [{
                "id": "c1",
                "media": "m1",
                "start": 0.0,
                "duration": 10.0,
                "sourceStart": 0.0,
                "speed": 1.0,
            }]},
        })];
        // The 50 s peak is past the clip; the cursor at it must reach the
        // 2 s peak beyond rather than end the stage.
        let peaks = [(1.0, 0.9), (50.0, 0.8), (2.0, 0.7)];
        let mut placed = 1;
        assert_eq!(
            next_played_peak(&mut placed, &peaks, &log, &takes),
            Some(2.0),
            "the unplayed peak is stepped over, not the end of the stage"
        );
        assert_eq!(placed, 2, "the cursor landed on the played peak");
        // Once every peak is spent, the stage is done.
        placed = 3;
        assert_eq!(next_played_peak(&mut placed, &peaks, &log, &takes), None);
    }

    /// The music stage reads only its own replies: the captions stage
    /// leaves a timeline view behind, and reading it as the stage's own
    /// made the run duck a bed that was never shopped - the bed was
    /// never added at all.
    #[test]
    fn the_music_stage_shops_the_bed_instead_of_reading_the_captions_view() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: "/edits/Reel/voiceover/enhanced-000-a.wav".to_owned(),
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(music) = &mut program.steps[10] else {
            panic!("step 10 is the music stage");
        };
        // The log as the stage finds it: the captions stage's own view,
        // with a timeline and clips - exactly the shape the old code read
        // as "a bed already exists".
        let captions_view = json!({
            "project": { "name": "Reel" },
            "timeline": { "id": "t1", "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 2.0 }
            ]},
            "media": [{ "id": "m1", "path": "/edits/Reel/voiceover/take-000-a.wav" }],
        });
        let mut log = vec![captions_view];
        // First call: the shelf's state, not a duck of nothing.
        assert_eq!(
            (music.next)(log.as_slice())
                .map(|call| call.tool)
                .as_deref(),
            Some("bgm_status")
        );
        log.push(json!({ "freepd": "missing", "lofi": "missing" }));
        assert_eq!(
            (music.next)(log.as_slice())
                .map(|call| call.tool)
                .as_deref(),
            Some("bgm_prepare")
        );
        log.push(json!({ "freepd": "ready", "lofi": "missing" }));
        assert_eq!(
            (music.next)(log.as_slice())
                .map(|call| call.tool)
                .as_deref(),
            Some("bgm_search")
        );
        log.push(json!({ "tracks": [{ "id": "freepd-abc123" }] }));
        assert_eq!(
            (music.next)(log.as_slice())
                .map(|call| call.tool)
                .as_deref(),
            Some("bgm_import")
        );
        // The import's own reply carries `mediaIds`, not `state`.
        log.push(json!({ "mediaIds": ["m9"], "view": { "project": {} } }));
        assert_eq!(
            (music.next)(log.as_slice())
                .map(|call| call.tool)
                .as_deref(),
            Some("timeline_read")
        );
        // The timeline read now carries the bed in the project's bgm
        // folder: duck it under the voice.
        log.push(json!({
            "timeline": { "id": "t1", "clips": [
                { "id": "bed1", "media": "m9", "start": 0.0, "duration": 8.0 }
            ]},
            "media": [{ "id": "m9", "path": "/edits/Reel/bgm/track-abc123.mp3" }],
        }));
        let next = (music.next)(log.as_slice()).expect("the bed is ducked");
        assert_eq!(next.tool, "clip_audio_levels_db");
        assert_eq!(next.arguments["clipId"], "bed1");
        assert_eq!(next.arguments["baseDb"], -20.0);
        // The duck's own reply is another view: the stage is done, not
        // ducking a second time.
        log.push(json!({ "timeline": { "id": "t1", "clips": [] }, "media": [] }));
        assert_eq!((music.next)(log.as_slice()), None);
    }

    /// The captions are one edit, made once: the edit's own reply is a
    /// timeline view, and re-deriving from it would place every caption
    /// again.
    #[test]
    fn the_captions_are_issued_once() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: "/edits/Reel/voiceover/enhanced-000-a.wav".to_owned(),
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(captions) = &mut program.steps[8] else {
            panic!("step 8 is the captions stage");
        };
        let view = json!({
            "timeline": { "id": "t1", "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 4.0,
                  "sourceStart": 0.0, "speed": 1.0 }
            ]},
            "media": [{ "id": "m1", "path": "/edits/Reel/voiceover/take-000-a.wav" }],
        });
        // A transcribe reply whose words fall inside the clip's span.
        let transcript = json!({
            "segments": [{
                "start": 0.5, "end": 1.5, "text": "hello there",
                "words": [{ "start": 0.5, "end": 1.5 }],
            }],
        });
        let mut log = vec![
            view,
            json!({ "path": "/edits/Reel/voiceover/take-000-a.wav", "output": transcript.to_string() }),
        ];
        let first = (captions.next)(log.as_slice()).expect("the captions are placed");
        assert_eq!(first.tool, "edit_apply");
        // The edit's own reply: another view. The stage is done.
        log.push(json!({ "timeline": { "id": "t1", "clips": [] }, "media": [] }));
        assert_eq!(
            (captions.next)(log.as_slice()),
            None,
            "the captions are issued once"
        );
    }

    /// The B-roll planner reads the narration's own gaps: a stretch
    /// between two spoken spans long enough to hold a shot becomes one
    /// slot, shopped with the tail of the words before it; a narration
    /// whose words run together has none.
    #[test]
    fn the_broll_slots_are_the_narration_s_gaps() {
        let takes = vec![TakePaths {
            take: "/takes/a.wav".to_owned(),
            enhanced: "/takes/a-enhanced.wav".to_owned(),
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "the city skyline at dusk tonight" },
                { "start": 4.0, "end": 6.0, "text": "and the crowds crossing" },
                { "start": 6.1, "end": 7.0, "text": "the street" },
            ],
        });
        let log = vec![json!({
            "path": "/takes/a.wav",
            "output": transcript.to_string(),
        })];
        let slots = broll_slots(&log, &takes);
        assert_eq!(slots.len(), 1, "one gap is long enough to hold a shot");
        assert_eq!(slots[0].query, "city skyline at dusk tonight");
        assert_eq!(slots[0].from, 2.0);
        assert_eq!(slots[0].to, 4.0);

        // No gap: the words run together, and the picture stage has
        // nothing to add.
        let tight = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "one" },
                { "start": 2.0, "end": 4.0, "text": "two" },
            ],
        });
        let log = vec![json!({ "path": "/takes/a.wav", "output": tight.to_string() })];
        assert!(broll_slots(&log, &takes).is_empty());
    }

    /// The gather stage uses a licensed live shelf and never weakens the
    /// OpenVid licence gate by asserting that an unknown project is
    /// non-monetized.
    #[test]
    fn the_broll_stage_imports_only_licensed_live_stock() {
        use concat_director::ProgramStep;

        let take = "/takes/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: "/takes/a-enhanced.wav".to_owned(),
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(gather) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "gather")
            .expect("the program has a gather stage")
        else {
            panic!("the gather stage is derived");
        };

        let view = json!({
            "timeline": { "clips": [
                {
                    "id": "voice",
                    "media": "voice-media",
                    "start": 0.0,
                    "duration": 10.0,
                    "sourceStart": 0.0,
                    "speed": 1.0
                }
            ]},
            "media": [{ "id": "voice-media", "path": take }],
        });
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "the city skyline at dusk tonight" },
                { "start": 4.0, "end": 6.0, "text": "the crowds crossing" },
            ],
        });
        let mut log = vec![
            view,
            json!({ "path": take, "output": transcript.to_string() }),
        ];

        let search = (gather.next)(&log).expect("the gap is searched");
        assert_eq!(search.tool, "stock_search");
        assert_eq!(search.arguments["provider"], "pexels");
        assert_eq!(search.arguments["query"], "city skyline at dusk tonight");
        assert!(search.arguments.get("monetized").is_none());

        log.push(json!({
            "provider": "pexels",
            "license": "Pexels License",
            "hits": [{
                "id": "2499611",
                "provider": "pexels",
                "url": "https://videos.pexels.com/files/hd.mp4",
                "license": "Pexels License"
            }]
        }));
        let import = (gather.next)(&log).expect("the licensed hit is imported");
        assert_eq!(import.tool, "stock_import");
        assert_eq!(import.arguments["provider"], "pexels");
        assert_eq!(import.arguments["id"], "2499611");
        assert_eq!(
            import.arguments["url"],
            "https://videos.pexels.com/files/hd.mp4"
        );
        assert!(import.arguments.get("monetized").is_none());

        log.push(json!({
            "mediaIds": ["stock-media"],
            "license": "Pexels License",
            "view": { "timeline": { "clips": [] }, "media": [] }
        }));
        let place = (gather.next)(&log).expect("the clip is placed at the gap");
        assert_eq!(place.tool, "edit_apply");
        assert_eq!(place.arguments["command"]["op"], "addClipAtFirstFree");
        assert_eq!(place.arguments["command"]["mediaId"], "stock-media");
        assert_eq!(place.arguments["command"]["start"], 2.0);

        log.push(json!({
            "timeline": { "clips": [
                {
                    "id": "voice",
                    "media": "voice-media",
                    "start": 0.0,
                    "duration": 10.0,
                    "sourceStart": 0.0,
                    "speed": 1.0
                },
                {
                    "id": "stock-clip",
                    "media": "stock-media",
                    "start": 2.0,
                    "duration": 8.0
                }
            ]},
            "media": [
                { "id": "voice-media", "path": take },
                { "id": "stock-media", "path": "/edits/Reel/stock/pexels-2499611.mp4" }
            ]
        }));
        let finish = (gather.next)(&log).expect("the placed clip is styled");
        assert_eq!(finish.tool, "edit_apply");
        let commands = finish.arguments["command"]["commands"]
            .as_array()
            .expect("one undoable styling batch");
        assert!(commands.iter().any(|command| command["op"] == "trimClip"));
        assert!(commands.iter().any(|command| command["op"] == "frameClip"));
        assert!(commands.iter().any(|command| command["op"] == "updateClip"));
        assert!(
            (gather.next)(&log).is_none(),
            "one gap is placed only once"
        );
    }

    /// The entrance pass keys the titles the timeline holds, and only
    /// them: a picture clip and a title too short to hold an entrance are
    /// left still, and the keys are fractions of the title's own span.
    #[test]
    fn the_entrance_keys_the_titles_the_timeline_holds() {
        let view = json!({
            "timeline": { "clips": [
                { "id": "b1", "kind": "video", "start": 0.0, "duration": 4.0 },
                { "id": "t1", "kind": "text", "start": 1.0, "duration": 2.0 },
                { "id": "t2", "kind": "text", "start": 3.2, "duration": 0.3 },
                { "id": "t3", "kind": "text", "start": 4.0, "duration": 1.0,
                  "keyframed": ["scale"] }
            ]},
        });
        let log = vec![view];
        let targets = entrance_targets(&log);
        assert_eq!(
            targets,
            vec![("t1".to_owned(), 2.0)],
            "only the picture-free title long enough to hold an entrance"
        );

        // Two seconds of title: the entrance settles within the first
        // 0.35 s, a fraction of the title's own span.
        let keys = entrance_keys("t1", 2.0);
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0]["property"], "scale");
        assert_eq!(keys[0]["at"], 0.0);
        assert_eq!(keys[0]["value"], 0.94);
        assert_eq!(keys[1]["at"], 0.175);
        assert_eq!(keys[1]["value"], 1.0);
        assert_eq!(keys[2]["property"], "offsetY");
        assert_eq!(keys[3]["value"], 0.0);

        // A very short title still settles inside its span, not past it.
        let keys = entrance_keys("t", 0.5);
        assert!(keys[1]["at"].as_f64().unwrap() <= 0.6);
    }

    /// A clip with no media - a title the run placed - does not end the
    /// file-second scan: the take's pieces are still mapped.
    #[test]
    fn timeline_at_steps_over_clips_with_no_media() {
        let view = json!({
            "timeline": { "clips": [
                { "id": "title", "start": 0.0, "duration": 1.0 },
                { "id": "c1", "media": "m1", "start": 1.0, "duration": 4.0,
                  "sourceStart": 0.0, "speed": 1.0 }
            ]},
        });
        let media = vec![("m1".to_owned(), "/takes/a.wav".to_owned())];
        assert_eq!(
            timeline_at(&view, &media, "/takes/a.wav", 2.0),
            Some(3.0),
            "the title is skipped and the file moment still maps"
        );
    }
}
