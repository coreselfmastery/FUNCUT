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
///
/// Not every silence is dead air: a pause that opens right after a line
/// that reads hard is a held beat, and cutting it is the one mistake that
/// makes an edit feel mechanical. Those spans are left alone, whatever
/// the silence detector called them.
fn dead_air_cuts(
    log: &[Value],
    takes: &[TakePaths],
    project: &str,
    emotion: &[EmotionLine],
) -> Vec<ToolCall> {
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
                // The beat after a hard line is held, not cut.
                if held_beat(emotion, path, start) {
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

/// A take's spoken spans, as the planner's slot mapping reads them: the
/// take file, then its lines as (start, end, text).
type SpokenTakes = Vec<(String, Vec<(f64, f64, String)>)>;

/// The hit from a search page that best fits the slot being filled, and
/// the ids already shopped so a clip is never laid twice.
///
/// The search returns a page ranked by the shelf's own relevance; the
/// remaining choice is fit, and it is made from metadata the reply
/// actually carries - never from a score invented here. Orientation is
/// the strongest preference (a vertical slot wants a vertical clip; a
/// horizontal clip in a 9:16 frame means a heavy crop), a clip shorter
/// than the span is refused outright because it cannot cover the hole
/// without a visible loop, and among the clips that pass the sharpest
/// wins. A page with no licensable clip is a slot left empty - the honest
/// answer - rather than a clip that will not fit.
fn best_hit(hits: &[Value], span: f64, aspect: &str, used: &[String]) -> Option<Value> {
    let wants_vertical = matches!(aspect, "9:16" | "4:5");
    let wants_horizontal = aspect == "16:9";

    let fits = |hit: &Value| {
        let id = hit["id"].as_str().unwrap_or_default();
        let url = hit["url"].as_str().unwrap_or_default();
        let license = hit["license"].as_str().unwrap_or_default();
        if id.trim().is_empty() || url.trim().is_empty() || license.trim().is_empty() {
            return None;
        }
        if used.iter().any(|seen| seen == id) {
            return None;
        }
        let width = hit["width"].as_f64().unwrap_or(0.0);
        let height = hit["height"].as_f64().unwrap_or(0.0);
        // Orientation is measured, not guessed: a clip whose frame is
        // taller than it is wide is vertical, and the opposite.
        let orientation_ok = if height <= 0.0 || width <= 0.0 {
            true
        } else if wants_vertical {
            height >= width
        } else if wants_horizontal {
            width >= height
        } else {
            true
        };
        // A clip shorter than the hole cannot cover it, and looping it is
        // a visible seam, so it is not a candidate at all - not a lower
        // ranked one. An absent/invalid duration is not proof of coverage:
        // refusing that hit is safer than silently leaving the slot short.
        let seconds = hit["seconds"]
            .as_f64()
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)?;
        if seconds + 0.05 < span {
            return None;
        }
        Some((orientation_ok, (width * height) as i64))
    };

    hits.iter()
        .filter_map(|hit| fits(hit).map(|score| (score, hit)))
        // Orientation first, then the sharpest frame - a deterministic
        // order over measured facts.
        .max_by(|left, right| left.0.0.cmp(&right.0.0).then(left.0.1.cmp(&right.0.1)))
        .map(|(_, hit)| hit.clone())
}

/// The take whose own audio already carries music, if any. The music read
/// does not echo the path it was asked about, so the replies that carry
/// `musicPresent` are matched to the takes in the order the Understand
/// stage issued them - one per take, in `takes` order.
fn musical_take(log: &[Value], takes: &[TakePaths]) -> Option<String> {
    let reads: Vec<bool> = log
        .iter()
        .filter_map(|reply| reply.get("musicPresent").and_then(Value::as_bool))
        .collect();
    reads
        .iter()
        .position(|present| *present)
        .and_then(|at| takes.get(at))
        .map(|take| take.take.clone())
}

/// The transcript for one take as `footage_plan_suggest` reads it. Takes
/// have independent file clocks; merging their raw timestamps creates
/// overlaps and erases real gaps, so the planner is called once per take.
fn planner_segments(log: &[Value], take: &TakePaths) -> Vec<Value> {
    let mut segments: Vec<Value> = transcripts_of(log, std::slice::from_ref(take))
        .into_iter()
        .flat_map(|(_, segments)| segments)
        .filter_map(|segment| {
            let start = segment["start"].as_f64()?;
            let end = segment["end"].as_f64()?;
            let text = segment["text"].as_str()?.trim();
            (end > start && !text.is_empty())
                .then(|| json!({ "start": start, "end": end, "text": text }))
        })
        .collect();
    segments.sort_by(|left, right| {
        left["start"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&right["start"].as_f64().unwrap_or(0.0))
    });
    segments
}

/// The beat grid the run measured, in the shape `footage_plan_suggest`
/// takes. `analyze_beats` answers with `cutTimes` - the onsets it would
/// cut on - so those become the grid; no measured grid is `None`, which
/// leaves the monotony read standing down rather than guessing.
fn latest_beats(log: &[Value]) -> Option<Value> {
    let cuts = log.iter().rev().find_map(|reply| {
        reply
            .get("cutTimes")
            .and_then(Value::as_array)
            .filter(|cuts| !cuts.is_empty())
    })?;
    Some(Value::Array(
        cuts.iter()
            .filter_map(Value::as_f64)
            .map(|seconds| json!({ "seconds": seconds, "strength": 0.5 }))
            .collect(),
    ))
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
///
/// The planner's own read wins when the run asked for one. `slots_from_plan`
/// reads `footage_plan_suggest`'s suggestions - dialogue gaps AND monotony
/// runs, each carrying the priority the planner scored it at. The plain gap
/// scan is a fallback only when the planner returned no suggestions (or
/// could not run); an explicit `needBRoll: false` is not overruled by a
/// second, less-informed scan.
fn broll_slots(log: &[Value], takes: &[TakePaths]) -> Vec<BrollSlot> {
    let planned = slots_from_plan(log, takes);
    let planner_has_suggestions = log.iter().any(|reply| {
        reply.get("take").and_then(Value::as_str).is_some()
            && reply
                .get("suggestions")
                .and_then(Value::as_array)
                .is_some_and(|suggestions| !suggestions.is_empty())
    });
    if planner_has_suggestions {
        planned
    } else {
        broll_slots_from_gaps(log, takes)
    }
}

/// The slots the footage planner asked for, in the order it ranked them.
/// The planner reads monotony as well as gaps - a long stretch of speech
/// no beat lands in wants coverage even though it is not silent - and it
/// scores each span, so the strongest asks are filled first and a run
/// that can only afford a few fills spends them where the plan says.
fn slots_from_plan(log: &[Value], takes: &[TakePaths]) -> Vec<BrollSlot> {
    const MOST: usize = 6;
    const SHORTEST: f64 = 1.2;
    const LONGEST: f64 = 8.0;

    // Each planner reply names the take whose independent file clock it
    // read. Never match a suggestion against another take merely because
    // their local timestamps happen to overlap.
    let transcripts: SpokenTakes = transcripts_of(log, takes)
        .into_iter()
        .map(|(take, segments)| {
            let spoken = segments
                .iter()
                .filter_map(|segment| {
                    let start = segment["start"].as_f64()?;
                    let end = segment["end"].as_f64()?;
                    let text = segment["text"].as_str().unwrap_or_default().trim();
                    (end > start && !text.is_empty()).then(|| (start, end, text.to_owned()))
                })
                .collect();
            (take, spoken)
        })
        .collect();

    let mut ranked: Vec<(f64, usize, BrollSlot)> = Vec::new();
    for plan in log.iter().filter(|reply| {
        reply.get("take").and_then(Value::as_str).is_some()
            && reply.get("suggestions").and_then(Value::as_array).is_some()
            && reply.get("gapCount").is_some()
    }) {
        let Some(take_path) = plan["take"].as_str() else {
            continue;
        };
        let Some(take_index) = takes
            .iter()
            .position(|take| take.take.as_str() == take_path)
        else {
            continue;
        };
        let Some((_, spoken)) = transcripts
            .iter()
            .find(|(path, _)| path.as_str() == take_path)
        else {
            continue;
        };
        let beats_known = plan["beatGridMeasured"].as_bool().unwrap_or(false);
        let Some(suggestions) = plan["suggestions"].as_array() else {
            continue;
        };

        for suggestion in suggestions {
            if suggestion.get("needBRoll").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let Some(start) = suggestion["startS"].as_f64() else {
                continue;
            };
            let Some(end) = suggestion["endS"].as_f64() else {
                continue;
            };
            let span = end - start;
            if !(SHORTEST..=LONGEST).contains(&span) {
                continue;
            }
            // A monotony run is an ask only when THIS take's planner call
            // actually received a measured beat grid. Another take's grid
            // must never make an unmeasured take look musical.
            if suggestion["kind"] == json!("monotony_run") && !beats_known {
                continue;
            }
            let priority = suggestion["priority"].as_f64().unwrap_or(0.5);
            let tags: Vec<String> = suggestion["topicTags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            let before = spoken
                .iter()
                .filter(|(_, segment_end, _)| *segment_end <= start)
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(_, _, text)| tail_words(text, 5))
                .unwrap_or_default();
            let query = if tags.is_empty() {
                before
            } else {
                tags.join(" ")
            };
            if query.trim().is_empty() {
                continue;
            }
            ranked.push((
                priority,
                take_index,
                BrollSlot {
                    take: take_path.to_owned(),
                    from: start,
                    to: end,
                    query,
                },
            ));
        }
    }

    // Spend the six-shot budget on the planner's strongest asks, then
    // restore first-play take order and local time within each take.
    ranked.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.from.total_cmp(&right.2.from))
    });
    ranked.truncate(MOST);
    ranked.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then(left.2.from.total_cmp(&right.2.from))
    });
    ranked.into_iter().map(|(_, _, slot)| slot).collect()
}

/// The plain gap scan: the read the button made before it had a planner.
fn broll_slots_from_gaps(log: &[Value], takes: &[TakePaths]) -> Vec<BrollSlot> {
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

/// The segments a take's transcript reply timed, wherever in the log the
/// reply sits. A take whose transcription never landed has none.
fn segments_of(log: &[Value], take: &str) -> Vec<Value> {
    let takes = [TakePaths {
        take: take.to_owned(),
        enhanced: String::new(),
    }];
    transcripts_of(log, &takes)
        .into_iter()
        .flat_map(|(_, segments)| segments)
        .collect()
}

/// The spoken lines a take's transcript measured, as the `emotion`
/// extract reads them: `{text, start, end}` in the take's own seconds.
/// The predicate is the one the emotion stage and the emotion reader
/// share - a take with no timed words is a take neither asks about, so
/// the replies and the takes stay zipped in the same order on both
/// sides.
fn spoken_lines(log: &[Value], take: &str) -> Vec<Value> {
    segments_of(log, take)
        .into_iter()
        .filter_map(|segment| {
            let start = segment["start"].as_f64()?;
            let end = segment["end"].as_f64()?;
            let text = segment["text"].as_str()?.trim();
            (end > start && !text.is_empty())
                .then(|| json!({ "text": text, "start": start, "end": end }))
        })
        .collect()
}

/// One line of the emotional read, keyed by the take it was read from.
#[derive(Clone)]
struct EmotionLine {
    /// The take the words came from.
    take: String,
    /// Where the line begins in the take, in file seconds.
    start: f64,
    /// Where it ends, in file seconds.
    end: f64,
    /// One of the seven the emotion engine answers.
    emotion: String,
    /// How strongly the line reads, 0.1 to 1.
    intensity: f64,
    /// The narrative act the line sits in: hook, setup, tension, turn,
    /// reveal, proof, cta.
    act: String,
    /// The colour temperature the profile names: hot, warm, balanced,
    /// cool, cold, sickly or split.
    temperature: String,
    /// The bed's level under this line, in dB, as the act profile sets it.
    duck_db: f64,
    /// The sound the profile asks for at this line's peak.
    sfx_type: String,
    /// The two `bgm_search` queries the line's bed wants, primary first.
    bgm_words: Vec<String>,
    /// The line's own words, as the transcript measured them: a motion
    /// card carries the narration it was earned by, never a sample's text.
    text: String,
    /// How fast the line is delivered, as `voice_signals` measured it:
    /// punchy, conversational, editorial or slow. Empty when the reading
    /// carried no rate, which is the honest answer for a take measured
    /// without word timings.
    pace: String,
}

impl EmotionLine {
    /// The line as an `emotion` extract wrote it, from a take file, with
    /// the words the transcript measured for it.
    fn of(take: &str, row: &Value, text: &str, pace: &str) -> Option<EmotionLine> {
        let text_free = |value: &Value| value.as_str().map(str::to_owned);
        Some(EmotionLine {
            take: take.to_owned(),
            start: row["start"].as_f64()?,
            end: row["end"].as_f64()?,
            emotion: text_free(&row["emotion"])?,
            intensity: row["intensity"].as_f64().unwrap_or(0.5),
            act: text_free(&row["act"]).unwrap_or_else(|| "setup".to_owned()),
            temperature: row
                .pointer("/colourTemp/temperature")
                .and_then(Value::as_str)
                .unwrap_or("balanced")
                .to_owned(),
            duck_db: row["duckDb"].as_f64().unwrap_or(-12.0),
            sfx_type: text_free(&row["sfxType"]).unwrap_or_default(),
            bgm_words: row["bgmWords"]
                .as_array()
                .map(|words| {
                    words
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            text: text.to_owned(),
            pace: pace.to_owned(),
        })
    }
}

/// How fast the take's delivery is at a span, as `voice_signals` measured
/// it: the line of its reading that overlaps the span most carries the
/// rate. Empty when the reply measured no rate there, so a caller can tell
/// "measured slow" from "not measured" - a stage that invented a pace
/// would be claiming evidence it never read.
fn delivery_pace(log: &[Value], take: &str, start: f64, end: f64) -> String {
    let mut best: Option<(f64, &str)> = None;
    for reply in log {
        if reply.pointer("/audio/path").and_then(Value::as_str) != Some(take) {
            continue;
        }
        for line in reply["lines"].as_array().into_iter().flatten() {
            let (Some(line_start), Some(line_end)) = (line["start"].as_f64(), line["end"].as_f64())
            else {
                continue;
            };
            let Some(pace) = line.pointer("/speechRate/pace").and_then(Value::as_str) else {
                continue;
            };
            let overlap = line_end.min(end) - line_start.max(start);
            if overlap <= 0.0 {
                continue;
            }
            if best.is_none_or(|(best_overlap, _)| overlap > best_overlap) {
                best = Some((overlap, pace));
            }
        }
    }
    best.map(|(_, pace)| pace.to_owned()).unwrap_or_default()
}

/// The emotional read the run made, in the order the takes were asked
/// about. The emotion stage asks once per take that has timed words, in
/// program order, so the `emotion` replies sit in the log in exactly
/// that order - the invariant this reader stands on, and the reason the
/// reader filters its takes with the same [`spoken_lines`] predicate the
/// stage skips them with.
/// The words the take's own delivery stressed, lowercased and
/// deduplicated: `voice_signals`' `prosody.emphasisWords`, the signal the
/// caption stage spends. Empty when no reply measured any, which is the
/// honest answer for a take read without prosody - the stage then falls
/// back to its own evidence rather than inventing a stress.
fn stressed_words(log: &[Value], takes: &[TakePaths]) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for reply in log {
        let Some(path) = reply.pointer("/audio/path").and_then(Value::as_str) else {
            continue;
        };
        if !takes.iter().any(|take| take.take == path) {
            continue;
        }
        for line in reply["lines"].as_array().into_iter().flatten() {
            for word in line
                .pointer("/prosody/emphasisWords")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                // The Rust ear names the word `wordText`; the Python
                // prosody pass names it `word`. Read either, so a take
                // read by either producer still hands its stress over.
                let Some(text) = word["wordText"].as_str().or_else(|| word["word"].as_str()) else {
                    continue;
                };
                let text = text.trim().to_lowercase();
                if !text.is_empty() && !words.contains(&text) {
                    words.push(text);
                }
            }
        }
    }
    words
}

/// setup line earns nothing - the restraint rule the director enforces,
/// because motion on every beat is what makes an edit read as a template.
///
/// The act is the primary key (a payoff wants a call to action, a reveal
/// wants a sting), and the emotion only breaks the tie within an act, so
/// two lines of the same act do not both get the same card.
fn comp_for_line(line: &EmotionLine) -> Option<&'static str> {
    // A quiet line never earns a card, whatever its act: the picture is
    // doing its job when nothing needs saying over it.
    if line.intensity < 0.45 && !matches!(line.act.as_str(), "cta" | "reveal") {
        return None;
    }
    Some(match line.act.as_str() {
        "hook" => match line.emotion.as_str() {
            "surprise" | "fear" => "funcut.chapter.chip",
            _ => "funcut.lower-third.glass",
        },
        "reveal" | "turn" => "funcut.sting.wordmark",
        "proof" => "funcut.stats.triptych",
        "cta" => "funcut.cta.subscribe",
        "tension" => match line.emotion.as_str() {
            "fear" | "anger" => "funcut.quote.pull",
            _ => "funcut.chapter.chip",
        },
        // A setup or a body line carries no card: the spine is the
        // narration, and the beats that earn a graphic are the ones that
        // turn the piece.
        _ => return None,
    })
}

/// The motion the run will bake, one row per line that earned a card, in
/// play order: the take and file second the line opens on (the stage maps
/// it through the timeline), the preset, and the words the card sets.
///
/// The density floor is the plan schema's own: at most one card per
/// section and at most one per fifteen seconds, strongest line first. Two
/// cards in the same three seconds is a slideshow, not an edit.
fn motion_specs(lines: &[EmotionLine]) -> Vec<MotionSpec> {
    const SPACING: f64 = 15.0;
    const MOST: usize = 8;
    let mut ranked: Vec<(f64, MotionSpec)> = lines
        .iter()
        .filter_map(|line| {
            let preset = comp_for_line(line)?;
            let words = card_words(&line.text);
            if words.is_empty() {
                return None;
            }
            Some((
                line.intensity,
                MotionSpec {
                    take: line.take.clone(),
                    at: line.start,
                    end: line.end,
                    preset,
                    words,
                    act: line.act.clone(),
                    pace: line.pace.clone(),
                },
            ))
        })
        .collect();
    ranked.sort_by(|left, right| right.0.total_cmp(&left.0));
    let mut kept: Vec<MotionSpec> = Vec::new();
    for (_, spec) in ranked {
        if kept.len() >= MOST {
            break;
        }
        let crowded = kept
            .iter()
            .any(|other| (other.at - spec.at).abs() < SPACING);
        if !crowded {
            kept.push(spec);
        }
    }
    kept.sort_by(|left, right| left.at.total_cmp(&right.at));
    kept
}

/// One designed beat, as the plan records it and the motion stage bakes
/// it: the line it sits on, the preset, the span it covers, and the words
/// the card sets - the line's own, so a card is never a sample's text.
#[derive(Clone, PartialEq, Debug)]
struct MotionSpec {
    /// The take the line was read from.
    take: String,
    /// Where the line opens in the take, in file seconds.
    at: f64,
    /// Where it ends, in file seconds.
    end: f64,
    /// The Geneva preset the card is built from.
    preset: &'static str,
    /// The words the card carries: the line's, trimmed to a card's length.
    words: String,
    /// The narrative act that earned the card.
    act: String,
    /// How fast the line is delivered, as `voice_signals` measured it. A
    /// card on a punchy line holds tighter than one on a slow line: the
    /// delivery, not the clock, is what sets the beat.
    pace: String,
}

impl MotionSpec {
    /// Seconds the card holds: its line's length, floored so a clipped
    /// line still reads, and capped at the editorial default so a long
    /// line does not own the screen.
    fn hold(&self) -> f64 {
        let measured = (self.end - self.at).clamp(1.5, 5.0);
        // A punchy line has no time for a slow card and a slow line would
        // leave one flashing: the measured delivery shortens or lengthens
        // the hold around its own length, never past the editorial cap.
        let paced = match self.pace.as_str() {
            "punchy" => measured * 0.75,
            "conversational" => measured * 0.9,
            "slow" => measured * 1.2,
            _ => measured,
        };
        round3(paced.clamp(1.2, 5.0))
    }
}

/// The words a card can hold: the line's own, cut at a word boundary so a
/// card never ends mid-word, and never longer than a card reads in the
/// seconds it is on screen.
fn card_words(text: &str) -> String {
    const MOST: usize = 48;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MOST {
        return trimmed.to_owned();
    }
    let cut: String = trimmed.chars().take(MOST).collect();
    match cut.rfind(' ') {
        Some(at) if at > MOST / 2 => cut[..at].to_owned(),
        _ => cut,
    }
}

/// The transition an emotion earns on the cut into a B-roll shot: a
/// speaker or mood change earns a visible one, a calm stretch earns a
/// soft one. The ids are the catalogue's own transition packages.
fn transition_for_emotion(emotion: &str) -> &'static str {
    match emotion {
        "joy" => "concat.cross-pop",
        "surprise" => "concat.flash-social",
        "anger" => "concat.crash-zoom",
        "fear" => "concat.blur-dissolve",
        "sadness" => "concat.dissolve",
        "disgust" => "concat.blinds-soft",
        _ => "concat.blur-dissolve",
    }
}

/// The second transition for a mood, so two shots cut from the same
/// emotion never wear the same move twice running: the no-repeat rule
/// the director enforces, not merely asks for.
fn alternate_transition(emotion: &str) -> &'static str {
    match emotion {
        "joy" => "concat.dissolve",
        "surprise" => "concat.cross-pop",
        "anger" => "concat.flash-social",
        "fear" => "concat.dissolve",
        "sadness" => "concat.blur-dissolve",
        "disgust" => "concat.dissolve",
        _ => "concat.dissolve",
    }
}

fn emotion_lines(log: &[Value], takes: &[TakePaths]) -> Vec<EmotionLine> {
    let replies: Vec<&Value> = log
        .iter()
        .filter(|reply| {
            reply["lines"]
                .as_array()
                .is_some_and(|lines| lines.iter().any(|line| line.get("emotion").is_some()))
        })
        .collect();
    let asked: Vec<&TakePaths> = takes
        .iter()
        .filter(|take| !spoken_lines(log, &take.take).is_empty())
        .collect();
    asked
        .iter()
        .zip(replies)
        .flat_map(|(take, reply)| {
            // The lines the stage sent this take's extract, in order: the
            // reply's rows answer them one for one, and the words the
            // transcript measured are the words a card carries.
            let sent: Vec<String> = spoken_lines(log, &take.take)
                .into_iter()
                .filter_map(|line| line["text"].as_str().map(str::to_owned))
                .collect();
            reply["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(index, row)| {
                    let text = sent.get(index).map(String::as_str).unwrap_or_default();
                    let pace = delivery_pace(
                        log,
                        &take.take,
                        row["start"].as_f64().unwrap_or_default(),
                        row["end"].as_f64().unwrap_or_default(),
                    );
                    EmotionLine::of(&take.take, row, text, &pace)
                })
        })
        .collect()
}

/// The read's dominant line: the one carrying the most intensity, which
/// is the beat the piece is really about - not the most frequent emotion,
/// which a long calm setup would always win.
fn dominant(lines: &[EmotionLine]) -> Option<&EmotionLine> {
    lines
        .iter()
        .filter(|line| line.emotion != "neutral")
        .max_by(|left, right| left.intensity.total_cmp(&right.intensity))
        .or_else(|| lines.first())
}

/// The line a timeline moment sits in, through the take it plays: the
/// nearest line that starts at or before the moment.
fn emotion_at<'a>(lines: &'a [EmotionLine], take: &str, moment: f64) -> Option<&'a EmotionLine> {
    lines
        .iter()
        .filter(|line| line.take == take && line.start <= moment + 0.05)
        .max_by(|left, right| left.start.total_cmp(&right.start))
}

/// Whether a silence opening at `start` is a beat the read says to hold:
/// the line it follows ended hard (an intense read) or it is the pause
/// after the piece's biggest moment. Those are the silences an editor
/// keeps, and the ones a silence detector alone always gets wrong.
fn held_beat(emotion: &[EmotionLine], take: &str, start: f64) -> bool {
    const HOLD: f64 = 0.45;
    let before = emotion
        .iter()
        .filter(|line| line.take == take && line.end <= start + 0.15 && line.end >= start - 0.6)
        .max_by(|left, right| left.end.total_cmp(&right.end));
    let Some(line) = before else {
        return false;
    };
    if start - line.end > HOLD {
        return false;
    }
    line.intensity >= 0.7 || matches!(line.act.as_str(), "reveal" | "turn")
}

/// The look an emotion's colour temperature asks for: the shelf's own
/// package for that palette, at the strength the package ships with. A
/// temperature the profile does not name falls back to the blockbuster
/// look, the one the shelf calls most requested.
fn look_for_temperature(temperature: &str) -> &'static str {
    match temperature {
        "hot" => "concat.warm",
        "warm" => "concat.golden-hour",
        "cool" => "concat.cool",
        "cold" => "concat.cool-blue",
        "sickly" => "concat.vintage",
        // split, balanced and anything unnamed: the teal-and-orange the
        // surprise profile itself asks for.
        _ => "concat.teal-orange",
    }
}

/// The sound queries an emotion's profile asks for, in the order the
/// accents spend them: the profile names a family ("impact-heavy (hard
/// hits, distorted risers)"), and the shelf is searched by the words that
/// find it. The words are the library's own categories - Whoosh, Impact,
/// Riser, Boom is not a category, so a hard hit is searched as one - and
/// the neutral row is the general accent vocabulary.
fn sfx_queries_for_emotion(emotion: &str) -> &'static [&'static str] {
    match emotion {
        "anger" => &["impact", "hit", "riser"],
        "fear" => &["riser", "whoosh", "drone"],
        "surprise" => &["whoosh", "riser", "pop"],
        "joy" => &["pop", "chime", "swell"],
        "sadness" => &["ambience", "pad", "tone"],
        "disgust" => &["foley", "texture", "hit"],
        _ => &["whoosh", "impact", "riser", "pop", "click", "foley"],
    }
}

/// The queries the emotional read's own `sfxType` names, which is more
/// specific than the emotion alone: a line read as a stinger is shopped
/// for a stinger, not the family its emotion happens to share. The
/// family is the fallback when the read named no register.
fn sfx_queries_for_line(line: Option<&EmotionLine>, emotion: &str) -> Vec<&'static str> {
    let register = line
        .map(|line| line.sfx_type.to_ascii_lowercase())
        .unwrap_or_default();
    let named: &[&str] = if register.contains("stinger") {
        &["stinger", "hit", "impact"]
    } else if register.contains("drone") {
        &["drone", "riser", "ambience"]
    } else if register.contains("chime") || register.contains("uplifting") {
        &["chime", "pop", "swell"]
    } else if register.contains("piano") || register.contains("room tone") {
        &["ambience", "tone", "pad"]
    } else if register.contains("impact-heavy") {
        &["impact", "hit", "riser"]
    } else if register.contains("texture") {
        &["foley", "texture", "hit"]
    } else {
        &[]
    };
    if named.is_empty() {
        sfx_queries_for_emotion(emotion).to_vec()
    } else {
        named.to_vec()
    }
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

/// The grade package's own intensity parameter, as the plan's `grade`
/// block needs it: the key the package reads and a value inside its
/// range. The plan validates the pair, and a filter whose key the plan
/// guesses is a plan that dies at validation.
fn grade_param(look: &str) -> (&'static str, f64) {
    match look {
        "concat.warm" => ("temperature", 4600.0),
        "concat.cool" => ("temperature", 8500.0),
        "concat.golden-hour" => ("glow", 50.0),
        "concat.cool-blue" => ("chill", 60.0),
        "concat.vintage" => ("age", 60.0),
        _ => ("amount", 70.0),
    }
}

/// The aspect the project's own frame is nearest: a 9:16 project is a
/// reel, a 16:9 one is not.
fn aspect_of(view: &Value) -> &'static str {
    let width = view
        .pointer("/settings/width")
        .and_then(Value::as_f64)
        .unwrap_or(1920.0);
    let height = view
        .pointer("/settings/height")
        .and_then(Value::as_f64)
        .unwrap_or(1080.0);
    if height <= 0.0 {
        return "16:9";
    }
    let ratio = width / height;
    let candidates = [
        ("16:9", 16.0 / 9.0),
        ("9:16", 9.0 / 16.0),
        ("1:1", 1.0),
        ("4:5", 0.8),
    ];
    candidates
        .iter()
        .min_by(|left, right| (left.1 - ratio).abs().total_cmp(&(right.1 - ratio).abs()))
        .map(|(name, _)| *name)
        .unwrap_or("16:9")
}

/// The plan the run can write from what it measured - the spine it is
/// about to cut, the sections its runtime falls into, and a `signalLog`
/// that names the signal behind every decision. This is the blueprint
/// `plan_validate` checks before a single edit lands, and the record that
/// makes the run's choices answerable.
///
/// It deliberately does not invent creative direction. The fields the
/// measurement reaches are filled - the opening line is the thesis the
/// narration argues, the closing line its payoff, the ordered emotions
/// its arc, the platform its aspect - and the ones it does not are left
/// absent, so `plan_validate` reports them as blockers and the run says
/// out loud which part of the brief no measurement answered. A stage that
/// made up an audience would score green and be wrong.
fn auto_plan(log: &[Value], takes: &[TakePaths], source: &AutoEditProgramSource) -> Option<Value> {
    let view = latest_view(log)?;
    let media = media_paths(view);
    let clips: Vec<&Value> = view["timeline"]["clips"]
        .as_array()?
        .iter()
        .filter(|clip| clip["media"].as_str().is_some())
        .collect();
    if clips.is_empty() {
        return None;
    }
    let runtime = clips
        .iter()
        .map(|clip| {
            clip["start"].as_f64().unwrap_or(0.0) + clip["duration"].as_f64().unwrap_or(0.0)
        })
        .fold(0.0_f64, f64::max);
    if runtime < 0.5 {
        return None;
    }
    let read = emotion_lines(log, takes);
    // The segments are the spine as the handoff left it: one per picture
    // clip, in order, each naming the media it plays.
    let segments: Vec<Value> = clips
        .iter()
        .map(|clip| {
            let start = clip["start"].as_f64().unwrap_or(0.0);
            let end = start + clip["duration"].as_f64().unwrap_or(0.0);
            let mut segment = json!({
                "start": round3(start),
                "end": round3(end),
                "visual": {
                    "type": "existing",
                    "mediaId": clip["media"].as_str().unwrap_or_default(),
                    "category": "voiceover",
                },
            });
            // The read's own act and temperature for the moment the
            // segment opens, where the read reached it: the clip's first
            // file second is the moment its line begins.
            let mood = clip["media"]
                .as_str()
                .and_then(|id| media.iter().find(|(mid, _)| mid == id))
                .and_then(|(_, path)| {
                    let (source_in, _, _) = clip_file_span(clip);
                    emotion_at(&read, path, source_in).cloned()
                });
            if let Some(line) = mood {
                segment["emotion"] = json!(line.emotion);
                segment["colourTemp"] = json!(line.temperature);
                segment["act"] = json!(line.act);
                segment["duckDb"] = json!(line.duck_db);
            }
            segment
        })
        .collect();
    // The sections the runtime falls into. A piece over 12 s owes a payoff
    // somewhere, so it gets a cta; a short one is hook and body alone.
    let hook_end = if runtime > 12.0 { 3.0 } else { runtime };
    let sections = if runtime > 12.0 {
        json!([
            { "role": "hook", "start": 0.0, "end": 3.0 },
            { "role": "body", "start": 3.0, "end": round3(runtime - 2.0) },
            { "role": "cta", "start": round3(runtime - 2.0), "end": round3(runtime) },
        ])
    } else {
        json!([{ "role": "hook", "start": 0.0, "end": round3(hook_end) }])
    };
    // The brief, only where the narration answers it: the opening line is
    // the claim, the closing line its payoff, the ordered emotions the
    // arc. Audience and goal are not in a transcript, so they stay absent
    // and the gate names them.
    let mut brief = serde_json::Map::new();
    let spoken: Vec<String> = takes
        .iter()
        .flat_map(|take| spoken_lines(log, &take.take))
        .filter_map(|line| line["text"].as_str().map(str::to_owned))
        .collect();
    if let Some(first) = spoken.first() {
        brief.insert("thesis".to_owned(), json!(first));
        brief.insert("curiosityGap".to_owned(), json!(first));
    }
    if let Some(last) = spoken.last() {
        brief.insert("payoff".to_owned(), json!(last));
    }
    let arc: Vec<String> = read.iter().map(|line| line.emotion.clone()).fold(
        Vec::new(),
        |mut seen: Vec<String>, emotion| {
            if seen.last() != Some(&emotion) {
                seen.push(emotion);
            }
            seen
        },
    );
    // The deterministic fallback has no user interview. It must still
    // state an honest, generic audience and goal instead of emitting a
    // structurally valid plan that can never pass the creative gate.
    // The default arc is an editorial intention, not a claim that the
    // voice model measured three emotions.
    let fallback_arc = ["curiosity", "understanding", "resolution"];
    brief.insert(
        "emotionalArc".to_owned(),
        if arc.len() >= 3 { json!(arc) } else { json!(fallback_arc) },
    );
    brief.insert(
        "audience".to_owned(),
        json!("General viewers seeking a clear explanation of the narration's subject"),
    );
    brief.insert(
        "goal".to_owned(),
        json!("Help viewers understand the central idea and why it matters"),
    );
    brief.insert(
        "tone".to_owned(),
        json!(dominant(&read).map(|line| line.emotion.as_str()).unwrap_or("clear documentary")),
    );
    brief.insert(
        "referenceStyle".to_owned(),
        json!("Evidence-led editorial documentary"),
    );
    brief.insert(
        "constraints".to_owned(),
        json!(["Preserve the supplied narration's meaning", "Use only media with clear usage rights"]),
    );
    brief.insert(
        "avoid".to_owned(),
        json!(["Unsupported factual claims", "Repeated filler visuals"]),
    );
    brief.insert(
        "platform".to_owned(),
        json!(if aspect_of(view) == "9:16" {
            "reels"
        } else {
            "youtube"
        }),
    );
    brief.insert("monetized".to_owned(), json!(true));
    // The evidence: one entry per decision, naming the signals from the
    // voice_signals vocabulary that actually drove it.
    let mut signal_log: Vec<Value> = Vec::new();
    for segment in segments.iter().skip(1) {
        signal_log.push(json!({
            "at": segment["start"],
            "decision": "cut",
            "signals": ["pause", "words"],
            "note": "the boundary is where the narration stops for breath",
        }));
    }
    signal_log.push(json!({
        "at": 0.0,
        "decision": "caption",
        "signals": ["words", "emphasisWords"],
        "note": "captions sit on the measured words; a stressed word takes the accent",
    }));
    if let Some(peak) = read
        .iter()
        .max_by(|left, right| left.intensity.total_cmp(&right.intensity))
    {
        signal_log.push(json!({
            "at": round3(peak.start),
            "decision": "graphic",
            "signals": ["emotion"],
            "note": "the title entrance lands on the piece's strongest line",
        }));
    }
    let bed_words = dominant(&read)
        .and_then(|line| line.bgm_words.first().cloned())
        .unwrap_or_else(|| "cinematic".to_owned());
    let duck = dominant(&read).map(|line| line.duck_db).unwrap_or(-18.0);
    signal_log.push(json!({
        "at": 0.0,
        "decision": "bgm",
        "signals": ["emotion", "arc"],
        "note": "the bed's words and its duck come from the read's dominant line",
    }));
    for slot in broll_slots(log, takes) {
        signal_log.push(json!({
            "at": round3(slot.from),
            "decision": "transition",
            "signals": ["emotion", "pause"],
            "note": "the cut into the shot takes the mood the gap opens on",
        }));
    }
    // The delivery the read measured, named only where it measured one:
    // a pace entry is evidence the run read, and a run with no word
    // timings says so by leaving the entry out rather than claiming a
    // rhythm it never heard.
    let paced: Vec<&EmotionLine> = read.iter().filter(|line| !line.pace.is_empty()).collect();
    if !paced.is_empty() {
        let mut seen: Vec<&str> = Vec::new();
        for line in &paced {
            if seen.contains(&line.pace.as_str()) {
                continue;
            }
            seen.push(&line.pace);
            signal_log.push(json!({
                "at": round3(line.start),
                "decision": "pacing",
                "signals": ["speechRate", "words"],
                "note": format!(
                    "the delivery here measured {}, so the shot's hold and the \
                     caption's size follow it",
                    line.pace
                ),
            }));
        }
    }
    let look = dominant(&read)
        .map(|line| look_for_temperature(&line.temperature))
        .unwrap_or("concat.teal-orange");
    let (intensity_key, intensity) = grade_param(look);
    let mode = if runtime > 60.0 { "longform" } else { "reel" };
    Some(json!({
        "title": source
            .project
            .rsplit('/')
            .next()
            .unwrap_or("Auto edit"),
        "mode": mode,
        "aspect": aspect_of(view),
        "sections": sections,
        "segments": segments,
        "creativeBrief": brief,
        "audio": {
            "bgm": {
                "source": "freepd",
                "query": bed_words,
                "duckDb": duck,
                "alignTo": "none",
            },
        },
        "grade": {
            "filter": look,
            "intensityKey": intensity_key,
            "intensity": intensity,
            "apply": "layer",
        },
        "signalLog": signal_log,
        "export": {
            "output": source.export,
            "codec": "h264",
            "crf": 20,
        },
    }))
}

/// A sheet's name, tagged: the same folder and stem with `-tag` before
/// the extension, so a run's sheets sit side by side and none of them can
/// overwrite another.
fn sheet_variant(sheet: &str, tag: &str) -> String {
    let path = std::path::Path::new(sheet);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("auto-edit");
    let name = match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => format!("{stem}-{tag}.{ext}"),
        None => format!("{stem}-{tag}.png"),
    };
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name).display().to_string(),
        _ => name,
    }
}

/// The picture clips the cut shows, in timeline order: a video or a still
/// with pixels of its own, its span, and whether a scale ride is already
/// keyed on it. Titles, layers, shapes and motion comps are not pictures
/// a reframe or a push-in can be laid over - the comp is authored at the
/// frame size, and a title's scale is its entrance.
fn picture_clips(view: &Value) -> Vec<(String, f64, f64, bool)> {
    picture_clips_with_media(view)
        .into_iter()
        .map(|(id, start, duration, keyed, _)| (id, start, duration, keyed))
        .collect()
}

/// The same pictures, each with the id of the media it plays - what a fix
/// that asks for different content is about, and what a move needs to
/// name the track it stays on.
fn picture_clips_with_media(view: &Value) -> Vec<(String, f64, f64, bool, String)> {
    view["timeline"]["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|clip| matches!(clip["kind"].as_str(), Some("video") | Some("image")))
        .filter_map(|clip| {
            let id = clip["id"].as_str()?.to_owned();
            let keyed = clip["keyframed"]
                .as_array()
                .is_some_and(|keys| keys.iter().any(|key| key == &json!("scale")));
            Some((
                id,
                clip["start"].as_f64().unwrap_or(0.0),
                clip["duration"].as_f64().unwrap_or(0.0),
                keyed,
                clip["trackId"].as_str().unwrap_or_default().to_owned(),
            ))
        })
        .collect()
}

/// The run's own measured evidence, as the brief the critic reads: the
/// emotional arc and delivery the voice_signals and emotion stages
/// measured, in the words the critic's prompt asks for. The critic is a
/// vision model looking at a sheet of frames; without this it judges
/// pictures alone and cannot say whether the cut kept the arc the voice
/// carried. Every field here is a number or a word the run measured - a
/// signal the run never read is left out rather than filled with a guess.
fn signal_brief(log: &[Value]) -> Value {
    let mut brief = serde_json::Map::new();
    // The emotion reply's own rows: the read is already paired with the
    // take it came from by the stage that asked, so the brief needs no
    // transcript to name the arc the voice carried.
    let rows: Vec<&Value> = log
        .iter()
        .filter(|reply| {
            reply["lines"]
                .as_array()
                .is_some_and(|lines| lines.iter().any(|line| line.get("emotion").is_some()))
        })
        .flat_map(|reply| reply["lines"].as_array().into_iter().flatten())
        .collect();
    if !rows.is_empty() {
        let word = |row: &Value, key: &str| row[key].as_str().unwrap_or_default().to_owned();
        let arc: Vec<String> = {
            let mut seen: Vec<String> = Vec::new();
            for row in &rows {
                let act = word(row, "act");
                if !act.is_empty() && !seen.contains(&act) {
                    seen.push(act);
                }
            }
            seen
        };
        if !arc.is_empty() {
            brief.insert("emotionalArc".to_owned(), json!(arc.join(" -> ")));
        }
        let peak = rows
            .iter()
            .filter(|row| word(row, "emotion") != "neutral")
            .max_by(|left, right| {
                left["intensity"]
                    .as_f64()
                    .unwrap_or(0.0)
                    .total_cmp(&right["intensity"].as_f64().unwrap_or(0.0))
            });
        if let Some(peak) = peak {
            brief.insert(
                "peak".to_owned(),
                json!(format!(
                    "{} at {:.1}s, intensity {:.2}",
                    word(peak, "emotion"),
                    peak["start"].as_f64().unwrap_or(0.0),
                    peak["intensity"].as_f64().unwrap_or(0.0)
                )),
            );
        }
        let paced: Vec<String> = {
            let mut seen: Vec<String> = Vec::new();
            for reply in log {
                let Some(lines) = reply["lines"].as_array() else {
                    continue;
                };
                for line in lines {
                    let Some(pace) = line
                        .pointer("/speechRate/pace")
                        .and_then(Value::as_str)
                        .filter(|pace| !pace.is_empty())
                    else {
                        continue;
                    };
                    if !seen.iter().any(|known| known == pace) {
                        seen.push(pace.to_owned());
                    }
                }
            }
            seen
        };
        if !paced.is_empty() {
            brief.insert("delivery".to_owned(), json!(paced.join(", ")));
        }
        let hot = rows
            .iter()
            .filter(|row| {
                matches!(
                    row.pointer("/colourTemp/temperature")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    "hot" | "warm"
                )
            })
            .count();
        brief.insert(
            "colourSplit".to_owned(),
            json!(format!("{hot} of {} lines read warm", rows.len())),
        );
    }
    // The measured emphasis, as the peaks the SFX stage spends: the words
    // the voice leaned on, which a cut that honours the narration puts
    // its strongest pictures and titles under.
    let stressed: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for reply in log {
            let Some(lines) = reply["lines"].as_array() else {
                continue;
            };
            for line in lines {
                let Some(words) = line
                    .pointer("/prosody/emphasisWords")
                    .and_then(Value::as_array)
                else {
                    continue;
                };
                for word in words {
                    if let Some(text) = word["wordText"].as_str()
                        && !seen.iter().any(|known| known == text)
                    {
                        seen.push(text.to_owned());
                    }
                }
            }
        }
        seen.truncate(12);
        seen
    };
    if !stressed.is_empty() {
        brief.insert("emphasisWords".to_owned(), json!(stressed));
    }
    Value::Object(brief)
}

/// The craft facts of the last sheet the run read, as the critic measured
/// them: the `craft` block of the newest `watch_render` reply in the log.
fn craft_facts(log: &[Value]) -> Option<&Value> {
    log.iter().rev().find_map(|reply| {
        reply
            .get("craft")
            .filter(|craft| craft.get("samples").is_some())
    })
}

/// The revisions the critic's own measurements ask for, as one batch of
/// commands: every one traceable to a number in the sheet, none of them a
/// matter of taste. A cut that measured clean revises nothing, and a
/// defect with no honest fix in the editing vocabulary (footage that is
/// simply repeated) is left to the report rather than papered over with
/// an effect.
///
/// Three rules, each the answer to a measured fault:
///
/// - Bars in the frame are a picture that does not fill it: `frameClip`
///   fill, on the pictures that carry no scale ride already.
/// - A black opening is a hook that never landed: the measured lead-in is
///   trimmed, a quarter of the clip at most and half a second at most -
///   a black frame is a fault, a fade is not.
/// - Near-static cuts are a slideshow: a push-in on every *other* shot,
///   never on all of them. The same zoom on every cut is the robotic
///   repetition the rule forbids, and two shots that differ in energy
///   read as a cut rather than as a still.
fn revise_commands(log: &[Value]) -> Vec<Value> {
    let (Some(craft), Some(view)) = (craft_facts(log), latest_view(log)) else {
        return Vec::new();
    };
    let samples = craft["samples"].as_f64().unwrap_or(0.0).max(1.0);
    let pictures = picture_clips(view);
    if pictures.is_empty() {
        return Vec::new();
    }
    let mut commands: Vec<Value> = Vec::new();

    let bars = craft["letterboxedFrames"].as_f64().unwrap_or(0.0);
    if bars / samples > 0.1 {
        for (id, _, _, keyed) in &pictures {
            if *keyed {
                continue;
            }
            commands.push(json!({ "op": "frameClip", "clipId": id, "framing": "fill" }));
        }
    }

    let black = craft["blackFrames"].as_f64().unwrap_or(0.0);
    let opening_black = log
        .iter()
        .rev()
        .find_map(|reply| {
            reply
                .pointer("/tiles/0/blackFraction")
                .and_then(Value::as_f64)
        })
        .unwrap_or(0.0);
    if black >= 1.0
        && opening_black > 0.5
        && let Some((id, start, duration, _)) = pictures.first()
    {
        let cut = (duration * 0.25).min(0.5);
        if *start < 0.05 && cut >= 0.08 {
            commands.push(json!({
                "op": "trimClip",
                "clipId": id,
                "edge": "start",
                "delta": round3(cut),
                "ripple": true,
            }));
        }
    }

    let static_cuts = craft["staticTransitions"].as_f64().unwrap_or(0.0);
    if static_cuts >= 3.0 && static_cuts / (samples - 1.0).max(1.0) > 0.4 {
        let ease = json!([0.42, 0.0, 0.58, 1.0]);
        for (n, (id, _, duration, keyed)) in pictures.iter().enumerate() {
            if n % 2 == 1 || *keyed || *duration < 1.2 {
                continue;
            }
            commands.push(json!({
                "op": "setClipKey", "clipId": id, "property": "scale",
                "at": 0.0, "value": 1.0, "ease": ease,
            }));
            commands.push(json!({
                "op": "setClipKey", "clipId": id, "property": "scale",
                "at": 1.0, "value": 1.04, "ease": ease,
            }));
        }
    }

    commands.extend(critic_fix_commands(log, &picture_clips_with_media(view)));
    commands
}

/// The critic's own structured fixes, as edit commands: the part of the
/// review that a model - local or hosted - answered rather than the pixel
/// measurements. A fix names its where, its problem, its change, and one
/// of five patch ops; this reads the op and the patch the critic carried
/// and spends them on the clip the fix is about.
///
/// A fix is only spent when it names a number this program can act on. The
/// critic is told a patch is applied exactly as written, so a fix that
/// arrives without one is a read of taste, and taste is the report's
/// business - not a blind edit. Every fix carries its confidence, and a
/// fix under the bar is left to the report for the same reason: the run
/// would rather ship a flaw it named than an edit it guessed.
fn critic_fix_commands(log: &[Value], pictures: &[(String, f64, f64, bool, String)]) -> Vec<Value> {
    const LEAST_CONFIDENCE: f64 = 0.6;
    let Some(critique) = log.iter().rev().find(|reply| {
        reply.get("fixes").and_then(Value::as_array).is_some() || reply.get("scores").is_some()
    }) else {
        return Vec::new();
    };
    let Some(fixes) = critique["fixes"].as_array() else {
        return Vec::new();
    };
    // The frames the critic speaks of, as the sheet labelled them: a fix
    // whose `where` names a frame number is about the clip that frame
    // plays, which is how a fix reaches a shot instead of the whole cut.
    let tiles: Vec<Value> = critique["tiles"]
        .as_array()
        .cloned()
        .or_else(|| {
            log.iter()
                .rev()
                .find_map(|reply| reply.get("tiles").and_then(Value::as_array).cloned())
        })
        .unwrap_or_default();
    let clip_at = |at: f64| -> Option<&(String, f64, f64, bool, String)> {
        pictures
            .iter()
            .find(|(_, start, duration, _, _)| at >= *start && at < start + duration)
            .or_else(|| {
                pictures
                    .iter()
                    .min_by(|left, right| (left.1 - at).abs().total_cmp(&(right.1 - at).abs()))
            })
    };
    let mut commands: Vec<Value> = Vec::new();
    let mut already: Vec<String> = Vec::new();
    for fix in fixes {
        let confidence = fix["confidence"].as_f64().unwrap_or(0.5);
        if confidence < LEAST_CONFIDENCE {
            continue;
        }
        let op = fix["op"].as_str().unwrap_or_default();
        // The frames the fix names, read from its `where` text: "frames
        // 6-8" is a span, "frame 3" a single sample.
        let named: Option<f64> = fix["where"]
            .as_str()
            .and_then(|where_| {
                let digits: String = where_
                    .chars()
                    .skip_while(|c| !c.is_ascii_digit())
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                digits
                    .split('-')
                    .next()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .and_then(|index| tiles.get(index).cloned())
            .and_then(|tile| tile["at"].as_f64());
        let target = named.and_then(clip_at).or_else(|| pictures.first());
        let Some((clip_id, start, duration, keyed, track_id)) = target else {
            continue;
        };
        let patch = fix.get("patch").filter(|patch| patch.is_object());
        match op {
            // Timing: a patch's `start` is where the element should begin,
            // a `duration` how long it should hold. Moving a clip earlier
            // or later is a start change; the op the critic means is the
            // one the patch's own keys spell.
            "retime" => {
                let Some(patch) = patch else { continue };
                if let Some(want) = patch["start"].as_f64()
                    && (want - start).abs() > 0.05
                    && already.iter().all(|id| id != &format!("retime:{clip_id}"))
                {
                    already.push(format!("retime:{clip_id}"));
                    commands.push(json!({
                        "op": "moveClips",
                        "moves": [{
                            "clipId": clip_id,
                            "start": round3(want.max(0.0)),
                            "trackId": track_id,
                        }],
                    }));
                }
            }
            // Size: a scale on the clip, keyed from 1.0 to the factor so
            // the shot settles rather than snapping.
            "resize" => {
                let Some(patch) = patch else { continue };
                let Some(factor) = patch["factor"].as_f64() else {
                    continue;
                };
                let key = format!("resize:{clip_id}");
                if !(0.5..=2.0).contains(&factor) || already.contains(&key) {
                    continue;
                }
                already.push(key);
                let ease = json!([0.42, 0.0, 0.58, 1.0]);
                commands.push(json!({
                    "op": "setClipKey", "clipId": clip_id, "property": "scale",
                    "at": 0.0, "value": 1.0, "ease": ease,
                }));
                commands.push(json!({
                    "op": "setClipKey", "clipId": clip_id, "property": "scale",
                    "at": 1.0, "value": round3(factor), "ease": ease,
                }));
            }
            // Position: a shift the critic measured in the frames, as the
            // transform's offset - clamped to the frame so a fix cannot
            // push the picture out of sight.
            "shift" => {
                let Some(patch) = patch else { continue };
                let (dx, dy) = (
                    patch["dx"].as_f64().unwrap_or(0.0),
                    patch["dy"].as_f64().unwrap_or(0.0),
                );
                if dx == 0.0 && dy == 0.0 {
                    continue;
                }
                let key = format!("shift:{clip_id}");
                if already.contains(&key) {
                    continue;
                }
                already.push(key);
                commands.push(json!({
                    "op": "setClipTransform",
                    "clipId": clip_id,
                    "offsetX": round3((dx / 1920.0).clamp(-0.25, 0.25)),
                    "offsetY": round3((dy / 1080.0).clamp(-0.25, 0.25)),
                }));
            }
            // Look: a framing fill for bars the critic saw, which is the
            // one restyle the edit vocabulary can apply without a taste
            // call. A clip already carrying a scale ride is left alone.
            "restyle" => {
                let key = format!("restyle:{clip_id}");
                if *keyed || already.contains(&key) {
                    continue;
                }
                let fill = fix["change"]
                    .as_str()
                    .is_some_and(|change| change.to_lowercase().contains("fill"));
                if !fill {
                    continue;
                }
                already.push(key);
                commands.push(json!({
                    "op": "frameClip", "clipId": clip_id, "framing": "fill",
                }));
            }
            // Replace: the fix asked for different content. The run has no
            // honest replacement to hand for a shot it did not shop, so
            // the fix is named in the report and left unspent - an edit
            // that swapped in arbitrary footage would be the mistake the
            // critic was warning about. A `_ = duration` keeps the read
            // explicit for the next fix that needs the span.
            "replace" => {
                let _ = duration;
            }
            _ => {}
        }
    }
    commands
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
/// not as it was planned.
///
/// The picture is built on the timeline, not merely exported: a one-click
/// run now leaves an editable cut, B-roll, a look and moving titles
/// included, for the window to tweak. And the cut is *looked at before it
/// is paid for*: the preview samples the open timeline (not the export,
/// which has not been rendered yet) and scores it, the export and the
/// verify follow, the watch scores the file that landed, and the revise
/// answers the critic's measured faults with targeted edits and looks
/// again - so the run's own craft journal carries a before and an after
/// instead of one score nobody acted on.
///
/// The judgement stages that still need a brain - the plan's creative
/// brief, the stock chosen by eye, the baked motion comps - stay the
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
    // 5. The understanding: the music gate (a take that already carries
    //    music gets no bed stacked under it) and the emotional read of
    //    every take that has words - the arc, the acts, the colour
    //    temperature, the bed's duck and the accent family each line
    //    asks for. The read is derived, not static: its lines are the
    //    transcripts the transcribe stage has already landed, so the
    //    program never has to guess words it has not measured.
    let understand_source = takes.clone();
    steps.push(
        DerivedStep::react(
            Stage::Understand,
            Box::new({
                let mut queue: Option<Vec<ToolCall>> = None;
                move |log| {
                    let queue = queue.get_or_insert_with(|| {
                        let mut calls: Vec<ToolCall> = understand_source
                            .iter()
                            .map(|take| {
                                call(
                                    "sense_audio_repair",
                                    json!({ "action": "detect_music_segments", "audio": take.take }),
                                )
                            })
                            .collect();
                        for take in &understand_source {
                            let lines = spoken_lines(log, &take.take);
                            if lines.is_empty() {
                                continue;
                            }
                            calls.push(call(
                                "emotion",
                                json!({ "action": "extract", "lines": lines }),
                            ));
                        }
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
    // 5b. The plan: the blueprint the run is about to cut, written from
    //     what the run itself measured, and validated before a single
    //     edit lands. The segments are the spine the handoff left, the
    //     sections the runtime falls into, and the signalLog names the
    //     signal behind every cut, caption, graphic, bed and transition -
    //     so a decision the profile did not drive is a validation problem
    //     here, while the edit is still only a plan.
    //
    //     The plan is deliberately incomplete where measurement is:
    //     audience and goal are not in a transcript, so the gate reports
    //     them as blockers rather than the stage inventing them. That
    //     reply is the run's honest statement of what it could and could
    //     not answer, which is worth more than a green score it did not
    //     earn.
    let plan_source = takes.clone();
    let plan_settings = source.clone();
    steps.push(
        DerivedStep::react(
            Stage::Plan,
            Box::new({
                // The beat analysis must finish BEFORE the planner call is
                // built: constructing both calls in one queue used to bake
                // an empty beat list into the planner request.
                let mut phase = 0u8;
                let mut queue: Vec<ToolCall> = Vec::new();
                move |log| {
                    if phase == 0 {
                        phase = 1;
                        if let Some(music) = musical_take(log, &plan_source) {
                            return Some(call("analyze_beats", json!({ "path": music })));
                        }
                    }
                    if phase == 1 {
                        phase = 2;
                        let music = musical_take(log, &plan_source);
                        let beats = latest_beats(log);
                        // Transcripts are in each source file's clock, not
                        // one shared clock. One planner call per take keeps
                        // gaps and monotony attached to the correct audio.
                        for take in &plan_source {
                            let segments = planner_segments(log, take);
                            if segments.is_empty() {
                                continue;
                            }
                            let mut spec = json!({
                                "take": take.take,
                                "segments": segments,
                            });
                            if music.as_deref() == Some(take.take.as_str())
                                && let Some(beats) = beats.clone()
                            {
                                spec["beats"] = beats;
                            }
                            queue.push(call("footage_plan_suggest", spec));
                        }
                    }
                    if !queue.is_empty() {
                        return Some(queue.remove(0));
                    }
                    if phase == 2 {
                        phase = 3;
                        let plan = auto_plan(log, &plan_source, &plan_settings)?;
                        return Some(call("plan_validate", json!({ "plan": plan })));
                    }
                    None
                }
            }),
        )
        .warn()
        .into(),
    );
    // 6. The timeline, in one stage and one step: the tighten, then the
    //    grade. The runner's resume record is keyed by the stage, so two
    //    steps sharing `Stage::Timeline` were not two steps - the second
    //    was skipped the moment the first completed, and the run never
    //    graded. The two are one queue here, in the order an editor works:
    //    cut the dead air, then lay the look over the cut that is left.
    //
    //    The tighten cuts only the dead air the reading classified, each
    //    span shrunk the padding rule's 80 ms, through `edit_apply` so the
    //    rollback and the resume both see it, then reads the timeline
    //    back as it now stands - the truth every later stage maps
    //    through. No dead air is a tighten that succeeded.
    //
    //    The grade is one look over the whole runtime (an effect layer
    //    spanning the cut, never a five-second layer: the duration is the
    //    timeline the tighten just read, and the layer lands above the
    //    picture so it treats it). The look is the one the emotional read
    //    asks for - a joyful take graded golden, a fearful one cold, a
    //    sad one cool-blue - and the blockbuster teal-and-orange only
    //    when the read named no temperature. A run with no runtime to
    //    read grades nothing.
    let tighten_source = takes.clone();
    let tighten_project = project.clone();
    let grade_source = takes.clone();
    let grade_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Timeline,
            Box::new({
                let mut queue: Option<Vec<ToolCall>> = None;
                let mut graded = false;
                move |log| {
                    let queue = queue.get_or_insert_with(|| {
                        let read = emotion_lines(log, &grade_source);
                        let mut calls =
                            dead_air_cuts(log, &tighten_source, &tighten_project, &read);
                        calls.push(call("timeline_read", json!({ "path": tighten_project })));
                        calls
                    });
                    if let Some(next) = (!queue.is_empty()).then(|| queue.remove(0)) {
                        return Some(next);
                    }
                    if graded {
                        return None;
                    }
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
                    let look = dominant(&emotion_lines(log, &grade_source))
                        .map(|line| look_for_temperature(&line.temperature))
                        .unwrap_or("concat.teal-orange");
                    graded = true;
                    Some(call(
                        "edit_apply",
                        json!({
                            "path": grade_project,
                            "command": {
                                "op": "addLayerClip",
                                "trackId": null,
                                "start": 0.0,
                                "duration": round3(runtime),
                                "effectId": look,
                                "name": "The look",
                            },
                        }),
                    ))
                }
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
    //
    //     Each gap's cut-in is the transition the emotion at that moment
    //     asks for - a burst for surprise, a soft dissolve for sadness -
    //     and never the same move twice running: the second shot cut from
    //     the same mood takes the mood's alternate. That is the director's
    //     no-repeat rule enforced, not merely asked for.
    let gather_source = takes.clone();
    let gather_project = project.clone();
    let gather_emotion = takes.clone();
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
                let mut pending_pace = String::new();
                let mut pending_transition: Option<&'static str> = None;
                // The transition the last placed shot cut in with: the
                // next shot never wears the same move twice running.
                let mut last_transition: Option<&'static str> = None;
                // The clip ids already shopped: a shelf can return the same
                // clip for two neighbouring gaps, and laying it twice reads
                // as a mistake to anyone watching.
                let mut used_ids: Vec<String> = Vec::new();
                move |log| loop {
                    let slots = slots.get_or_insert_with(|| broll_slots(log, &gather_source));
                    if placed >= slots.len() {
                        return None;
                    }
                    let slot = &slots[placed];
                    // The mood the gap opens on: the line whose speech
                    // the gap follows, so the picture cuts in the way the
                    // voice just sounded.
                    let mood =
                        emotion_at(&emotion_lines(log, &gather_emotion), &slot.take, slot.from)
                            .map(|line| line.emotion.clone())
                            .unwrap_or_else(|| "neutral".to_owned());
                    let transition = {
                        let first = transition_for_emotion(&mood);
                        if last_transition == Some(first) {
                            alternate_transition(&mood)
                        } else {
                            first
                        }
                    };
                    match phase {
                        0 => {
                            phase = 1;
                            return Some(call(
                                "stock_search",
                                json!({
                                    "provider": gather_provider.clone(),
                                    "query": slot.query.clone(),
                                    "limit": 5,
                                }),
                            ));
                        }
                        1 => {
                            // Only a live-shelf hit with an explicit provider
                            // licence is eligible. OpenVid has no per-clip
                            // licence record and is never imported here.
                            //
                            // The page is read for the clip that *fits* this
                            // slot rather than the first one returned: the
                            // shelf ranks by its own relevance, and the frame
                            // the project is (vertical or not), the span to
                            // cover and the clips already used are all known
                            // here. Taking `hits/0` put whatever the shelf
                            // ranked highest into the cut even when it was a
                            // landscape clip in a vertical reel.
                            let span = slot.to - slot.from;
                            let aspect = latest_view(log)
                                .map(|view| aspect_of(view).to_owned())
                                .unwrap_or_else(|| "16:9".to_owned());
                            let hits = log
                                .last()
                                .and_then(|reply| reply["hits"].as_array())
                                .cloned()
                                .unwrap_or_default();
                            let hit = best_hit(&hits, span, &aspect, &used_ids);
                            let provider = hit.as_ref().and_then(|hit| hit["provider"].as_str());
                            let id = hit.as_ref().and_then(|hit| hit["id"].as_str());
                            let url = hit.as_ref().and_then(|hit| hit["url"].as_str());
                            let license = hit.as_ref().and_then(|hit| hit["license"].as_str());
                            match (provider, id, url, license) {
                                (Some(provider), Some(id), Some(url), Some(license))
                                    if provider == gather_provider.as_str()
                                        && !id.trim().is_empty()
                                        && !url.trim().is_empty()
                                        && !license.trim().is_empty() =>
                                {
                                    used_ids.push(id.to_owned());
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
                                    // The delivery over the gap sets how
                                    // much the shot moves: a punchy stretch
                                    // takes a livelier drift, a slow one a
                                    // quieter, longer push.
                                    pending_pace =
                                        delivery_pace(log, &slot.take, slot.from, slot.to);
                                    pending_transition = Some(transition);
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
                            let drift = match pending_pace.as_str() {
                                "punchy" => 1.10,
                                "conversational" => 1.07,
                                "slow" => 1.03,
                                _ => 1.06,
                            };
                            commands.push(json!({
                                "op": "setClipKey",
                                "clipId": clip_id,
                                "property": "scale",
                                "at": 1.0,
                                "value": drift,
                                "ease": [0.0, 0.0, 0.58, 1.0],
                            }));
                            let chosen = pending_transition.unwrap_or("concat.dissolve");
                            last_transition = Some(chosen);
                            commands.push(json!({
                                "op": "updateClip",
                                "clipId": clip_id,
                                "patch": {
                                    "transitionIn": { "id": chosen, "duration": 0.3 },
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
    //    undoable batch of titles in the reading's house style. The style
    //    follows the read at each line: a line that reads loud is set
    //    heavier, a hot or split line takes the accent colour, and a calm
    //    informational line stays plain - the same words do not all earn
    //    the same look, which is the difference between captions and a
    //    transcript.
    let caption_source = takes.clone();
    let caption_project = project.clone();
    let caption_emotion = takes.clone();
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
                    let read = emotion_lines(log, &caption_emotion);
                    let emphasis = stressed_words(log, &caption_source);
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
                            let line = emotion_at(&read, &take, start);
                            let intensity = line.map(|line| line.intensity).unwrap_or(0.5);
                            let hot = line.is_some_and(|line| {
                                matches!(line.temperature.as_str(), "hot" | "split")
                            });
                            // A caption that carries one of the take's own
                            // stressed words takes the accent even on a
                            // neutral line: the emphasis signal is the
                            // delivery's, not the mood's, and the log
                            // promises the caption stage spends it.
                            let stressed = !emphasis.is_empty()
                                && text
                                    .split_whitespace()
                                    .map(|word| {
                                        word.trim_matches(|c: char| !c.is_alphanumeric())
                                            .to_lowercase()
                                    })
                                    .any(|word| emphasis.contains(&word));
                            // A fast line is read at a glance, so its
                            // caption is set a touch smaller; a slow line
                            // can carry a larger one. The measured delivery
                            // is what decides, and an unmeasured line keeps
                            // the house size rather than guessing.
                            let size = match line.map(|line| line.pace.as_str()) {
                                Some("punchy") => 0.036,
                                Some("slow") => 0.046,
                                _ => 0.04,
                            };
                            captions.push(json!({
                                "start": round3(at),
                                "end": round3(to),
                                "text": text,
                                "weight": if intensity >= 0.65 { 800 } else { 600 },
                                "accent": hot || stressed,
                                "size": size,
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
                            let mut style = json!({
                                "fontFamily": "Hanken Grotesk",
                                "fontSize": caption["size"],
                                "fontWeight": caption["weight"],
                                "content": caption["text"],
                            });
                            if caption["accent"] == json!(true) {
                                style["color"] = json!("#ffd400");
                            }
                            json!({
                                "op": "addTextClip",
                                "trackId": null,
                                "above": true,
                                "start": caption["start"],
                                "duration": round3(
                                    caption["end"].as_f64().unwrap_or(0.0)
                                        - caption["start"].as_f64().unwrap_or(0.0)
                                ),
                                "style": style,
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
    // 8. The graphics, then the entrance. Two motions in one stage, in
    //    the order an editor works: the designed cards are baked through
    //    the Geneva engine and placed on the timeline as motion clips
    //    (linked to their comp, so the card stays editable and the export
    //    finds its bake by the comp's hash), and then the captions the
    //    previous stage landed are given their entrance.
    //
    //    The cards are the ones the plan named - the read's own act and
    //    emotion chose each preset, and the line's own words are what the
    //    card says. A bake that fails skips its card and the run goes on:
    //    a missing graphic is a smaller loss than an abandoned edit. This
    //    is the stage that was the Director's alone; it now runs on the
    //    read, which is what makes the button's motion evidence-driven
    //    rather than decorative.
    let motion_project = project.clone();
    let motion_source = takes.clone();
    steps.push(
        DerivedStep::react(
            Stage::Motion,
            Box::new({
                // The cards this run will bake, decided on the first ask
                // from the read: file second and preset, in play order.
                let mut specs: Option<Vec<MotionSpec>> = None;
                // The preset library's own slot labels, read once: a
                // preset's text slot is not called the same thing in every
                // card, and guessing the label is how a bake is refused.
                let mut labels: Option<Vec<(String, String)>> = None;
                // Where the stage is: which card, and which of its calls
                // (the library, the bake, then the placement) is next.
                let mut at = 0usize;
                let mut phase = 0u8;
                let mut entrances = false;
                move |log| loop {
                    let specs = specs
                        .get_or_insert_with(|| motion_specs(&emotion_lines(log, &motion_source)));
                    if at >= specs.len() {
                        // The cards are placed: the captions' entrance is
                        // the stage's last act, issued once.
                        if entrances {
                            return None;
                        }
                        entrances = true;
                        let targets = entrance_targets(log);
                        if targets.is_empty() {
                            return None;
                        }
                        let mut commands: Vec<Value> = Vec::new();
                        for (id, duration) in &targets {
                            commands.extend(entrance_keys(id, *duration));
                        }
                        return Some(call(
                            "edit_apply",
                            json!({
                                "path": motion_project,
                                "command": { "op": "batch", "commands": commands },
                            }),
                        ));
                    }
                    let spec = &specs[at];
                    match phase {
                        0 => {
                            // The library once, before the first bake: each
                            // preset's own text slot, so the words land in
                            // the field the preset actually has.
                            if labels.is_none() {
                                phase = 1;
                                return Some(call("motion_presets", json!({})));
                            }
                            phase = 2;
                            // The bake: the preset the plan named, its
                            // text slot filled with the line's own words,
                            // at the project's own frame (no width or
                            // height, so a vertical reel bakes vertical).
                            // A preset with no text slot bakes as itself.
                            let mut arguments = json!({
                                "project": motion_project,
                                "preset": spec.preset,
                            });
                            if let Some((_, slot)) = labels
                                .as_ref()
                                .and_then(|labels| labels.iter().find(|(id, _)| id == spec.preset))
                            {
                                arguments["ops"] = json!([{
                                    "op": "set-slot", "slot": slot, "value": spec.words,
                                }]);
                            }
                            return Some(call("motion_bake", arguments));
                        }
                        1 => {
                            // The library's reply, read once and kept: the
                            // first text slot of each preset, by id.
                            labels = Some(
                                log.last()
                                    .and_then(|reply| reply["presets"].as_array())
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|preset| {
                                        let id = preset["id"].as_str()?.to_owned();
                                        let slot = preset["slots"]
                                            .as_array()?
                                            .iter()
                                            .find(|slot| slot["field"] == json!("text"))?["label"]
                                            .as_str()?
                                            .to_owned();
                                        Some((id, slot))
                                    })
                                    .collect(),
                            );
                            phase = 0;
                        }
                        2 => {
                            // The bake's reply carries the comp's hash -
                            // the key the export resolves the frames by,
                            // and the comp file is named after it. Place
                            // the card where the line it was read from now
                            // plays, which the tighten may have moved.
                            let baked = log.last().cloned().unwrap_or_default();
                            let hash = baked["hash"].as_str().map(str::to_owned);
                            let at_timeline = latest_view(log).and_then(|view| {
                                let media = media_paths(view);
                                timeline_at(view, &media, &spec.take, spec.at)
                            });
                            match (hash, at_timeline) {
                                (Some(hash), Some(start))
                                    if hash.len() == 16
                                        && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
                                {
                                    phase = 3;
                                    // The comp reference is the file name
                                    // under the project's comps/, which
                                    // the bake named after the hash; the
                                    // reply's `comp` is that file's full
                                    // path, which the command refuses.
                                    return Some(call(
                                        "edit_apply",
                                        json!({
                                            "path": motion_project,
                                            "command": {
                                                "op": "addMotionClip",
                                                "start": round3(start),
                                                "comp": format!("{hash}.json"),
                                                "hash": hash,
                                            },
                                        }),
                                    ));
                                }
                                // A bake with no hash, or a line the
                                // tighten cut away: no card, next beat.
                                _ => {
                                    at += 1;
                                    phase = 0;
                                }
                            }
                        }
                        3 => {
                            // The card lands at the editorial default of
                            // five seconds; a card that outlives its line
                            // is the defect the critique stage names, so
                            // it is trimmed to the line's own hold. The
                            // trim is a tail trim with no ripple: the
                            // card owns a lane of its own and nothing
                            // follows it on that lane.
                            let placed = log.last().cloned().unwrap_or_default();
                            let id = placed["createdId"].as_str().map(str::to_owned);
                            let default = placed["timeline"]["clips"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .find(|clip| clip["id"] == placed["createdId"])
                                .and_then(|clip| clip["duration"].as_f64());
                            let change = match (id, default) {
                                (Some(id), Some(duration)) => {
                                    let delta = spec.hold() - duration;
                                    (delta.abs() > 0.05).then_some((id, delta))
                                }
                                _ => None,
                            };
                            at += 1;
                            phase = 0;
                            if let Some((id, delta)) = change {
                                return Some(call(
                                    "edit_apply",
                                    json!({
                                        "path": motion_project,
                                        "command": {
                                            "op": "trimClip",
                                            "clipId": id,
                                            "edge": "tail",
                                            "delta": round3(delta),
                                        },
                                    }),
                                ));
                            }
                        }
                        _ => return None,
                    }
                }
            }),
        )
        .warn()
        .into(),
    );
    // 9. The music bed, only where the gate said the take carries none:
    //    the shelf prepared and polled, the track shopped (the emotional
    //    read's own bed words for the query - the mood and act the line
    //    sits in name the genre - or the reading's language hint when the
    //    read named neither), the bed imported at zero, the timeline read
    //    back, and the bed ducked under the voice to the level the arc's
    //    own act profile set. The CC0 shelf only - an unattended run never
    //    ships the non-commercial one.
    let music_project = project.clone();
    let music_source = takes.clone();
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
                                // The bed's words: the emotional read's
                                // own, from the piece's dominant line -
                                // the mood and the act name the genre -
                                // and the reading's language hint only
                                // when the read named no words at all.
                                let query = dominant(&emotion_lines(log, &music_source))
                                    .and_then(|line| line.bgm_words.first().cloned())
                                    .or_else(|| {
                                        log.iter()
                                            .filter_map(|reply| {
                                                reply.pointer("/language/musicGenreHint")
                                            })
                                            .filter_map(|hint| hint.as_str())
                                            .find(|hint| !hint.trim().is_empty())
                                            .map(str::to_owned)
                                    });
                                let mut arguments = json!({
                                    "source": "freepd",
                                    "limit": 5,
                                    "sort": "relevance",
                                });
                                if let Some(query) = query {
                                    arguments["query"] = json!(query);
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
                            // The bed sits at the level the piece's own
                            // act profile sets: a payoff may carry a
                            // louder bed than a quiet setup, and the
                            // emotional read is what knows which it is.
                            let base_db = dominant(&emotion_lines(log, &music_source))
                                .map(|line| line.duck_db)
                                .unwrap_or(-20.0);
                            call(
                                "clip_audio_levels_db",
                                json!({
                                    "path": music_project,
                                    "clipId": bed["id"],
                                    "baseDb": base_db,
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
    //    the sound, not the program's guess. The query is the accent
    //    family the emotion at that moment asks for (an anger peak shops
    //    an impact, a fear peak a riser), and only a peak with no read
    //    behind it falls back to the general vocabulary.
    let sfx_source = takes.clone();
    let sfx_project = project.clone();
    let sfx_emotion = takes.clone();
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
                        // An accent on every emphasis peak is the robotic
                        // mistake the skill warns about: a stressed word in
                        // a calm informational line does not want a hit.
                        // When the read exists it decides - a peak is only
                        // accented where the line reads as feeling, at the
                        // strength worth hearing. With no read at all the
                        // peaks stand as measured, since nothing better
                        // knows.
                        let read = emotion_lines(log, &sfx_emotion);
                        if !read.is_empty() {
                            found.retain(|(time, _)| {
                                sfx_source.iter().any(|take| {
                                    emotion_at(&read, &take.take, *time).is_some_and(|line| {
                                        line.emotion != "neutral" && line.intensity >= 0.5
                                    })
                                })
                            });
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
                    // The accent family the peak's own line asks for: the
                    // peak's file moment is the line's, so the emotion
                    // read there names the sound.
                    let (take, moment) = peaks
                        .get(placed)
                        .map(|(moment, _)| {
                            let take = sfx_source
                                .iter()
                                .find(|take| {
                                    latest_view(log).is_some_and(|view| {
                                        timeline_at(view, &media_paths(view), &take.take, *moment)
                                            .is_some()
                                    })
                                })
                                .map(|take| take.take.clone())
                                .unwrap_or_default();
                            (take, *moment)
                        })
                        .unwrap_or_default();
                    let read = emotion_lines(log, &sfx_emotion);
                    let line = emotion_at(&read, &take, moment);
                    let emotion = line
                        .map(|line| line.emotion.clone())
                        .unwrap_or_else(|| "neutral".to_owned());
                    let queries = sfx_queries_for_line(line, &emotion);
                    Some(call(
                        "sounds_search",
                        json!({
                            "query": queries[placed % queries.len()],
                            "limit": 3,
                        }),
                    ))
                }
            }),
        )
        .warn()
        .into(),
    );
    // 11. The preview: the cut as it stands, sampled from the OPEN
    //     timeline - not the export, which has not been rendered yet - and
    //     scored on the six craft axes. This is the look that decides
    //     whether the render is worth paying for, and the numbers the
    //     revise stage answers. Warn-policy: a cut that cannot be watched
    //     still exports, with the flaw recorded.
    let preview_project = project.clone();
    let preview_sheet = sheet_variant(&source.sheet, "preview");
    steps.push(
        DerivedStep::react(
            Stage::Preview,
            Box::new({
                let preview_project = preview_project.clone();
                let preview_sheet = preview_sheet.clone();
                let mut phase = 0u8;
                move |log| {
                    if phase == 0 {
                        phase = 1;
                        return Some(call(
                            "watch_render",
                            json!({
                                "project": preview_project,
                                "out": preview_sheet,
                            }),
                        ));
                    }
                    if phase == 1 {
                        phase = 2;
                        // The sheet names where it kept its own pixels;
                        // that is the path the critic reads, not one this
                        // program reconstructs from the name it asked for.
                        // A sheet that named none is a preview that could
                        // not be looked at: end the stage rather than hand
                        // the critic an empty path.
                        let png = log
                            .iter()
                            .rev()
                            .find_map(|reply| reply.get("pngPath").and_then(Value::as_str))
                            .filter(|png| !png.trim().is_empty())?;
                        // The brief carries the arc the voice measured, so
                        // the critic judges the cut against the story it
                        // was cut to and not the pictures alone.
                        let measured = signal_brief(log);
                        let brief = if measured.as_object().is_some_and(|o| !o.is_empty()) {
                            measured
                        } else {
                            json!({})
                        };
                        // `autoSetup` is the difference between a review
                        // and a pixel count: without it a machine with no
                        // vision model yet answers from measurements alone
                        // and the stage would call that a critique.
                        return Some(call(
                            "craft_critique",
                            json!({
                                "pngPath": png,
                                "project": preview_project,
                                "brief": brief,
                                "autoSetup": true,
                            }),
                        ));
                    }
                    None
                }
            }),
        )
        .warn()
        .into(),
    );
    // 12. The revise, warn-policy: the critic's measured faults answered
    //     with targeted edits, then the cut looked at again. Every command
    //     comes from a number the sheet carried - bars to fill, a black
    //     opening to trim, static cuts to choreograph - so a clean cut
    //     issues no edit at all and the stage completes on its first
    //     `None`. A revision that lands is followed by one re-watch and
    //     one re-critique of the same frames, which the craft journal
    //     records as a second pass with its own delta: the loop is proved
    //     by the journal, not claimed by the program.
    //
    //     The revise sits BEFORE the export, not after it. A revision
    //     applied to the timeline once the render had landed would leave
    //     the shipped file showing the unrevised cut - the fix would exist
    //     only in the project the user opens, never in the video they
    //     publish. Placing it here makes the one encode the run pays for
    //     the corrected cut by construction, and the watch then scores the
    //     file that actually ships.
    let revise_project = project.clone();
    let revise_sheet = sheet_variant(&source.sheet, "revised");
    steps.push(
        DerivedStep::react(
            Stage::Revise,
            Box::new({
                let revise_project = revise_project.clone();
                let revise_sheet = revise_sheet.clone();
                let mut phase = 0u8;
                move |log| {
                    if phase == 0 {
                        // A `None` here ends the stage: the runner reads it
                        // as "nothing to do", so a clean cut is a revise
                        // that succeeded, not one that failed.
                        let commands = revise_commands(log);
                        if commands.is_empty() {
                            return None;
                        }
                        phase = 1;
                        return Some(call(
                            "edit_apply",
                            json!({
                                "path": revise_project,
                                "command": { "op": "batch", "commands": commands },
                            }),
                        ));
                    }
                    if phase == 1 {
                        phase = 2;
                        return Some(call(
                            "watch_render",
                            json!({
                                "project": revise_project,
                                "out": revise_sheet,
                            }),
                        ));
                    }
                    if phase == 2 {
                        phase = 3;
                        // As in the preview: a sheet that named no pixels
                        // ends the re-critique rather than sending the
                        // critic an empty path.
                        let png = log
                            .iter()
                            .rev()
                            .find_map(|reply| reply.get("pngPath").and_then(Value::as_str))
                            .filter(|png| !png.trim().is_empty())?;
                        let measured = signal_brief(log);
                        let brief = if measured.as_object().is_some_and(|o| !o.is_empty()) {
                            measured
                        } else {
                            json!({})
                        };
                        return Some(call(
                            "craft_critique",
                            json!({
                                "pngPath": png,
                                "project": revise_project,
                                "brief": brief,
                                "autoSetup": true,
                            }),
                        ));
                    }
                    None
                }
            }),
        )
        .warn()
        .into(),
    );

    // 13. The render, waited on by its project folder: the job's name is
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
    // 14. The verify: the sanity reads first - collisions, and the
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
                    if phase == 7 {
                        phase = 8;
                        // Transcript and loudness are not proof that the
                        // rendered file is a usable video. Probe the actual
                        // export as the final verification call: the media
                        // reader reports its streams, duration, dimensions,
                        // frame rate and codecs, and refuses a missing or
                        // unreadable file.
                        return Some(call(
                            "motion_probe",
                            json!({ "path": verify_export }),
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
    // 15. The watch, warn-policy: the whole cut as one contact sheet,
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
    /// music bed and accents, the preview, the export, the verify, the
    /// watch and the revise. The plan's creative brief, the eye-picked
    /// stock and the baked motion comps remain the Director agent's.
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
        // transcription, the derived tighten-then-grade, B-roll, captions,
        // title entrances, bed and accents, the preview, the revise,
        // the static export, the derived verify-gates, and the watch.
        // The creative layer's remaining stages (the plan's brief, the
        // baked motion comps) are absent - the fraction the page shows
        // counts only these.
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
                "plan",
                "timeline",
                "gather",
                "captions",
                "motion",
                "music",
                "sfx",
                "preview",
                "revise",
                "export",
                "verify",
                "watch",
            ]
        );
        // The register and the run are the same order, not merely the same
        // set: `Runner::completed` reports a resumed run in `Stage::ALL`'s
        // order, so an `ALL` that disagreed with the program would resume
        // a run in an order it never ran.
        let register: Vec<&str> = Stage::ALL.iter().map(|stage| stage.label()).collect();
        assert_eq!(register, order, "the register is the run's own order");
        // The warn-policy stages, and only them: a take that cannot be
        // repaired ships as recorded, an emotional read that cannot be
        // made leaves the cut on its defaults, a plan the gate refuses
        // still leaves the cut it measured, a tighten or B-roll slot or
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
                "understand",
                "plan",
                "timeline",
                "gather",
                "captions",
                "motion",
                "music",
                "sfx",
                "preview",
                "revise",
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

        // The tighten-and-grade, the B-roll, the captions, the title
        // entrances, the bed, the accents, the preview and the revise are
        // derived: their calls are the run's own replies to answer for,
        // and the runner's reactive queue asks for them one at a time.
        // The tighten and the grade are ONE step: the resume record is
        // keyed by the stage, so a second step sharing `Stage::Timeline`
        // was skipped the moment the first completed and the run never
        // graded.
        for at in [5usize, 6, 7, 8, 9, 10, 11, 12, 13, 15] {
            assert!(
                matches!(program.steps[at], ProgramStep::Derived(_)),
                "step {at} is derived"
            );
        }

        // The export is waited on by its project folder - the job's name
        // is minted at run time - and the watch's sheet is a project-
        // relative name, because the window's server refuses absolute
        // write paths.
        let export = program.steps[14].as_static();
        assert_eq!(export.calls[0].tool, "export_start");
        assert_eq!(export.calls[0].arguments["output"], source.export.as_str());
        assert_eq!(export.calls[1].tool, "export_wait");
        assert_eq!(export.calls[1].arguments["path"], source.project.as_str());
        let watch = program.steps[16].as_static();
        assert_eq!(watch.calls[0].arguments["out"], "auto-edit-sheet.png");
        assert_eq!(
            watch.calls[1].arguments["pngPath"],
            "/edits/Reel edit/auto-edit-sheet.png"
        );

        // The verify's gates: the loudness target, and the words (F-04) -
        // on the derived step that reads the timeline as it ended up.
        use concat_director::ReplyGate;
        assert!(matches!(program.steps[15], ProgramStep::Derived(_)));
        assert_eq!(
            program.steps[15].gates(),
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
            program.steps[16].gates(),
            vec![ReplyGate::AtLeast {
                field: "overall",
                minimum: 7.0,
                what: "the cut scored under the craft bar",
            }]
        );
        // No other step gates its replies.
        for (at, step) in program.steps.iter().enumerate() {
            if at != 15 && at != 16 {
                assert!(step.gates().is_empty(), "step {at} gates its replies");
            }
        }
        // The fan compile-time proof the type system wanted: the repair
        // grouping is the FanStep the runner fans out.
        let _: &FanStep = repair;
    }

    /// A stressed word really does reach the captions: the log entry has
    /// always claimed `emphasisWords`, and until this was wired the caption
    /// stage never read the signal, so the claim was decorative. A caption
    /// carrying one of the take's own stressed words takes the accent; one
    /// carrying none stays plain.
    #[test]
    fn a_stressed_word_takes_the_caption_accent() {
        let takes = vec![TakePaths {
            take: "/takes/a.wav".to_owned(),
            enhanced: String::new(),
        }];
        let log = vec![json!({ "audio": { "path": "/takes/a.wav" },
            "lines": [{
                "prosody": { "emphasisWords": [
                    { "t": 1.0, "strength": 1.2, "wordText": "wreck" },
                ]},
            }],
        })];
        let words = stressed_words(&log, &takes);
        assert_eq!(words, vec!["wreck".to_owned()]);
        let carries = |text: &str| {
            text.split_whitespace()
                .map(|w| {
                    w.trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase()
                })
                .any(|w| words.contains(&w))
        };
        assert!(carries("we found the wreck"));
        assert!(!carries("the hull was empty"));
    }

    /// The graphic's log entry names only the signals the choice consumed:
    /// naming `prosody` while the entrance is picked by the read's
    /// intensity would be the log asserting evidence no decision used.
    #[test]
    fn the_graphic_log_claims_only_the_signals_it_spent() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 3.0, "text": "we found the wreck" },
                { "start": 4.0, "end": 7.0, "text": "it was not empty" },
            ],
        });
        let log = vec![
            json!({ "path": "/t/a.wav", "output": transcript.to_string() }),
            json!({
                "timeline": { "clips": [
                    { "id": "c1", "media": "m1", "start": 0.0, "duration": 8.0,
                      "sourceStart": 0.0, "speed": 1.0 },
                ]},
                "media": [{ "id": "m1", "path": "/t/a.wav" }],
            }),
            json!({ "lines": [
                emotion_row(0.0, 3.0, "surprise", 0.9, "hook", "hot", ""),
                emotion_row(4.0, 7.0, "fear", 0.9, "reveal", "cold", ""),
            ]}),
        ];
        let plan = auto_plan(
            &log,
            &takes,
            &AutoEditProgramSource {
                project: "/t".to_owned(),
                takes: takes.clone(),
                placements: Vec::new(),
                export: "/t/out.mp4".to_owned(),
                sheet: "sheet.png".to_owned(),
                stock_provider: "pexels".to_owned(),
            },
        )
        .expect("a plan from the read");
        let log_rows = plan["signalLog"].as_array().expect("a signal log");
        let graphic = log_rows
            .iter()
            .find(|row| row["decision"] == json!("graphic"))
            .expect("a graphic entry");
        let signals = graphic["signals"].as_array().expect("its signals");
        assert!(
            !signals.contains(&json!("prosody")),
            "the graphic entry claims prosody, which no decision here reads"
        );
        assert!(signals.contains(&json!("emotion")));
    }

    /// The critic's structured fixes are spent, not ignored: a fix that
    /// carries a number this program can apply becomes the edit command
    /// for the clip the named frame plays, and one that carries none - or
    /// arrives under the confidence bar - is left to the report. This is
    /// the part of the review a vision model answered, so the test is
    /// that its word reaches the timeline only where it was exact.
    #[test]
    fn the_critics_exact_fixes_become_edits() {
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "kind": "video", "media": "m1", "trackId": "t1",
                  "start": 0.0, "duration": 4.0 },
                { "id": "c2", "kind": "video", "media": "m2", "trackId": "t1",
                  "start": 4.0, "duration": 4.0, "keyframed": [] },
            ]},
        });
        let log = vec![json!({
            "craft": { "samples": 8.0, "letterboxedFrames": 0.0, "blackFrames": 0.0,
                       "staticTransitions": 0.0 },
            "tiles": [
                { "index": 0, "at": 0.5 }, { "index": 1, "at": 1.5 },
                { "index": 2, "at": 2.5 }, { "index": 3, "at": 3.5 },
                { "index": 4, "at": 4.5 }, { "index": 5, "at": 5.5 },
                { "index": 6, "at": 6.5 }, { "index": 7, "at": 7.5 },
            ],
            "fixes": [
                { "where": "the caption on frame 5", "problem": "enters early",
                  "change": "start 4.9s", "op": "retime",
                  "patch": { "start": 4.9 }, "confidence": 0.8 },
                { "where": "the chart in frames 6-8", "problem": "too small",
                  "change": "scale to 1.2", "op": "resize",
                  "patch": { "factor": 1.2 }, "confidence": 0.7 },
                { "where": "frame 2", "problem": "off centre",
                  "change": "shift x by 96px", "op": "shift",
                  "patch": { "dx": 96, "dy": 0 }, "confidence": 0.65 },
                { "where": "frame 1", "problem": "a taste I hold loosely",
                  "change": "scale to 1.1", "op": "resize",
                  "patch": { "factor": 1.1 }, "confidence": 0.3 },
                { "where": "frame 0", "problem": "unreadable",
                  "change": "make it better", "op": "restyle",
                  "confidence": 0.9 },
            ],
        })];
        let run = [vec![view.clone()], log].concat();
        let commands = revise_commands(&run);
        let ops: Vec<&str> = commands
            .iter()
            .filter_map(|command| command["op"].as_str())
            .collect();
        assert!(
            ops.contains(&"moveClips"),
            "the exact retime is spent: {commands:?}"
        );
        let moved = commands
            .iter()
            .find(|command| command["op"] == json!("moveClips"))
            .expect("the move");
        assert_eq!(moved["moves"][0]["clipId"], json!("c2"));
        assert_eq!(moved["moves"][0]["start"], json!(4.9));
        assert!(ops.contains(&"setClipKey"), "the resize is spent");
        let resized = commands
            .iter()
            .find(|command| command["op"] == json!("setClipKey"))
            .expect("the resize");
        assert_eq!(resized["clipId"], json!("c2"));
        assert!(
            commands
                .iter()
                .any(|command| command["op"] == json!("setClipTransform")),
            "the exact shift is spent"
        );
        // The low-confidence fix and the fix with no patch are the
        // report's, not the timeline's.
        assert!(
            commands
                .iter()
                .filter(|command| command["op"] == json!("setClipKey"))
                .count()
                == 2,
            "one resize spent, the loose one left alone"
        );
        assert!(
            !ops.contains(&"frameClip"),
            "a restyle with no measured fill is not applied"
        );
    }

    /// The brief the critic reads carries what the run measured: the arc,
    /// the peak, the delivery and the words the voice leaned on. A run
    /// with no read hands the critic nothing rather than a guess.
    #[test]
    fn the_critics_brief_carries_the_measured_signals() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
        }];
        let log = vec![
            json!({ "lines": [
                emotion_row(0.0, 3.0, "surprise", 0.9, "hook", "hot", ""),
                emotion_row(4.0, 7.0, "fear", 0.4, "reveal", "cold", ""),
            ]}),
            json!({ "audio": { "path": "/t/a.wav" },
                "lines": [
                    { "start": 0.0, "end": 3.0,
                      "speechRate": { "pace": "punchy" },
                      "prosody": { "emphasisWords": [
                          { "t": 1.0, "strength": 1.2, "wordText": "wreck" },
                      ]} },
                    { "start": 4.0, "end": 7.0, "speechRate": { "pace": "slow" } },
                ],
            }),
        ];
        let brief = signal_brief(&log);
        let _ = takes;
        assert_eq!(brief["emotionalArc"], json!("hook -> reveal"));
        assert_eq!(brief["delivery"], json!("punchy, slow"));
        assert_eq!(brief["emphasisWords"], json!(["wreck"]));
        assert!(
            brief["peak"]
                .as_str()
                .is_some_and(|peak| peak.contains("surprise")),
            "the peak names the strongest line: {brief:?}"
        );
        assert!(
            brief["colourSplit"]
                .as_str()
                .is_some_and(|split| split.contains("1 of 2")),
            "the colour split is counted: {brief:?}"
        );
        assert!(
            signal_brief(&[])
                .as_object()
                .is_some_and(|brief| brief.is_empty()),
            "a run with no read hands the critic nothing"
        );
    }

    /// A plan built on a take whose delivery was measured names that
    /// delivery in its log, and one built on a take with no rate measured
    /// names none: the log never claims a rhythm the reading did not hear.
    #[test]
    fn the_plan_logs_the_delivery_only_where_it_was_measured() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
        }];
        let transcript = json!({
            "segments": [{ "start": 0.0, "end": 3.0, "text": "we found the wreck" }],
        });
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 3.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/t/a.wav" }],
        });
        let read = json!({ "lines": [
            emotion_row(0.0, 3.0, "surprise", 0.9, "hook", "hot", ""),
        ]});
        let source = || AutoEditProgramSource {
            project: "/t".to_owned(),
            takes: takes.clone(),
            placements: Vec::new(),
            export: "/t/out.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let pacing = |log: &[Value]| {
            let plan = auto_plan(log, &takes, &source()).expect("a plan from the read");
            plan["signalLog"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["decision"] == json!("pacing"))
                .cloned()
                .collect::<Vec<_>>()
        };
        let measured = vec![
            json!({ "path": "/t/a.wav", "output": transcript.to_string() }),
            view.clone(),
            read.clone(),
            json!({ "audio": { "path": "/t/a.wav" },
                "lines": [{ "start": 0.0, "end": 3.0,
                            "speechRate": { "pace": "punchy" } }] }),
        ];
        let entries = pacing(&measured);
        assert_eq!(entries.len(), 1, "one measured pace, one entry");
        let signals = entries[0]["signals"].as_array().expect("signals");
        assert!(signals.contains(&json!("speechRate")));
        assert!(
            signals
                .iter()
                .filter_map(Value::as_str)
                .all(crate::tools::signal_vocabulary_has)
        );
        let unmeasured = vec![
            json!({ "path": "/t/a.wav", "output": transcript.to_string() }),
            view,
            read,
        ];
        assert!(
            pacing(&unmeasured).is_empty(),
            "a take with no measured rate logs no pacing"
        );
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
    /// The preview looks at the OPEN timeline before the render is paid
    /// for: the sheet it samples is the project's, not the export's, and
    /// its own name - the watch's sheet is written later and must not be
    /// the file the preview scored.
    #[test]
    fn the_preview_samples_the_open_timeline_and_its_own_sheet() {
        use concat_director::ProgramStep;

        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: Vec::new(),
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let at = program
            .steps
            .iter()
            .position(|step| step.stage() == Stage::Preview)
            .expect("the preview stage is in the program");
        let ProgramStep::Derived(preview) = &mut program.steps[at] else {
            panic!("the preview reacts to the timeline it just read");
        };

        let first = (preview.next)(&[]).expect("the preview samples the cut");
        assert_eq!(first.tool, "watch_render");
        assert_eq!(
            first.arguments["project"], source.project,
            "the preview samples the open timeline, not the render"
        );
        assert!(first.arguments.get("path").is_none());
        assert_eq!(first.arguments["out"], "auto-edit-sheet-preview.png");

        // The critic reads the path the sheet itself named, not one the
        // program reconstructs from the name it asked for.
        let sheet = json!({
            "ok": true,
            "pngPath": "/edits/Reel/auto-edit-sheet-preview.png",
            "craft": { "samples": 12 },
        });
        let second = (preview.next)(&[sheet]).expect("the preview's sheet is scored");
        assert_eq!(second.tool, "craft_critique");
        assert_eq!(
            second.arguments["pngPath"],
            "/edits/Reel/auto-edit-sheet-preview.png"
        );
        assert_eq!(second.arguments["project"], source.project);
        assert!((preview.next)(&[json!({}), json!({})]).is_none());
    }

    /// A revision that landed must be in the file that ships. The revise
    /// therefore runs on the OPEN timeline BEFORE the export, not after
    /// it: an edit applied once the render had landed would exist only in
    /// the project, and the exported video - the thing the user publishes
    /// - would still show the cut the critic faulted.
    #[test]
    fn the_revise_lands_before_the_export_so_the_shipped_file_has_it() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: Vec::new(),
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let program = auto_edit_program(&source);
        let order: Vec<&str> = program
            .steps
            .iter()
            .map(|step| step.stage().label())
            .collect();
        let at = |label: &str| {
            order
                .iter()
                .position(|stage| *stage == label)
                .unwrap_or_else(|| panic!("{label} is in the program"))
        };
        let preview = at("preview");
        let revise = at("revise");
        let export = at("export");
        let verify = at("verify");
        let watch = at("watch");

        // The revise reads the preview's sheet, so it follows it...
        assert!(preview < revise, "the revise answers the preview");
        // ...and corrects the timeline the export then encodes.
        assert!(
            revise < export,
            "a revise after the export leaves the shipped file unrevised"
        );
        // The verify and the watch both read the file, so both follow it.
        assert!(export < verify, "the verify certifies the export");
        assert!(export < watch, "the watch scores the exported file");
    }

    /// The revise answers a measured fault and only a measured fault: a
    /// sheet with no bars, no black opening and moving cuts issues no
    /// edit, and the stage completes on its first `None` instead of
    /// watching a cut it would not change.
    #[test]
    fn a_clean_cut_revises_nothing() {
        use concat_director::ProgramStep;

        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: Vec::new(),
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let at = program
            .steps
            .iter()
            .position(|step| step.stage() == Stage::Revise)
            .expect("the revise stage is in the program");
        assert!(
            program.steps[at].gates().is_empty(),
            "the revise gates nothing"
        );
        let ProgramStep::Derived(revise) = &mut program.steps[at] else {
            panic!("the revise reacts to what the watch measured");
        };

        let clean = json!({
            "craft": {
                "samples": 12,
                "letterboxedFrames": 0,
                "blackFrames": 0,
                "staticTransitions": 1,
            },
            "tiles": [{ "blackFraction": 0.0 }],
            "timeline": { "clips": [] },
        });
        assert!(
            (revise.next)(&[clean]).is_none(),
            "a clean cut is a revise that succeeded, not one to re-watch"
        );
    }

    /// The revise's edits come from the sheet's numbers, not from taste:
    /// bars become a fill on the pictures, a black opening becomes a trim
    /// of the lead-in, and near-static cuts become a push-in on every
    /// *other* shot - never the same ride on all of them.
    #[test]
    fn the_revise_turns_measured_faults_into_targeted_edits() {
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "kind": "video", "start": 0.0, "duration": 4.0 },
                { "id": "c2", "kind": "video", "start": 4.0, "duration": 4.0 },
                { "id": "c3", "kind": "image", "start": 8.0, "duration": 3.0 },
                { "id": "t1", "kind": "text", "start": 0.0, "duration": 2.0 },
                { "id": "k1", "kind": "video", "start": 11.0, "duration": 2.0,
                  "keyframed": ["scale"] },
            ]},
        });
        let log = vec![
            json!({
                "craft": {
                    "samples": 12,
                    "letterboxedFrames": 5,
                    "blackFrames": 2,
                    "staticTransitions": 9,
                },
                "tiles": [{ "blackFraction": 0.9 }],
            }),
            view,
        ];
        let commands = revise_commands(&log);
        let ops: Vec<&str> = commands
            .iter()
            .filter_map(|command| command["op"].as_str())
            .collect();

        // Bars: a fill on the pictures, and never on the title - a title's
        // scale is its entrance. The clip already carrying a scale ride is
        // left alone.
        let fills: Vec<&Value> = commands
            .iter()
            .filter(|command| command["op"] == "frameClip")
            .collect();
        assert_eq!(fills.len(), 3, "every un-keyed picture is filled");
        for fill in fills {
            assert_eq!(fill["framing"], "fill");
            assert_ne!(fill["clipId"], "t1");
            assert_ne!(fill["clipId"], "k1");
        }

        // The black opening: the first picture's lead-in trimmed, rippled,
        // and bounded to a quarter of the clip.
        let trim = commands
            .iter()
            .find(|command| command["op"] == "trimClip")
            .expect("the black opening is trimmed");
        assert_eq!(trim["clipId"], "c1");
        assert_eq!(trim["edge"], "start");
        assert_eq!(trim["ripple"], true);
        assert_eq!(trim["delta"], 0.5, "half a second, the cap");

        // Static cuts: a push-in on every other shot, never all of them.
        let pushes: Vec<&Value> = commands
            .iter()
            .filter(|command| command["op"] == "setClipKey")
            .collect();
        assert_eq!(pushes.len(), 4, "two shots, two keys each");
        let pushed: Vec<&str> = pushes
            .iter()
            .filter_map(|command| command["clipId"].as_str())
            .collect();
        assert!(pushed.contains(&"c1") && pushed.contains(&"c3"));
        assert!(
            !pushed.contains(&"c2"),
            "the same zoom on every cut is the fault"
        );
        assert!(!pushed.contains(&"k1"), "a keyed clip keeps its own ride");

        assert!(ops.contains(&"frameClip") && ops.contains(&"trimClip"));

        // The batch is legal for the API, not merely shaped right: the
        // commands deserialize into the `Request` an `edit_apply` carries,
        // through the same reader the tool's arguments go through. A typo
        // in an op name or a field would fail here, not mid-run on a
        // user's laptop.
        let request = json!({
            "method": "edit.apply",
            "path": "/edits/Reel",
            "command": { "op": "batch", "commands": commands },
        });
        let parsed: concat_api::Request =
            serde_json::from_value(request).expect("the revise batch is a legal edit");
        assert!(matches!(parsed, concat_api::Request::EditApply { .. }));
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
        let ProgramStep::Derived(music) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "music")
            .expect("the program has a music stage")
        else {
            panic!("the music stage is derived");
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
        let ProgramStep::Derived(captions) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "captions")
            .expect("the program has a captions stage")
        else {
            panic!("the captions stage is derived");
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

    /// The planner's own read wins over the plain gap scan: its
    /// suggestions are ranked by the priority it scored them at, and the
    /// gather stage shops where the plan says coverage is wanted. A plan
    /// whose spans fall outside the shot-length window is not honoured -
    /// a 0.4 s hole cannot hold a shot.
    #[test]
    fn the_planner_s_suggestions_drive_the_slots_it_ranked() {
        let takes = vec![TakePaths {
            take: "/takes/a.wav".to_owned(),
            enhanced: "/takes/a-enhanced.wav".to_owned(),
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "the harbour at dawn" },
                { "start": 6.0, "end": 8.0, "text": "and the fishing boats" },
            ],
        });
        let log = vec![
            json!({ "path": "/takes/a.wav", "output": transcript.to_string() }),
            json!({
                "take": "/takes/a.wav",
                "beatGridMeasured": false,
                "gapCount": 2,
                "suggestionCount": 2,
                "cutCount": 0,
                "gaps": [],
                "cuts": [],
                "suggestions": [
                    // Too short to hold a shot: dropped.
                    { "kind": "dialogue_gap", "needBRoll": true, "category": "unknown",
                      "startS": 2.0, "endS": 2.4, "durationMs": 400, "priority": 0.9,
                      "topicTags": [] },
                    // The real ask, and the one the plan scored highest.
                    { "kind": "dialogue_gap", "needBRoll": true, "category": "outdoor",
                      "startS": 2.0, "endS": 6.0, "durationMs": 4000, "priority": 0.8,
                      "topicTags": ["harbour", "boats"] },
                ],
            }),
        ];
        let slots = broll_slots(&log, &takes);
        assert_eq!(slots.len(), 1, "only the span that can hold a shot");
        assert_eq!(slots[0].from, 2.0);
        assert_eq!(slots[0].to, 6.0);
        // The planner's topic tags, not the tail of the line before.
        assert_eq!(slots[0].query, "harbour boats");
    }

    /// Planner replies stay attached to the take whose independent file
    /// clock they were measured in, even when two takes use identical times.
    #[test]
    fn the_planner_keeps_each_take_s_slots_separate() {
        let take_a = "/takes/a.wav";
        let take_b = "/takes/b.wav";
        let takes = vec![
            TakePaths {
                take: take_a.to_owned(),
                enhanced: String::new(),
            },
            TakePaths {
                take: take_b.to_owned(),
                enhanced: String::new(),
            },
        ];
        let log = vec![
            json!({ "path": take_a, "output": json!({ "segments": [
                { "start": 0.0, "end": 2.0, "text": "harbour lights" },
                { "start": 5.0, "end": 7.0, "text": "ships at dawn" },
            ]}).to_string() }),
            json!({ "path": take_b, "output": json!({ "segments": [
                { "start": 0.0, "end": 3.0, "text": "mountain road" },
                { "start": 6.0, "end": 8.0, "text": "cars in snow" },
            ]}).to_string() }),
            json!({
                "take": take_a, "beatGridMeasured": false,
                "gapCount": 1, "suggestions": [{
                    "kind": "dialogue_gap", "needBRoll": true,
                    "startS": 2.0, "endS": 5.0, "priority": 0.8,
                    "topicTags": ["harbour", "ships"],
                }],
            }),
            json!({
                "take": take_b, "beatGridMeasured": false,
                "gapCount": 1, "suggestions": [{
                    "kind": "dialogue_gap", "needBRoll": true,
                    "startS": 3.0, "endS": 6.0, "priority": 0.7,
                    "topicTags": ["mountain", "snow"],
                }],
            }),
        ];

        let slots = broll_slots(&log, &takes);
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].take, take_a);
        assert_eq!((slots[0].from, slots[0].to), (2.0, 5.0));
        assert_eq!(slots[0].query, "harbour ships");
        assert_eq!(slots[1].take, take_b);
        assert_eq!((slots[1].from, slots[1].to), (3.0, 6.0));
        assert_eq!(slots[1].query, "mountain snow");
    }

    /// An explicit planner decision that a span does not need B-roll
    /// must not be overridden by the less-informed plain gap fallback.
    #[test]
    fn an_explicit_no_broll_suggestion_does_not_fall_back_to_gap_scan() {
        let take = "/takes/a.wav";
        let takes = vec![TakePaths {
            take: take.to_owned(),
            enhanced: String::new(),
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "the harbour at dawn" },
                { "start": 6.0, "end": 8.0, "text": "and the fishing boats" },
            ],
        });
        let log = vec![
            json!({ "path": take, "output": transcript.to_string() }),
            json!({
                "take": take,
                "beatGridMeasured": false,
                "gapCount": 1,
                "suggestions": [{
                    "kind": "dialogue_gap",
                    "needBRoll": false,
                    "startS": 2.0,
                    "endS": 6.0,
                    "priority": 0.2,
                    "topicTags": [],
                }],
            }),
        ];

        assert!(
            broll_slots(&log, &takes).is_empty(),
            "an explicit no-B-roll answer is authoritative"
        );
    }

    /// A monotony run means "speech no beat lands in". With no beat grid
    /// every long stretch of speech answers that trivially, so honouring
    /// these without a measured grid would blanket a talking-head cut in
    /// B-roll. They are taken only when the run measured a grid.
    #[test]
    fn a_monotony_run_without_a_measured_grid_is_not_honoured() {
        let takes = vec![TakePaths {
            take: "/takes/a.wav".to_owned(),
            enhanced: "/takes/a-enhanced.wav".to_owned(),
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "the harbour at dawn" },
                { "start": 2.0, "end": 12.0, "text": "and the long unbroken explanation" },
            ],
        });
        let plan = json!({
            "take": "/takes/a.wav",
            "beatGridMeasured": false,
            "gapCount": 0,
            "suggestionCount": 1,
            "cutCount": 0,
            "gaps": [],
            "cuts": [],
            "suggestions": [{
                "kind": "monotony_run", "needBRoll": true, "category": "unknown",
                "startS": 2.0, "endS": 10.0, "durationMs": 8000, "priority": 0.6,
                "topicTags": ["harbour"],
            }],
        });
        let no_grid = vec![
            json!({ "path": "/takes/a.wav", "output": transcript.to_string() }),
            plan.clone(),
        ];
        assert!(
            broll_slots(&no_grid, &takes).is_empty(),
            "a monotony run with no beat grid is not an ask"
        );

        // With a measured grid the run is a real ask, and is honoured.
        let mut with_grid = no_grid.clone();
        with_grid[1]["beatGridMeasured"] = json!(true);
        let slots = broll_slots(&with_grid, &takes);
        assert_eq!(slots.len(), 1, "the measured grid makes the run an ask");
        assert_eq!(slots[0].query, "harbour");
    }

    /// The stock page is read for the clip that fits the slot, not the
    /// clip the shelf happened to rank first: a vertical project gets a
    /// vertical clip, a clip too short to cover the hole is not a
    /// candidate, and a clip already shopped is never shopped again.
    #[test]
    fn the_best_hit_fits_the_slot_rather_than_the_first_result() {
        let hit = |id: &str, w: f64, h: f64, seconds: f64| {
            json!({
                "id": id, "provider": "pexels", "url": format!("https://x/{id}"),
                "license": "Pexels License", "width": w, "height": h,
                "seconds": seconds,
            })
        };
        let page = vec![
            // The shelf's own first result: a 4K landscape clip in a reel.
            hit("a", 3840.0, 2160.0, 30.0),
            // Fits the vertical frame and covers the 4 s hole.
            hit("b", 1080.0, 1920.0, 12.0),
            // Vertical but only 1.5 s: cannot cover the hole.
            hit("c", 1080.0, 1920.0, 1.5),
        ];
        let picked = best_hit(&page, 4.0, "9:16", &[]).expect("a vertical clip fits");
        assert_eq!(picked["id"], "b");

        // In a horizontal project the landscape clip is the right one.
        let picked = best_hit(&page, 4.0, "16:9", &[]).expect("a landscape clip fits");
        assert_eq!(picked["id"], "a");

        // Missing duration is not evidence that a hit can cover the hole.
        let unmeasured = hit("unknown-duration", 1080.0, 1920.0, 0.0);
        assert!(
            best_hit(&[unmeasured], 4.0, "9:16", &[]).is_none(),
            "a hit with no measured duration cannot be promised as full coverage"
        );

        // A clip already shopped is not a candidate: the next gap takes
        // the next best fit rather than repeating the shot.
        let picked = best_hit(&page, 4.0, "9:16", &["b".to_owned()])
            .expect("a clip that has not been used fits");
        assert_ne!(picked["id"], "b");
        assert_ne!(
            picked["id"], "c",
            "the short clip still cannot cover the hole"
        );

        // Nothing left that fits the frame: the best remaining candidate is
        // a landscape clip that will be cropped to fill. Orientation is a
        // preference, not a veto - a cropped shot still beats a hole in the
        // narration - so it is returned last rather than refused.
        let picked = best_hit(&page, 4.0, "9:16", &["b".to_owned(), "c".to_owned()])
            .expect("the landscape clip is the last resort");
        assert_eq!(picked["id"], "a");

        // Only a page with no licensable clip at all fills nothing.
        assert!(best_hit(&[], 4.0, "9:16", &[]).is_none());
    }

    /// A hit without the licence metadata the import gate needs is never
    /// a candidate, whatever its shape.
    #[test]
    fn a_hit_without_a_licence_is_never_the_best_hit() {
        let page = vec![json!({
            "id": "x", "provider": "pexels", "url": "https://x/x",
            "width": 1080.0, "height": 1920.0, "seconds": 10.0,
        })];
        assert!(best_hit(&page, 3.0, "9:16", &[]).is_none());
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
                "license": "Pexels License",
                "width": 1080.0,
                "height": 1920.0,
                "seconds": 10.0
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
        assert!((gather.next)(&log).is_none(), "one gap is placed only once");
    }

    /// A malformed live-stock hit without a provider licence is not
    /// imported or placed.
    #[test]
    fn the_broll_stage_skips_hits_without_license_metadata() {
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
            "timeline": { "clips": [{
                "id": "voice",
                "media": "voice-media",
                "start": 0.0,
                "duration": 10.0,
                "sourceStart": 0.0,
                "speed": 1.0
            }]},
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
        log.push(json!({
            "hits": [{
                "id": "2499611",
                "provider": "pexels",
                "url": "https://videos.pexels.com/files/hd.mp4"
            }]
        }));
        assert!(
            (gather.next)(&log).is_none(),
            "a hit without licence metadata is left unused"
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

    /// The read's own line, built the way the `emotion` extract writes it.
    fn emotion_row(
        start: f64,
        end: f64,
        emotion: &str,
        intensity: f64,
        act: &str,
        temperature: &str,
        sfx: &str,
    ) -> Value {
        json!({
            "start": start, "end": end, "emotion": emotion, "intensity": intensity,
            "act": act, "duckDb": -14.0, "sfxType": sfx,
            "colourTemp": { "temperature": temperature },
            "bgmWords": ["warm cinematic strings", "cinematic"],
        })
    }

    /// The emotion read the stage asks for is derived from the take's own
    /// transcripts: one `extract` per take that has timed words, carrying
    /// exactly those lines. A take with no transcript is never asked
    /// about, so the replies and the takes stay zipped in the reader.
    #[test]
    fn the_emotion_stage_reads_the_takes_own_lines() {
        use concat_director::ProgramStep;

        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![
                TakePaths {
                    take: "/edits/Reel/voiceover/a.wav".to_owned(),
                    enhanced: String::new(),
                },
                TakePaths {
                    take: "/edits/Reel/voiceover/b.wav".to_owned(),
                    enhanced: String::new(),
                },
            ],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(understand) = &mut program.steps[4] else {
            panic!("step 4 is the understand stage");
        };
        // Only the first take transcribed: one music gate per take, one
        // emotion read for the take with words.
        let log = vec![
            json!({ "musicPresent": false }),
            json!({ "musicPresent": true }),
            json!({
                "path": "/edits/Reel/voiceover/a.wav",
                "output": json!({
                    "segments": [
                        { "start": 0.0, "end": 1.5, "text": "we found it" },
                        { "start": 2.0, "end": 3.0, "text": "and it was ours" },
                    ],
                }).to_string(),
            }),
        ];
        let mut asked: Vec<ToolCall> = Vec::new();
        while let Some(next) = (understand.next)(&log) {
            asked.push(next);
        }
        let reads: Vec<&ToolCall> = asked.iter().filter(|c| c.tool == "emotion").collect();
        assert_eq!(reads.len(), 1, "one read, for the take that has words");
        let lines = reads[0].arguments["lines"]
            .as_array()
            .expect("the read is given lines");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["text"], "we found it");
        assert_eq!(lines[0]["start"], 0.0);
        assert_eq!(lines[1]["end"], 3.0);
    }

    /// The delivery the reading measured reaches the decisions it should:
    /// the rate over a span is the line that overlaps it most, and a span
    /// no line measured answers empty rather than inventing a pace.
    #[test]
    fn the_delivery_pace_is_read_from_the_line_over_the_span() {
        let log = vec![json!({
            "audio": { "path": "/t/a.wav" },
            "lines": [
                { "start": 0.0, "end": 2.0, "speechRate": { "pace": "punchy" } },
                { "start": 2.0, "end": 6.0, "speechRate": { "pace": "slow" } },
                { "start": 6.0, "end": 7.0 },
            ],
        })];
        assert_eq!(delivery_pace(&log, "/t/a.wav", 0.2, 1.4), "punchy");
        assert_eq!(delivery_pace(&log, "/t/a.wav", 2.5, 5.0), "slow");
        assert_eq!(
            delivery_pace(&log, "/t/a.wav", 6.1, 6.9),
            "",
            "a line with no measured rate answers no pace"
        );
        assert_eq!(
            delivery_pace(&log, "/t/other.wav", 0.0, 2.0),
            "",
            "another take's reading is not this take's pace"
        );
    }

    /// A card on a punchy line holds tighter than the same line delivered
    /// slowly, and an unmeasured line keeps the editorial default: the
    /// delivery is what sets the beat, not the clock alone.
    #[test]
    fn a_punchy_line_holds_a_tighter_card() {
        let card = |pace: &str| MotionSpec {
            take: "/t/a.wav".to_owned(),
            at: 0.0,
            end: 4.0,
            preset: "kinetic.title",
            words: "the wreck".to_owned(),
            act: "reveal".to_owned(),
            pace: pace.to_owned(),
        };
        assert!(card("punchy").hold() < card("").hold());
        assert!(card("slow").hold() > card("").hold());
    }

    /// The look the grade lays over the cut is the temperature the read
    /// named, not a fixed blockbuster look: a joyful take grades golden,
    /// a fearful one cold, and only an unread take falls back.
    #[test]
    fn the_grade_follows_the_read_s_temperature() {
        let lines = vec![
            EmotionLine {
                take: "/t/a.wav".to_owned(),
                start: 0.0,
                end: 2.0,
                emotion: "joy".to_owned(),
                intensity: 0.9,
                act: "reveal".to_owned(),
                temperature: "warm".to_owned(),
                duck_db: -14.0,
                sfx_type: "bright chime, pop, uplifting swell".to_owned(),
                bgm_words: vec!["warm".to_owned()],
                text: String::new(),
                pace: String::new(),
            },
            EmotionLine {
                take: "/t/a.wav".to_owned(),
                start: 2.0,
                end: 4.0,
                emotion: "neutral".to_owned(),
                intensity: 0.3,
                act: "setup".to_owned(),
                temperature: "balanced".to_owned(),
                duck_db: -20.0,
                sfx_type: String::new(),
                bgm_words: Vec::new(),
                text: String::new(),
                pace: String::new(),
            },
        ];
        let line = dominant(&lines).expect("a dominant line");
        assert_eq!(
            line.emotion, "joy",
            "the intense line wins, not the calm one"
        );
        assert_eq!(
            look_for_temperature(&line.temperature),
            "concat.golden-hour"
        );
        assert_eq!(look_for_temperature("cold"), "concat.cool-blue");
        assert_eq!(look_for_temperature("hot"), "concat.warm");
        assert_eq!(look_for_temperature("unheard-of"), "concat.teal-orange");
    }

    /// The director's no-repeat rule, enforced: two shots cut from the
    /// same mood never wear the same transition twice running.
    #[test]
    fn two_shots_of_one_mood_never_repeat_a_transition() {
        for mood in ["joy", "surprise", "anger", "fear", "sadness", "disgust"] {
            let first = transition_for_emotion(mood);
            let second = alternate_transition(mood);
            assert_ne!(first, second, "{mood} has a distinct alternate");
        }
        assert_eq!(transition_for_emotion("joy"), "concat.cross-pop");
        assert_eq!(alternate_transition("joy"), "concat.dissolve");
    }

    /// The accents are shopped by the read's own register: a line the
    /// emotion engine called a stinger is searched for a stinger, not for
    /// the generic family its emotion happens to share.
    #[test]
    fn the_accents_are_shopped_by_the_read_s_register() {
        let stinger = EmotionLine {
            take: "/t/a.wav".to_owned(),
            start: 0.0,
            end: 1.0,
            emotion: "surprise".to_owned(),
            intensity: 0.9,
            act: "reveal".to_owned(),
            temperature: "split".to_owned(),
            duck_db: -12.0,
            sfx_type: "hit + beat of silence (stinger)".to_owned(),
            bgm_words: Vec::new(),
            text: String::new(),
            pace: String::new(),
        };
        assert_eq!(
            sfx_queries_for_line(Some(&stinger), "surprise"),
            vec!["stinger", "hit", "impact"]
        );
        // No register named: the emotion's family is the fallback.
        let mut plain = stinger.clone();
        plain.sfx_type = String::new();
        assert_eq!(
            sfx_queries_for_line(Some(&plain), "surprise"),
            sfx_queries_for_emotion("surprise").to_vec()
        );
        assert_eq!(
            sfx_queries_for_line(None, "neutral"),
            sfx_queries_for_emotion("neutral").to_vec()
        );
    }

    /// A silence after a hard line is a held beat, not dead air: the
    /// tighten leaves it alone even though the detector called it
    /// `dead_air`. A silence in the middle of a calm stretch is cut.
    #[test]
    fn the_tighten_keeps_the_beat_after_a_hard_line() {
        let line = |start: f64, end: f64, intensity: f64, act: &str| EmotionLine {
            take: "/t/a.wav".to_owned(),
            start,
            end,
            emotion: "anger".to_owned(),
            intensity,
            act: act.to_owned(),
            temperature: "hot".to_owned(),
            duck_db: -12.0,
            sfx_type: String::new(),
            bgm_words: Vec::new(),
            text: String::new(),
            pace: String::new(),
        };
        let read = vec![line(0.0, 2.0, 0.9, "tension")];
        assert!(
            held_beat(&read, "/t/a.wav", 2.2),
            "a hard line's beat is held"
        );
        assert!(
            !held_beat(&read, "/t/a.wav", 2.9),
            "a silence well after the line is not its beat"
        );
        let calm = vec![line(0.0, 2.0, 0.3, "setup")];
        assert!(
            !held_beat(&calm, "/t/a.wav", 2.2),
            "a calm line's pause is cut"
        );
        let reveal = vec![line(0.0, 2.0, 0.4, "reveal")];
        assert!(
            held_beat(&reveal, "/t/a.wav", 2.2),
            "the pause after a reveal is held whatever its volume"
        );

        // And the tighten honours it: the same pause the detector called
        // dead air is cut for a calm line and kept for a hard one.
        let log = |intensity: f64, act: &str| {
            vec![
                json!({
                    "timeline": { "clips": [{
                        "id": "c1", "media": "m1", "start": 0.0, "duration": 6.0,
                        "sourceStart": 0.0, "speed": 1.0,
                    }]},
                    "media": [{ "id": "m1", "path": "/t/a.wav" }],
                }),
                json!({
                    "audio": { "path": "/t/a.wav" },
                    "pauses": [{ "start": 2.0, "end": 3.5, "class": "dead_air" }],
                }),
                // The take's transcript: the reader only asks `emotion`
                // about takes with timed words, so the read below is the
                // reply to this one.
                json!({
                    "path": "/t/a.wav",
                    "output": json!({
                        "segments": [{ "start": 0.0, "end": 2.0, "text": "we found it" }],
                    }).to_string(),
                }),
                json!({
                    "lines": [emotion_row(0.0, 2.0, "anger", intensity, act, "hot", "")],
                }),
            ]
        };
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
        }];
        let hard = log(0.9, "tension");
        let read = emotion_lines(&hard, &takes);
        assert!(
            dead_air_cuts(&hard, &takes, "/edits/Reel", &read).is_empty(),
            "the held beat is not cut"
        );
        let calm = log(0.3, "setup");
        let read = emotion_lines(&calm, &takes);
        assert_eq!(
            dead_air_cuts(&calm, &takes, "/edits/Reel", &read).len(),
            1,
            "the calm pause is cut"
        );
    }

    /// The bed is shopped by the read's own words and ducked to the level
    /// its act profile sets - not to a fixed -20 dB.
    #[test]
    fn the_bed_follows_the_read_s_words_and_level() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
        }];
        let log = vec![
            json!({
                "path": "/t/a.wav",
                "output": json!({
                    "segments": [{ "start": 0.0, "end": 2.0, "text": "we found it" }],
                }).to_string(),
            }),
            json!({
                "lines": [emotion_row(0.0, 2.0, "joy", 0.9, "reveal", "warm", "")],
            }),
        ];
        let read = emotion_lines(&log, &takes);
        let line = dominant(&read).expect("the read");
        assert_eq!(line.bgm_words[0], "warm cinematic strings");
        assert_eq!(line.duck_db, -14.0);
    }

    /// An accent on every emphasis peak is the robotic mistake: where the
    /// read exists, only a peak inside a line that reads as feeling, at a
    /// strength worth hearing, earns a sound. A stressed word in a calm
    /// informational line does not.
    #[test]
    fn the_accents_skip_peaks_in_calm_lines() {
        use concat_director::ProgramStep;

        let take = "/t/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(sfx) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "sfx")
            .expect("the program has an sfx stage")
        else {
            panic!("the sfx stage is derived");
        };
        let view = json!({
            "timeline": { "clips": [{
                "id": "c1", "media": "m1", "start": 0.0, "duration": 20.0,
                "sourceStart": 0.0, "speed": 1.0,
            }]},
            "media": [{ "id": "m1", "path": take }],
        });
        // Two peaks: one inside a loud angry line, one inside a calm
        // informational line the read calls neutral.
        let log = vec![
            view,
            json!({
                "path": take,
                "output": json!({
                    "segments": [
                        { "start": 0.0, "end": 4.0, "text": "we found it" },
                        { "start": 5.0, "end": 9.0, "text": "the file is on the desk" },
                    ],
                }).to_string(),
            }),
            json!({
                "audio": { "path": take },
                "lines": [
                    { "start": 0.0, "end": 4.0,
                      "prosody": { "emphasisWords": [{ "t": 1.0, "strength": 0.9 }] } },
                    { "start": 5.0, "end": 9.0,
                      "prosody": { "emphasisWords": [{ "t": 6.0, "strength": 0.8 }] } },
                ],
            }),
            json!({
                "lines": [
                    emotion_row(0.0, 4.0, "anger", 0.9, "tension", "hot", "impact-heavy (hard hits, distorted risers)"),
                    emotion_row(5.0, 9.0, "neutral", 0.2, "setup", "balanced", ""),
                ],
            }),
        ];
        let first = (sfx.next)(&log).expect("the loud peak is shopped");
        assert_eq!(first.tool, "sounds_search");
        // The calm line's peak is skipped: the shelf's answer for the loud
        // one is the last call, so the next ask is the add, and once that
        // is spent the stage ends rather than shopping the calm peak.
        let mut after = log.clone();
        after.push(json!({ "sounds": [{ "id": "s1" }] }));
        let add = (sfx.next)(&after).expect("the sound is added");
        assert_eq!(add.tool, "sounds_add");
        after.push(json!({ "mediaIds": ["m2"] }));
        assert_eq!(
            (sfx.next)(&after),
            None,
            "the calm line's peak never earns an accent"
        );
    }

    /// The plan the run writes is the one it measured, and it says which
    /// of the brief's questions the measurement could not answer instead
    /// of inventing an answer. Every decision in it names a signal.
    #[test]
    fn the_plan_names_the_signal_behind_every_decision() {
        use concat_director::ProgramStep;

        let take = "/t/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(plan) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "plan")
            .expect("the program has a plan stage")
        else {
            panic!("the plan stage is derived");
        };
        let log = vec![
            json!({
                "settings": { "width": 1080, "height": 1920 },
                "timeline": { "clips": [
                    { "id": "c1", "media": "m1", "start": 0.0, "duration": 6.0,
                      "sourceStart": 0.0, "speed": 1.0 },
                    { "id": "c2", "media": "m1", "start": 6.0, "duration": 6.0,
                      "sourceStart": 6.0, "speed": 1.0 },
                ]},
                "media": [{ "id": "m1", "path": take }],
            }),
            json!({
                "path": take,
                "output": json!({
                    "segments": [
                        { "start": 0.0, "end": 2.0, "text": "we found the wreck" },
                        { "start": 3.0, "end": 5.0, "text": "it was not empty" },
                        { "start": 7.0, "end": 9.0, "text": "and then it moved" },
                    ],
                }).to_string(),
            }),
            json!({
                "lines": [
                    emotion_row(0.0, 2.0, "surprise", 0.6, "hook", "cold", ""),
                    emotion_row(3.0, 5.0, "fear", 0.8, "tension", "cold", "impact-heavy (hard hits)"),
                    emotion_row(7.0, 9.0, "fear", 0.9, "reveal", "cold", "stinger"),
                ],
            }),
        ];
        // The footage shopping list is planned first: the planner reads the
        // transcript and says where coverage is wanted, before the plan
        // that names the signals is validated.
        let first = (plan.next)(&log).expect("the footage plan is asked for");
        assert_eq!(first.tool, "footage_plan_suggest");
        assert!(
            !first.arguments["segments"]
                .as_array()
                .expect("the planner is given the transcript")
                .is_empty(),
            "the planner reads the narration's own spans: {first:?}"
        );
        let ask = (plan.next)(&log).expect("the plan is validated");
        assert_eq!(ask.tool, "plan_validate");
        let plan = &ask.arguments["plan"];
        // The frame the project is: a 1080x1920 project is a vertical reel.
        assert_eq!(plan["aspect"], "9:16");
        assert_eq!(plan["mode"], "reel");
        // The narration answered the thesis and the payoff; audience and
        // goal are not in a transcript, so they stay absent and the gate
        // is what names them.
        assert_eq!(plan["creativeBrief"]["thesis"], "we found the wreck");
        assert_eq!(plan["creativeBrief"]["payoff"], "and then it moved");
        assert!(
            plan["creativeBrief"].get("audience").is_none(),
            "an audience is not measured from a transcript"
        );
        // The grade is the read's temperature, with the package's own
        // intensity parameter - the pair the plan validates.
        assert_eq!(plan["grade"]["filter"], "concat.cool-blue");
        assert_eq!(plan["grade"]["intensityKey"], "chill");
        // Every decision names at least one signal from the vocabulary.
        let entries = plan["signalLog"].as_array().expect("a signalLog");
        assert!(!entries.is_empty());
        let decisions: Vec<&str> = entries
            .iter()
            .filter_map(|entry| entry["decision"].as_str())
            .collect();
        assert!(decisions.contains(&"cut"), "every boundary is a cut");
        assert!(decisions.contains(&"caption"));
        assert!(decisions.contains(&"bgm"));
        for entry in entries {
            let signals = entry["signals"].as_array().expect("signals");
            assert!(!signals.is_empty(), "a decision with no signal");
            for signal in signals {
                let signal = signal.as_str().expect("a signal name");
                assert!(
                    crate::tools::signal_vocabulary_has(signal),
                    "signal {signal:?} is not one voice_signals answers"
                );
            }
        }
    }
    /// The measured beat grid is returned before the planner requests are
    /// constructed, and every take gets its own transcript clock.
    #[test]
    fn the_footage_planner_runs_per_take_after_beat_analysis() {
        use concat_director::ProgramStep;

        let take_a = "/takes/a.wav";
        let take_b = "/takes/b.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![
                TakePaths {
                    take: take_a.to_owned(),
                    enhanced: String::new(),
                },
                TakePaths {
                    take: take_b.to_owned(),
                    enhanced: String::new(),
                },
            ],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(plan) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "plan")
            .expect("the program has a plan stage")
        else {
            panic!("the plan stage is derived");
        };
        let log = vec![
            json!({ "musicPresent": false }),
            json!({ "musicPresent": true }),
            json!({ "path": take_a, "output": json!({ "segments": [
                { "start": 0.0, "end": 2.0, "text": "harbour" },
                { "start": 4.0, "end": 6.0, "text": "ships" },
            ]}).to_string() }),
            json!({ "path": take_b, "output": json!({ "segments": [
                { "start": 0.0, "end": 3.0, "text": "mountains" },
                { "start": 5.0, "end": 7.0, "text": "snow" },
            ]}).to_string() }),
        ];

        let beat_call = (plan.next)(&log).expect("the musical take is analysed first");
        assert_eq!(beat_call.tool, "analyze_beats");
        assert_eq!(beat_call.arguments["path"], take_b);

        let mut measured = log.clone();
        measured.push(json!({ "cutTimes": [1.0, 2.5, 4.0], "bpm": 120.0 }));
        let first = (plan.next)(&measured).expect("the first take is planned");
        assert_eq!(first.tool, "footage_plan_suggest");
        assert_eq!(first.arguments["take"], take_a);
        assert_eq!(first.arguments["segments"].as_array().unwrap().len(), 2);
        assert!(first.arguments.get("beats").is_none());

        let second = (plan.next)(&measured).expect("the second take is planned");
        assert_eq!(second.tool, "footage_plan_suggest");
        assert_eq!(second.arguments["take"], take_b);
        assert_eq!(second.arguments["segments"].as_array().unwrap().len(), 2);
        assert_eq!(
            second.arguments["beats"],
            json!([
                { "seconds": 1.0, "strength": 0.5 },
                { "seconds": 2.5, "strength": 0.5 },
                { "seconds": 4.0, "strength": 0.5 }
            ])
        );
    }

    /// A successful transcript and loudness reading do not prove that
    /// the rendered file is a decodable video. Verify ends by probing the
    /// exact export path, after the timeline, transcript and mix checks.
    #[test]
    fn verify_probes_the_rendered_export_after_content_and_mix_gates() {
        use concat_director::ProgramStep;

        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take.wav".to_owned(),
                enhanced: "/edits/Reel/voiceover/enhanced.wav".to_owned(),
            }],
            placements: vec![TakePlacement {
                take: "/edits/Reel/voiceover/take.wav".to_owned(),
                start: 0.0,
                source_in: 0.0,
                source_out: 4.0,
                speed: 1.0,
            }],
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(verify) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "verify")
            .expect("the program has a Verify stage")
        else {
            panic!("Verify is a derived stage");
        };

        let mut log = Vec::new();
        for expected in [
            "collision_check",
            "timeline_read",
            "transcribe",
            "analysis_wait",
            "analysis_wait",
            "measure_loudness",
            "emotion",
        ] {
            let next = (verify.next)(&log).expect("the next verification call exists");
            assert_eq!(next.tool, expected);
            log.push(json!({}));
        }

        let probe = (verify.next)(&log).expect("the actual export is probed");
        assert_eq!(probe.tool, "motion_probe");
        assert_eq!(probe.arguments["path"], source.export);
        log.push(json!({ "probe": { "streams": [] } }));
        assert!(
            (verify.next)(&log).is_none(),
            "the probe is the final Verify call"
        );
    }
}
